//! The server icon: the picture a game shows next to a server in its server
//! list (Minecraft's 64×64 `server-icon.png`), and the one the app shows on
//! the server's card. Which file and what size come from the manifest.
//!
//! The file in the server folder is the only source of truth. The app never
//! keeps its own copy, so an icon dropped in through the Files panel (or one
//! that came with an imported server) shows up the same as one set here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use base64::Engine;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::game::manifest::ServerIconSpec;
use crate::model::Instance;
use crate::service::InstanceService;

/// Generous for a photo off a phone; anything larger is not a picture
/// someone meant to use as a 64-pixel icon.
const MAX_SOURCE_BYTES: usize = 20 * 1024 * 1024;

/// Above this many pixels decoding alone costs real memory; refuse rather
/// than stall the app on a 20000×20000 PNG.
const MAX_SOURCE_PIXELS: u64 = 40_000_000;

/// An icon file bigger than this was not written by us and is not usable as
/// a 64-pixel picture; the card falls back to the initial.
const MAX_ICON_FILE_BYTES: u64 = 1024 * 1024;

/// How a non-square picture becomes a square.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconFit {
    /// Fill the square and trim the long side, centred. What people expect
    /// from a profile-picture style crop.
    #[default]
    Cover,
    /// Keep the whole picture, letterboxed on transparency.
    Contain,
}

impl InstanceService {
    /// Turn any picture into the server's icon. `data` is the file's bytes,
    /// base64-encoded (it crosses IPC or HTTP as JSON either way), or a whole
    /// `data:` URL.
    pub async fn set_icon(&self, instance_id: &str, data: &str, fit: IconFit) -> AppResult<()> {
        let instance = self.store.get(instance_id).await?;
        let (path, size) = self.icon_target(&instance)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(strip_data_url(data).trim())
            .map_err(|error| bad_image("unreadable").detail(error))?;
        // The game ignores an icon of any other size, so every picture is
        // brought to exactly the manifest's square.
        let png = tokio::task::spawn_blocking(move || render_icon(&bytes, fit, size))
            .await
            .map_err(AppError::unexpected)??;
        write_atomic(&path, &png)?;
        self.touch_icon(instance_id).await
    }

    /// Back to no icon: the game shows its default, the app shows the
    /// server's initial.
    pub async fn clear_icon(&self, instance_id: &str) -> AppResult<()> {
        let instance = self.store.get(instance_id).await?;
        let (path, _) = self.icon_target(&instance)?;
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.touch_icon(instance_id).await
    }

    /// The icon file of this instance's game, and its side in pixels.
    fn icon_target(&self, instance: &Instance) -> AppResult<(PathBuf, u32)> {
        let spec = self
            .registry
            .manifest(&instance.game)?
            .server_icon
            .ok_or_else(|| AppError::new(ErrorCode::GameNotSupported).with("game", &instance.game))?;
        Ok((crate::store::paths::resolve_inside(&instance.path, &spec.file)?, spec.size.clamp(16, 512)))
    }

    /// The stored instance carries a stamp that changes with the icon, so a
    /// UI holding the server list knows to repaint it.
    async fn touch_icon(&self, instance_id: &str) -> AppResult<()> {
        self.store
            .update(instance_id, |instance| {
                instance.icon = Some(chrono::Utc::now().timestamp_millis().to_string());
            })
            .await?;
        Ok(())
    }
}

/// The icon as a `data:` URL, or `None` when the server has none. Cached by
/// size and modification time, so listing twenty servers every few seconds
/// does not re-read twenty files.
pub fn icon_data_url(root: &Path, spec: Option<&ServerIconSpec>) -> Option<String> {
    /// Icon path → (modified, length, data URL).
    type Cached = HashMap<PathBuf, (SystemTime, u64, String)>;
    static CACHE: OnceLock<Mutex<Cached>> = OnceLock::new();
    let path = crate::store::paths::resolve_inside(root, &spec?.file).ok()?;
    let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
    if meta.len() == 0 || meta.len() > MAX_ICON_FILE_BYTES {
        return None;
    }
    let modified = meta.modified().ok()?;
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((at, len, url)) = cache.lock().ok()?.get(&path) {
        if *at == modified && *len == meta.len() {
            return Some(url.clone());
        }
    }
    let bytes = std::fs::read(&path).ok()?;
    let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
    if let Ok(mut map) = cache.lock() {
        map.insert(path, (modified, meta.len(), url.clone()));
    }
    Some(url)
}

/// Decode, square, scale, encode. GIFs and other animations give their first
/// frame, which is what a single decode returns.
pub fn render_icon(bytes: &[u8], fit: IconFit, size: u32) -> AppResult<Vec<u8>> {
    if bytes.is_empty() {
        return Err(bad_image("unreadable"));
    }
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(bad_image("too_large").with("limit_mb", MAX_SOURCE_BYTES / (1024 * 1024)));
    }
    let format = image::guess_format(bytes).map_err(|error| bad_image("unreadable").detail(error))?;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_SOURCE_PIXELS * 4);
    reader.limits(limits);
    let source = reader.decode().map_err(|error| bad_image("unreadable").detail(error))?;
    let (width, height) = source.dimensions();
    if width == 0 || height == 0 {
        return Err(bad_image("unreadable"));
    }
    if u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS {
        return Err(bad_image("too_large").with("limit_mb", MAX_SOURCE_BYTES / (1024 * 1024)));
    }

    let icon = square(source, fit, size);
    let mut out = std::io::Cursor::new(Vec::new());
    icon.write_to(&mut out, ImageFormat::Png).map_err(AppError::unexpected)?;
    Ok(out.into_inner())
}

