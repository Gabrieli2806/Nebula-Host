/* global console, fetch */
// Bundle full graphical Android OpenJDK runtimes from the engine's runtime publisher.
// SHA-256 values pin the release assets used for this build; never run a host JVM.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const versions = {
  21: '8d41ec401ee59f7722df60ed991f81ad146e130452804bfdd8a05d3436f7bbfe',
  25: 'd3eb7afe2240c26728a1bb440502c5f18ac3883e932d202dd7f0c9bcbbce4c37',
};
for (const [major, checksum] of Object.entries(versions)) {
  const cache = join(root, '.cache', 'android-game-jre');
  mkdirSync(cache, { recursive: true });
  const archive = join(cache, `jre${major}-android-arm64.tar.xz`);
  if (!existsSync(archive)) {
    console.log(`Downloading graphical Android Java ${major}…`);
    const response = await fetch(`https://github.com/AngelAuraMC/angelauramc-openjdk-build/releases/download/download_jre${major}/jre${major}-android-arm64.tar.xz`);
    if (!response.ok) throw new Error(`Java ${major}: HTTP ${response.status}`);
    writeFileSync(archive, new Uint8Array(await response.arrayBuffer()));
  }
  if (createHash('sha256').update(readFileSync(archive)).digest('hex') !== checksum) throw new Error(`Java ${major}: checksum mismatch`);
  const target = join(root, 'src-tauri', 'android', 'game-runtime', 'src', 'main', 'assets', 'game-jre', major);
  if (!existsSync(join(target, 'release'))) {
    mkdirSync(target, { recursive: true });
    const result = spawnSync('tar', ['-xJf', archive, '-C', target], { stdio: 'inherit' });
    if (result.status !== 0) throw new Error(`Java ${major}: extraction failed`);
    if (!existsSync(join(target, 'release'))) throw new Error(`Java ${major}: unexpected runtime layout`);
  }
  console.log(`Android Java ${major}: verified and bundled`);
}
