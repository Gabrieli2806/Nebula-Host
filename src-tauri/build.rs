fn main() {
    // `desktop` / `mobile` are set by tauri-build. Declare them always, so the
    // headless node build (no Tauri) does not warn about an unknown cfg.
    println!("cargo::rustc-check-cfg=cfg(desktop)");
    println!("cargo::rustc-check-cfg=cfg(mobile)");

    // Only the desktop app embeds the Tauri config and icons; the headless
    // node binary has no window to describe.
    #[cfg(feature = "desktop")]
    tauri_build::build();
}