fn square(source: DynamicImage, fit: IconFit, size: u32) -> DynamicImage {
    let (width, height) = source.dimensions();
    // Pixel art (a 16×16 block, a 32×32 sprite) stays crisp when it is
    // scaled up; smoothing it would blur the very edges that make it
    // readable at 64 pixels.
    let filter = if width <= size && height <= size { FilterType::Nearest } else { FilterType::Lanczos3 };
    match fit {
        IconFit::Cover => {
            let side = width.min(height);
            let x = (width - side) / 2;
            let y = (height - side) / 2;
            DynamicImage::ImageRgba8(source.crop_imm(x, y, side, side).resize_exact(size, size, filter).to_rgba8())
        }
        IconFit::Contain => {
            let scaled = source.resize(size, size, filter).to_rgba8();
            let mut canvas = image::RgbaImage::new(size, size);
            let x = i64::from((size - scaled.width()) / 2);
            let y = i64::from((size - scaled.height()) / 2);
            image::imageops::overlay(&mut canvas, &scaled, x, y);
            DynamicImage::ImageRgba8(canvas)
        }
    }
}

/// Accept either bare base64 or a whole `data:image/...;base64,` URL, since
/// that is what a browser's FileReader hands over.
fn strip_data_url(data: &str) -> &str {
    match data.find(";base64,") {
        Some(at) if data.starts_with("data:") => &data[at + ";base64,".len()..],
        _ => data,
    }
}

/// `invalid_input` with a precise reason the UI has its own copy for
/// (`invalid_input_image_unreadable`, `invalid_input_image_too_large`).
fn bad_image(reason: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput).with("reason", format!("image_{reason}"))
}

/// The game may be reading the old icon right now (a status ping); a
/// half-written PNG would show as no icon until the next restart.
fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".nebula-part");
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes)?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(image: DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, format).expect("encode");
        out.into_inner()
    }

    fn decoded(png: &[u8]) -> DynamicImage {
        image::load_from_memory_with_format(png, ImageFormat::Png).expect("valid png")
    }

    #[test]
    fn any_picture_becomes_a_square_png_of_the_manifest_size() {
        let wide = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(300, 120, image::Rgba([200, 10, 10, 255])));
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Bmp, ImageFormat::Gif] {
            let source = match format {
                ImageFormat::Jpeg => DynamicImage::ImageRgb8(wide.to_rgb8()),
                _ => wide.clone(),
            };
            let png = render_icon(&encode(source, format), IconFit::Cover, 64).expect("renders");
            assert_eq!(decoded(&png).dimensions(), (64, 64), "{format:?}");
        }
    }

    #[test]
    fn small_pixel_art_is_scaled_up_too() {
        let tiny = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(16, 16, image::Rgba([1, 2, 3, 255])));
        let png = render_icon(&encode(tiny, ImageFormat::Png), IconFit::Cover, 64).expect("renders");
        assert_eq!(decoded(&png).dimensions(), (64, 64));
    }

    #[test]
    fn cover_crops_the_centre_and_contain_letterboxes() {
        // Left third red, middle green, right third blue.
        let mut source = image::RgbaImage::new(300, 100);
        for (x, _, pixel) in source.enumerate_pixels_mut() {
            *pixel = match x {
                0..=99 => image::Rgba([255, 0, 0, 255]),
                100..=199 => image::Rgba([0, 255, 0, 255]),
                _ => image::Rgba([0, 0, 255, 255]),
            };
        }
        let bytes = encode(DynamicImage::ImageRgba8(source), ImageFormat::Png);

        let cover = decoded(&render_icon(&bytes, IconFit::Cover, 64).expect("cover")).to_rgba8();
        assert_eq!(cover.get_pixel(32, 32).0[1], 255, "the centre square is the green third");
        assert_eq!(cover.get_pixel(2, 32).0[1], 255);

        let contain = decoded(&render_icon(&bytes, IconFit::Contain, 64).expect("contain")).to_rgba8();
        assert_eq!(contain.get_pixel(32, 2).0[3], 0, "letterbox bands are transparent");
        assert_eq!(contain.get_pixel(2, 32).0[0], 255, "the whole width is kept");
    }

    #[test]
    fn garbage_and_empty_input_are_refused_as_input_errors() {
        for bytes in [&b""[..], &b"definitely not an image"[..]] {
            let error = render_icon(bytes, IconFit::Cover, 64).expect_err("refused");
            assert_eq!(error.code, ErrorCode::InvalidInput);
            assert_eq!(error.params.get("reason").map(String::as_str), Some("image_unreadable"));
        }
    }

    #[test]
    fn data_urls_and_bare_base64_both_decode() {
        assert_eq!(strip_data_url("data:image/png;base64,AAAA"), "AAAA");
        assert_eq!(strip_data_url("AAAA"), "AAAA");
    }

    #[test]
    fn the_icon_reads_back_as_a_data_url_and_tracks_changes() {
        let root = tempfile::tempdir().expect("tempdir");
        let spec = ServerIconSpec { file: "server-icon.png".into(), size: 64 };
        assert!(icon_data_url(root.path(), Some(&spec)).is_none(), "no file, no icon");
        assert!(icon_data_url(root.path(), None).is_none(), "a game without icons has none");

        let png = render_icon(
            &encode(DynamicImage::ImageRgba8(image::RgbaImage::new(8, 8)), ImageFormat::Png),
            IconFit::Cover,
            64,
        )
        .expect("renders");
        write_atomic(&root.path().join("server-icon.png"), &png).expect("writes");
        let url = icon_data_url(root.path(), Some(&spec)).expect("an icon");
        assert!(url.starts_with("data:image/png;base64,"));
    }
}
