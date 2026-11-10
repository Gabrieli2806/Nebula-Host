/**
 * Open-source components Nebula Host ships with, for Settings › About ›
 * Licences. Names and licences are proper nouns, so they are not translated.
 *
 * Hand-kept list of the direct dependencies (package.json, src-tauri and
 * workspace Cargo.toml). Before a store release this should be generated
 * from the lockfiles (cargo-about, license-checker) with full texts.
 */

export interface LicenceEntry {
  name: string;
  licence: string;
  url: string;
}

export const LICENCE_GROUPS: {
  key: "app" | "fonts" | "backend" | "android";
  /** Shipped only in the Android app. */
  mobileOnly?: boolean;
  entries: LicenceEntry[];
}[] = [
  {
    key: "app",
    entries: [
      { name: "Tauri", licence: "MIT / Apache-2.0", url: "https://github.com/tauri-apps/tauri" },
      { name: "React", licence: "MIT", url: "https://github.com/facebook/react" },
      { name: "TanStack Query", licence: "MIT", url: "https://github.com/TanStack/query" },
      { name: "React Router", licence: "MIT", url: "https://github.com/remix-run/react-router" },
      { name: "Tailwind CSS", licence: "MIT", url: "https://github.com/tailwindlabs/tailwindcss" },
      { name: "Vite", licence: "MIT", url: "https://github.com/vitejs/vite" },
      { name: "skinview3d", licence: "MIT", url: "https://github.com/bs-community/skinview3d" },
    ],
  },
  {
    key: "fonts",
    entries: [
      { name: "Quicksand", licence: "SIL Open Font License 1.1", url: "https://github.com/andrew-paglinawan/QuicksandFamily" },
      { name: "JetBrains Mono", licence: "SIL Open Font License 1.1", url: "https://github.com/JetBrains/JetBrainsMono" },
    ],
  },
  {
    key: "backend",
    entries: [
      { name: "Tokio", licence: "MIT", url: "https://github.com/tokio-rs/tokio" },
      { name: "Axum", licence: "MIT", url: "https://github.com/tokio-rs/axum" },
      { name: "Reqwest", licence: "MIT / Apache-2.0", url: "https://github.com/seanmonstar/reqwest" },
      { name: "Serde", licence: "MIT / Apache-2.0", url: "https://github.com/serde-rs/serde" },
      { name: "Chrono", licence: "MIT / Apache-2.0", url: "https://github.com/chronotope/chrono" },
      { name: "image", licence: "MIT / Apache-2.0", url: "https://github.com/image-rs/image" },
      { name: "zip", licence: "MIT", url: "https://github.com/zip-rs/zip2" },
      { name: "igd-next", licence: "MIT", url: "https://github.com/dariusc93/rust-igd" },
      { name: "sysinfo", licence: "MIT", url: "https://github.com/GuillaumeGomez/sysinfo" },
      { name: "RustCrypto (sha1, sha2)", licence: "MIT / Apache-2.0", url: "https://github.com/RustCrypto/hashes" },
      { name: "tracing", licence: "MIT", url: "https://github.com/tokio-rs/tracing" },
      { name: "regex", licence: "MIT / Apache-2.0", url: "https://github.com/rust-lang/regex" },
      { name: "uuid", licence: "MIT / Apache-2.0", url: "https://github.com/uuid-rs/uuid" },
    ],
  },
  {
    // The Android game engine and the Java runtimes inside the APK. LGPL and
    // GPL: the links lead to the source of what is distributed.
    key: "android",
    mobileOnly: true,
    entries: [
      { name: "Amethyst-Android", licence: "LGPL-3.0", url: "https://github.com/AngelAuraMC/Amethyst-Android" },
      { name: "OpenJDK (game runtime)", licence: "GPL-2.0 with Classpath Exception", url: "https://github.com/AngelAuraMC/angelauramc-openjdk-build" },
      { name: "OpenJDK (server runtime, Termux build)", licence: "GPL-2.0 with Classpath Exception", url: "https://github.com/termux/termux-packages" },
      { name: "LWJGL 3", licence: "BSD-3-Clause", url: "https://github.com/LWJGL/lwjgl3" },
    ],
  },
];
