<#
.SYNOPSIS
  Package Nebula Host as MSIX for the Microsoft Store.

.DESCRIPTION
  Builds the frontend and the Rust binary for each requested architecture,
  stages the package contents, and packs them with makeappx.exe from the
  Windows SDK. Nothing here is committed: the whole output directory is
  ignored, and no certificate lives in the repository.

.PARAMETER Arch
  "x64", "arm64", or "both" (default).

.PARAMETER SkipBuild
  Package what is already in target/ instead of rebuilding.

.PARAMETER Bundle
  Also produce a .msixbundle spanning both architectures.

.PARAMETER Sign
  Path to a PFX to sign with. Store submissions are signed by the Store, so
  this is only for sideloading a test build.

.PARAMETER PackageName
  Package identity name. Defaults to the one Partner Center assigned to the
  reserved "Nebula Host" listing; pass another for a sideload-only build.

.PARAMETER Publisher
  Package identity publisher. Defaults to the Store-assigned value (the Store
  signs the package, so it must match what Partner Center expects).

.PARAMETER CertThumbprint
  Thumbprint of a certificate already in the local store, as an alternative to
  -Sign. Never hard-code one here.
#>
param(
    [ValidateSet("x64", "arm64", "both")]
    [string]$Arch = "x64",
    [switch]$SkipBuild,
    [switch]$SkipFrontend,
    [switch]$Bundle,
    [string]$Sign,
    [string]$CertThumbprint,
    [string]$PackageName = "44733G2806.NebulaHost",
    [string]$Publisher = "CN=334A4A5F-C172-4419-93DE-B4E9376FFA14"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# ── Paths ───────────────────────────────────────────────────────────────────
$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Definition
$TauriRoot   = Split-Path -Parent $ScriptDir   # src-tauri
$ProjectRoot = Split-Path -Parent $TauriRoot   # repository root
$IconsDir    = Join-Path $TauriRoot "icons"
$ManifestTpl = Join-Path $ScriptDir "AppxManifest.xml"
$OutDir      = Join-Path $ScriptDir "out"

$TauriConf  = Get-Content (Join-Path $TauriRoot "tauri.conf.json") -Raw | ConvertFrom-Json
$AppVersion = $TauriConf.version
# MSIX insists on four version parts.
while (($AppVersion -split '\.').Count -lt 4) { $AppVersion = "$AppVersion.0" }

$AppId          = "NebulaHost"
$DisplayName    = "Nebula Host"
$Description    = "Nebula Host - Game Server Manager"
$ExecutableName = "Nebula Host.exe"
$CargoBinName   = "nebula-host"

# ── Windows SDK tools ───────────────────────────────────────────────────────
function Find-SdkTool([string]$Name) {
    $roots = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin",
        "$env:ProgramFiles\Windows Kits\10\bin"
    ) | Where-Object { Test-Path $_ }

    foreach ($root in $roots) {
        $found = Get-ChildItem $root -Recurse -Filter $Name -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending | Select-Object -First 1
        if ($found) { return $found.FullName }
    }
    return $null
}

$MakeAppx = Find-SdkTool "makeappx.exe"
if (-not $MakeAppx) {
    throw "makeappx.exe not found. Install the Windows 10/11 SDK (App Certification Kit component)."
}
$SignTool = Find-SdkTool "signtool.exe"

$ArchMap = @{
    "x64"   = @{ RustTarget = "x86_64-pc-windows-msvc";  MsixArch = "x64"   }
    "arm64" = @{ RustTarget = "aarch64-pc-windows-msvc"; MsixArch = "arm64" }
}
$Targets = if ($Arch -eq "both") { @("x64", "arm64") } else { @($Arch) }

if (-not (Test-Path $OutDir)) { New-Item $OutDir -ItemType Directory -Force | Out-Null }

# ── Frontend, once for every architecture ───────────────────────────────────
if (-not $SkipBuild -and -not $SkipFrontend) {
    Write-Host "[frontend] building..." -ForegroundColor Yellow
    Push-Location $ProjectRoot
    & npm run build
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "Frontend build failed." }
    Pop-Location
}

$RequiredAssets = @(
    "Square44x44Logo.png",
    "Square71x71Logo.png",
    "Square150x150Logo.png",
    "Square310x310Logo.png",
    "Wide310x150Logo.png",
    "StoreLogo.png"
)

