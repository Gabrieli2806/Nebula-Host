param([string]$JavaHome = 'C:\Program Files\Java\jdk-21.0.10')
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $taskRoot
if (-not (Test-Path -LiteralPath (Join-Path $JavaHome 'bin\java.exe'))) {
    throw 'Install a JDK 21 and pass its folder using -JavaHome.'
}
$env:JAVA_HOME = $JavaHome
$env:Path = "$JavaHome\bin;$env:Path"
git submodule update --init src-tauri/android/engine
if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare the pinned game engine.' }
git -C src-tauri/android/engine -c submodule.androidnsbypass.url=https://github.com/alexytomi/androidnsbypass.git submodule update --init androidnsbypass
if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare the native game dependency.' }
node scripts/android-game-jre.mjs
if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare graphical Java.' }
node scripts/android-jre.mjs arm64-v8a 21,25
if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare server/installer Java.' }
& .\node_modules\.bin\tauri.cmd android build --debug --target aarch64 --apk --ci
if ($LASTEXITCODE -ne 0) { throw 'Android build failed.' }
New-Item -ItemType Directory -Force dist-android | Out-Null
Copy-Item -LiteralPath src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk -Destination dist-android/NebulaHost-launcher-arm64-debug.apk
Write-Output 'APK: dist-android/NebulaHost-launcher-arm64-debug.apk'
