#!/usr/bin/env node
/**
 * Prepare the Java runtimes that ship inside the Android app.
 *
 *   node scripts/android-jre.mjs <abi> <major>[,<major>…]
 *   node scripts/android-jre.mjs arm64-v8a 21
 *
 * Android cannot download and run a Java runtime (it may not execute files
 * from its data folder, and Google Play forbids fetching executable code), so
 * the runtimes are part of the package. They are OpenJDK builds for Android
 * taken from the Termux package repository (GPLv2 + Classpath Exception),
 * trimmed to what a headless server needs, and written to
 * `src-tauri/gen/android/app/src/main/assets/jre-<abi>/`, from where the app
 * copies them to its private folder on first start.
 *
 * Needs `xz` and `tar` on the PATH. Downloads are cached in `.cache/android-jre`.
 */

import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const [abi, majorsArg = "21"] = process.argv.slice(2);
const TERMUX_ARCH = { "arm64-v8a": "aarch64", x86_64: "x86_64" }[abi];
const NDK_TRIPLE = { "arm64-v8a": "aarch64-linux-android", x86_64: "x86_64-linux-android" }[abi];
if (!TERMUX_ARCH) {
  console.error("usage: android-jre.mjs <arm64-v8a|x86_64> <major>[,<major>…]");
  process.exit(2);
}
const majors = majorsArg.split(",").map(Number);

const BASE = "https://packages.termux.dev/apt/termux-main";
const cache = join(root, ".cache", "android-jre", TERMUX_ARCH);
mkdirSync(cache, { recursive: true });

async function packageIndex() {
  const file = join(cache, "Packages");
  if (!existsSync(file) || Date.now() - statSync(file).mtimeMs > 24 * 3600 * 1000) {
    const response = await fetch(`${BASE}/dists/stable/main/binary-${TERMUX_ARCH}/Packages`);
    if (!response.ok) throw new Error(`index: ${response.status}`);
    writeFileSync(file, Buffer.from(await response.arrayBuffer()));
  }
  const entries = new Map();
  for (const block of readFileSync(file, "utf8").split("\n\n")) {
    const name = /^Package: (.+)$/m.exec(block)?.[1];
    const filename = /^Filename: (.+)$/m.exec(block)?.[1];
    if (name && filename) entries.set(name, filename);
  }
  return entries;
}

async function fetchDeb(index, name) {
  const filename = index.get(name);
  if (!filename) throw new Error(`Termux has no package ${name} for ${TERMUX_ARCH}`);
  const target = join(cache, filename.split("/").pop());
  if (!existsSync(target)) {
    console.log(`  download ${filename}`);
    const response = await fetch(`${BASE}/${filename}`);
    if (!response.ok) throw new Error(`${name}: ${response.status}`);
    writeFileSync(target, Buffer.from(await response.arrayBuffer()));
  }
  return target;
}

/** A .deb is an `ar` archive; its payload is `data.tar.xz`. */
function unpackDeb(deb, into) {
  const bytes = readFileSync(deb);
  let offset = 8;
  while (offset < bytes.length) {
    const name = bytes.subarray(offset, offset + 16).toString().trim().replace(/\/$/, "");
    const size = parseInt(bytes.subarray(offset + 48, offset + 58).toString(), 10);
    offset += 60;
    if (name === "data.tar.xz") {
      const tarFile = deb + ".tar.xz";
      writeFileSync(tarFile, bytes.subarray(offset, offset + size));
      // Symlinks into Termux's own prefix cannot be created on every host;
      // they are not needed, so ignore those errors.
      // Relative paths and cwd: a drive letter in an argument is read as a remote host by GNU tar.
      const result = spawnSync("tar", ["-xJf", relative(into, tarFile)], { cwd: into, stdio: "pipe" });
      if (result.error) throw result.error;
      rmSync(tarFile);
      return;
    }
    offset += size + (size % 2);
  }
  throw new Error(`no data in ${deb}`);
}

function ndkLibCxx() {
  const sdk = process.env.ANDROID_HOME ?? process.env.ANDROID_SDK_ROOT;
  const ndkRoot = join(sdk, "ndk");
  const version = readdirSync(ndkRoot).sort().pop();
  const host = readdirSync(join(ndkRoot, version, "toolchains", "llvm", "prebuilt"))[0];
  return join(ndkRoot, version, "toolchains", "llvm", "prebuilt", host, "sysroot", "usr", "lib", NDK_TRIPLE, "libc++_shared.so");
}

const out = join(root, "src-tauri", "gen", "android", "app", "src", "main", "assets", `jre-${abi}`);
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const index = await packageIndex();
const prefix = ["data", "data", "com.termux", "files", "usr"];

// Libraries every runtime needs and Android does not have.
const extra = join(out, "extra");
mkdirSync(extra, { recursive: true });
const extraRoot = join(cache, "extra-root");
rmSync(extraRoot, { recursive: true, force: true });
mkdirSync(extraRoot, { recursive: true });
for (const name of ["libandroid-shmem", "libandroid-spawn", "libiconv", "zlib"]) {
  unpackDeb(await fetchDeb(index, name), extraRoot);
}
const libDir = join(extraRoot, ...prefix, "lib");
for (const file of readdirSync(libDir)) {
  if (/^lib(android-shmem|android-spawn|iconv|charset)\.so$/.test(file)) cpSync(join(libDir, file), join(extra, file));
}
const zlib = readdirSync(libDir).find((file) => /^libz\.so\.\d+\.\d+\.\d+$/.test(file));
cpSync(join(libDir, zlib), join(extra, "libz.so.1"));
cpSync(ndkLibCxx(), join(extra, "libc++_shared.so"));

for (const major of majors) {
  console.log(`Java ${major} (${abi})`);
  const work = join(cache, `jdk-${major}`);
  rmSync(work, { recursive: true, force: true });
  mkdirSync(work, { recursive: true });
  unpackDeb(await fetchDeb(index, `openjdk-${major}`), work);
  const jvmDir = join(work, ...prefix, "lib", "jvm");
  const home = join(jvmDir, readdirSync(jvmDir).find((name) => name.startsWith("java-")));
  // A headless server needs none of these.
  for (const unwanted of ["jmods", "demo", "man", "include", "legal", "lib/ct.sym", "lib/src.zip"]) {
    rmSync(join(home, unwanted), { recursive: true, force: true });
  }
  // Only `bin/java` stays, as the marker the app looks for: the tools in bin/
  // are programs Android will not run from the data folder.
  for (const tool of readdirSync(join(home, "bin"))) if (tool !== "java") rmSync(join(home, "bin", tool), { force: true });
  cpSync(home, join(out, String(major)), { recursive: true, dereference: true });
}

// One line per file: the app copies exactly these (listing assets at run time
// is slow and does not say what is a folder).
const files = [];
(function walk(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) walk(path);
    else files.push(relative(out, path).replaceAll("\\", "/"));
  }
})(out);
writeFileSync(join(out, "files.txt"), files.join("\n") + "\n");
console.log(`${files.length} files → ${relative(root, out)}`);