foreach ($target in $Targets) {
    $info       = $ArchMap[$target]
    $rustTarget = $info.RustTarget
    $msixArch   = $info.MsixArch

    Write-Host "`n── $msixArch ($rustTarget) ──" -ForegroundColor Cyan

    # `tauri-build` does not emit rerun-if-env-changed=TAURI_DEV, so after a
    # `tauri dev` session Cargo happily reuses a cfg(dev) build script output —
    # and the packaged app then tries to load the app from a dev server that is
    # not running. Delete those stale outputs so the build script reruns.
    if (-not $SkipBuild) {
        $buildCache = Join-Path $ProjectRoot "target\$rustTarget\release\build"
        $fpCache    = Join-Path $ProjectRoot "target\$rustTarget\release\.fingerprint"
        foreach ($pattern in @("tauri-*", "nebula-host-*")) {
            foreach ($dir in (Get-ChildItem $buildCache -Filter $pattern -ErrorAction SilentlyContinue)) {
                $outputFile = Join-Path $dir.FullName "output"
                if ((Test-Path $outputFile) -and ((Get-Content $outputFile -Raw) -match "rustc-cfg=dev")) {
                    Write-Host "[clean] stale dev artifact: $($dir.Name)" -ForegroundColor DarkYellow
                    Remove-Item $dir.FullName -Recurse -Force
                    $fpDir = Join-Path $fpCache $dir.Name
                    if (Test-Path $fpDir) { Remove-Item $fpDir -Recurse -Force }
                }
            }
        }

        Write-Host "[cargo] release build..." -ForegroundColor Yellow
        Push-Location $ProjectRoot
        & npx tauri build --no-bundle --target $rustTarget
        if ($LASTEXITCODE -ne 0) { Pop-Location; throw "Build failed for $rustTarget." }
        Pop-Location
    }

    $ExePath = Join-Path $ProjectRoot "target\$rustTarget\release\$CargoBinName.exe"
    if (-not (Test-Path $ExePath)) { throw "Binary not found: $ExePath" }

    # ── Stage ──
    $StageDir = Join-Path $OutDir "stage-$msixArch"
    if (Test-Path $StageDir) { Remove-Item $StageDir -Recurse -Force }
    New-Item (Join-Path $StageDir "Assets") -ItemType Directory -Force | Out-Null

    Copy-Item $ExePath (Join-Path $StageDir $ExecutableName)

    $Wv2Loader = Join-Path $ProjectRoot "target\$rustTarget\release\WebView2Loader.dll"
    if (Test-Path $Wv2Loader) { Copy-Item $Wv2Loader $StageDir }

    # Game manifests ship with the app: they are how it knows what it can run.
    Copy-Item (Join-Path $TauriRoot "games") $StageDir -Recurse -Force

    foreach ($asset in $RequiredAssets) {
        $src = Join-Path $IconsDir $asset
        if (Test-Path $src) {
            Copy-Item $src (Join-Path $StageDir "Assets\$asset")
        } else {
            Write-Warning "Missing icon asset: $asset (the package will fail validation)"
        }
    }

    $manifest = (Get-Content $ManifestTpl -Raw).
        Replace("{{VERSION}}", $AppVersion).
        Replace("{{ARCH}}", $msixArch).
        Replace("{{PACKAGE_NAME}}", $PackageName).
        Replace("{{PUBLISHER}}", $Publisher).
        Replace("{{DISPLAY_NAME}}", $DisplayName).
        Replace("{{DESCRIPTION}}", $Description).
        Replace("{{EXECUTABLE_NAME}}", $ExecutableName).
        Replace("{{APP_ID}}", $AppId)
    Set-Content -Path (Join-Path $StageDir "AppxManifest.xml") -Value $manifest -Encoding UTF8

    # ── Pack ──
    $MsixFile = Join-Path $OutDir ("NebulaHost_{0}_{1}.msix" -f $AppVersion, $msixArch)
    & $MakeAppx pack /d $StageDir /p $MsixFile /o
    if ($LASTEXITCODE -ne 0) { throw "makeappx pack failed for $msixArch." }
    Write-Host "packed: $MsixFile" -ForegroundColor Green

    if ($SignTool -and ($Sign -or $CertThumbprint)) {
        if ($Sign) {
            & $SignTool sign /fd SHA256 /a /f $Sign $MsixFile
        } else {
            & $SignTool sign /fd SHA256 /sha1 $CertThumbprint $MsixFile
        }
        if ($LASTEXITCODE -ne 0) { Write-Warning "Signing failed; the package is unsigned." }
    }
}

# ── Bundle ──────────────────────────────────────────────────────────────────
if ($Bundle -and $Targets.Count -gt 1) {
    $BundleFile  = Join-Path $OutDir ("NebulaHost_{0}.msixbundle" -f $AppVersion)
    $BundleStage = Join-Path $OutDir "bundle-stage"
    if (Test-Path $BundleStage) { Remove-Item $BundleStage -Recurse -Force }
    New-Item $BundleStage -ItemType Directory -Force | Out-Null

    Get-ChildItem $OutDir -Filter ("NebulaHost_{0}_*.msix" -f $AppVersion) |
        ForEach-Object { Copy-Item $_.FullName $BundleStage }

    & $MakeAppx bundle /d $BundleStage /p $BundleFile /o
    if ($LASTEXITCODE -ne 0) { throw "makeappx bundle failed." }

    if ($SignTool -and ($Sign -or $CertThumbprint)) {
        if ($Sign) { & $SignTool sign /fd SHA256 /a /f $Sign $BundleFile }
        else { & $SignTool sign /fd SHA256 /sha1 $CertThumbprint $BundleFile }
    }
    Write-Host "bundled: $BundleFile" -ForegroundColor Green
}

Write-Host ""
Get-ChildItem $OutDir -Filter "*.msix*" | ForEach-Object {
    Write-Host ("  {0}  ({1} MB)" -f $_.Name, [math]::Round($_.Length / 1MB, 1))
}
Write-Host ""
Write-Host "  Sideload with: Add-AppxPackage -Path .\<file>.msix" -ForegroundColor DarkGray
Write-Host "  (needs Developer Mode, and a signed package)" -ForegroundColor DarkGray
