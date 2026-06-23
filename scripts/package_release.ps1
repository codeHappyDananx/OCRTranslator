param(
  [switch]$SkipTauriBundle
)

$ErrorActionPreference = "Stop"

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$AppDir = Join-Path $ProjectRoot "crates\app-tauri"
$ResourceDir = Join-Path $AppDir "resources\SnippingTool"
$InstallerDir = Join-Path $ProjectRoot "installer"
$PortableDir = Join-Path $InstallerDir ("OCR-Translator-portable-build-{0:yyyyMMddHHmmss}" -f (Get-Date))
$PortableZip = Join-Path $InstallerDir "OCR-Translator-portable-v0.1.0.zip"
$ReleaseExe = Join-Path $ProjectRoot "target\release\ocr-translator.exe"

function Test-OneOcrRuntime([string]$Dir) {
  return (
    (Test-Path (Join-Path $Dir "oneocr.dll")) -and
    (Test-Path (Join-Path $Dir "oneocr.onemodel")) -and
    (Test-Path (Join-Path $Dir "onnxruntime.dll"))
  )
}

Push-Location $ProjectRoot
try {
  Stop-Process -Name OCR-Translator,ocr-translator -Force -ErrorAction SilentlyContinue

  Push-Location $AppDir
  try {
    if (-not (Test-Path (Join-Path $AppDir "node_modules"))) {
      & npm.cmd install
    }
    & npm.cmd run build
  }
  finally {
    Pop-Location
  }

  if (-not (Test-OneOcrRuntime $ResourceDir)) {
    Write-Host "Preparing bundled OneOCR runtime..."
    $localCache = Join-Path $env:LOCALAPPDATA "OCR-Translator\SnippingTool"
    if (-not (Test-OneOcrRuntime $localCache)) {
      cargo run -q -p app-windows --example ocr_probe -- install-oneocr | Write-Host
    }
    if (-not (Test-OneOcrRuntime $localCache)) {
      throw "OneOCR runtime is unavailable after preparation: $localCache"
    }
    if (Test-Path $ResourceDir) {
      Remove-Item -LiteralPath $ResourceDir -Recurse -Force
    }
    New-Item -ItemType Directory -Path $ResourceDir | Out-Null
    Get-ChildItem -LiteralPath $localCache -Force | Copy-Item -Destination $ResourceDir -Recurse -Force
  }

  cargo fmt --check
  cargo check
  cargo test -p ocr-translator
  powershell -ExecutionPolicy Bypass -File scripts\ui_state_regression.ps1
  powershell -ExecutionPolicy Bypass -File scripts\ocr_regression.ps1 -Engine oneocr
  cargo build -p ocr-translator --release

  if (-not (Test-Path $InstallerDir)) {
    New-Item -ItemType Directory -Path $InstallerDir | Out-Null
  }
  Get-ChildItem -LiteralPath $InstallerDir -Directory -Filter "OCR-Translator-portable-build-*" -ErrorAction SilentlyContinue |
    ForEach-Object {
      try {
        Remove-Item -LiteralPath $_.FullName -Recurse -Force -ErrorAction Stop
      } catch {
        Write-Warning "旧便携构建目录正在使用，跳过清理：$($_.FullName)"
      }
    }
  New-Item -ItemType Directory -Path $PortableDir | Out-Null
  Copy-Item -LiteralPath $ReleaseExe -Destination (Join-Path $PortableDir "OCR-Translator.exe") -Force
  Copy-Item -LiteralPath (Join-Path $ProjectRoot "README.md") -Destination (Join-Path $PortableDir "README.md") -Force
  Copy-Item -LiteralPath (Join-Path $ProjectRoot "docs\THIRD_PARTY.md") -Destination (Join-Path $PortableDir "THIRD_PARTY.md") -Force
  Copy-Item -LiteralPath $ResourceDir -Destination (Join-Path $PortableDir "SnippingTool") -Recurse -Force
  if (Test-Path $PortableZip) {
    Remove-Item -LiteralPath $PortableZip -Force
  }
  Compress-Archive -Path (Join-Path $PortableDir "*") -DestinationPath $PortableZip -Force
  try {
    Remove-Item -LiteralPath $PortableDir -Recurse -Force -ErrorAction Stop
  } catch {
    Write-Warning "便携构建临时目录正在使用，跳过清理：$PortableDir"
  }

  if (-not $SkipTauriBundle) {
    cargo tauri build
  }

  Write-Host "Portable package: $PortableZip"
  $BundleDir = Join-Path $ProjectRoot "target\release\bundle"
  if (Test-Path $BundleDir) {
    Get-ChildItem $BundleDir -Recurse -File | Select-Object FullName, Length
  }
}
finally {
  Pop-Location
}
