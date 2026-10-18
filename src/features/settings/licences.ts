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

export const LICENCE_GROUPS: { key: "app" | "fonts" | "backend"; entries: LicenceEntry[] }[] = [
  {
    key: "app",
    entries: [
      { name: "Tauri", licence: "MIT / Apache-2.0", url: "https://github.com/tauri-apps/tauri" },
      { name: "React", licence: "MIT", url: "https://github.com/facebook/react" },
      { name: "TanStack Query", licence: "MIT", url: "https://github.com/TanStack/query" },
      { name: "React Router", licence: "MIT", url: "https://github.com/remix-run/react-router" },
      { name: "Tailwind CSS", licence: "MIT", url: "https://github.com/tailwindlabs/tailwindcss" },
      { name: "Vite", licence: "MIT", url: "https://github.com/vitejs/vite" },
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
];
