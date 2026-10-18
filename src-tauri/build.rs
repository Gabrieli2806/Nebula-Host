fn main() {
    // Only the desktop app embeds the Tauri config and icons; the headless
    // node binary has no window to describe.
    #[cfg(feature = "desktop")]
    tauri_build::build();
}
