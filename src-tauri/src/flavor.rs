//! Which build this is, and what it may install and run on this device.
//!
//! The regular build (`Full`) does everything. The Google Play build
//! (`PlayStore`, cargo feature `play-store`) must not download or run code
//! that did not come from Google Play — Play's Device and Network Abuse
//! policy — so on this device it:
//!
//! - plays only the official game (Vanilla): no mod loaders;
//! - adds only content that is not code: resource packs, never mods,
//!   modpacks or shader packs (they need a mod to work);
//! - checks every archive it does add, and refuses one that carries code
//!   (`.jar`, `.class`, `.dex`, native libraries, executables);
//! - does not host servers on the phone.
//!
//! The rules are enforced here, in the backend, so every way in (the UI, the
//! MCP tools, a dropped file) meets them. Managing another machine is not
//! affected: that machine's own build decides. See
//! `docs/google-play-proposal.md`.

use std::io::{Read, Seek};

use serde::Serialize;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::launcher::{ContentKind, Loader};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Flavor {
    Full,
    PlayStore,
}

impl Flavor {
    /// The flavor this binary was built as.
    pub const fn current() -> Self {
        if cfg!(feature = "play-store") {
            Self::PlayStore
        } else {
            Self::Full
        }
    }

    pub fn loader_allowed(self, loader: &Loader) -> bool {
        match self {
            Self::Full => true,
            Self::PlayStore => matches!(loader, Loader::Vanilla),
        }
    }

    pub fn content_allowed(self, kind: ContentKind) -> bool {
        match self {
            Self::Full => true,
            Self::PlayStore => matches!(kind, ContentKind::Resourcepack),
        }
    }

    /// Whether this device may run game servers itself.
    pub fn hosting_allowed(self) -> bool {
        self == Self::Full
    }

    /// Whether archives must be checked for code before they are kept.
    pub fn scans_archives(self) -> bool {
        self == Self::PlayStore
    }

    pub fn ensure_loader(self, loader: &Loader) -> AppResult<()> {
        if self.loader_allowed(loader) { Ok(()) } else { Err(not_in_build("loader")) }
    }

    pub fn ensure_content(self, kind: ContentKind) -> AppResult<()> {
        if self.content_allowed(kind) { Ok(()) } else { Err(not_in_build("content")) }
    }

    pub fn ensure_hosting(self) -> AppResult<()> {
        if self.hosting_allowed() { Ok(()) } else { Err(not_in_build("hosting")) }
    }

    /// Refuse an archive that carries code, in a build that must not run any
    /// from outside. `name` is only for the error.
    pub fn ensure_no_code<R: Read + Seek>(self, archive: R, name: &str) -> AppResult<()> {
        if !self.scans_archives() {
            return Ok(());
        }
        match code_entry(archive)? {
            None => Ok(()),
            Some(entry) => Err(not_in_build("code_in_archive").with("file", name).detail(entry)),
        }
    }
}

fn not_in_build(what: &str) -> AppError {
    AppError::new(ErrorCode::NotInThisBuild).with("reason", what)
}

/// File endings that are code a JVM or the system could load or run.
const CODE_ENDINGS: [&str; 10] = [".jar", ".class", ".dex", ".so", ".dll", ".dylib", ".jnilib", ".exe", ".sh", ".bat"];

/// The first entry of a zip archive that is code, if any. An archive that
/// cannot be read is refused rather than trusted.
pub fn code_entry<R: Read + Seek>(archive: R) -> AppResult<Option<String>> {
    let mut zip = zip::ZipArchive::new(archive)
        .map_err(|_| AppError::new(ErrorCode::InvalidInput).with("reason", "not a valid archive"))?;
    for index in 0..zip.len() {
        let entry = zip
            .by_index_raw(index)
            .map_err(|_| AppError::new(ErrorCode::InvalidInput).with("reason", "not a valid archive"))?;
        let name = entry.name().to_ascii_lowercase();
        // A native library is often versioned: libfoo.so.1
        let native_versioned = name.contains(".so.");
        if native_versioned || CODE_ENDINGS.iter().any(|ending| name.ends_with(ending)) {
            return Ok(Some(entry.name().to_string()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn zip_with(names: &[&str]) -> Cursor<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in names {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(b"x").unwrap();
        }
        let mut cursor = writer.finish().unwrap();
        cursor.set_position(0);
        cursor
    }

    #[test]
    fn full_build_allows_everything() {
        let full = Flavor::Full;
        for loader in [Loader::Vanilla, Loader::Fabric, Loader::Forge, Loader::Neoforge, Loader::Quilt] {
            assert!(full.loader_allowed(&loader));
        }
        for kind in [ContentKind::Mod, ContentKind::Modpack, ContentKind::Resourcepack, ContentKind::Shader] {
            assert!(full.content_allowed(kind));
        }
        assert!(full.hosting_allowed());
        // The full build does not scan, even an archive full of code.
        assert!(full.ensure_no_code(zip_with(&["mod.jar"]), "pack.zip").is_ok());
    }

    #[test]
    fn play_build_allows_only_the_official_game_and_resource_packs() {
        let play = Flavor::PlayStore;
        assert!(play.loader_allowed(&Loader::Vanilla));
        for loader in [Loader::Fabric, Loader::Forge, Loader::Neoforge, Loader::Quilt] {
            assert!(!play.loader_allowed(&loader));
            assert_eq!(play.ensure_loader(&loader).unwrap_err().code, ErrorCode::NotInThisBuild);
        }
        assert!(play.content_allowed(ContentKind::Resourcepack));
        for kind in [ContentKind::Mod, ContentKind::Modpack, ContentKind::Shader] {
            assert!(!play.content_allowed(kind));
            assert!(play.ensure_content(kind).is_err());
        }
        assert!(!play.hosting_allowed());
        assert!(play.ensure_hosting().is_err());
    }

    #[test]
    fn play_build_refuses_archives_that_carry_code() {
        let play = Flavor::PlayStore;
        assert!(play.ensure_no_code(zip_with(&["pack.mcmeta", "assets/minecraft/textures/block/stone.png"]), "ok.zip").is_ok());
        for bad in ["assets/x.jar", "Hidden.CLASS", "classes.dex", "lib/arm64/libx.so", "lib/libx.so.1", "run.sh", "x.dll"] {
            let error = play.ensure_no_code(zip_with(&["pack.mcmeta", bad]), "bad.zip").unwrap_err();
            assert_eq!(error.code, ErrorCode::NotInThisBuild, "{bad}");
        }
        // Not a zip at all: refused, not trusted.
        assert!(play.ensure_no_code(Cursor::new(b"not a zip".to_vec()), "x.zip").is_err());
    }

    #[test]
    fn current_matches_the_cargo_feature() {
        assert_eq!(Flavor::current() == Flavor::PlayStore, cfg!(feature = "play-store"));
    }
}
