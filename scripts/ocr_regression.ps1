param(
  [string]$ProjectRoot = "F:\AI\dn-ocr-translator"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function New-TestImage {
  param(
    [string]$Path,
    [int]$Width,
    [int]$Height,
    [scriptblock]$Draw
  )
  Add-Type -AssemblyName System.Drawing
  $bmp = New-Object System.Drawing.Bitmap $Width, $Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.Clear([System.Drawing.Color]::White)
  $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit
  & $Draw $g
  $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose()
  $bmp.Dispose()
}

function Invoke-OcrProbe {
  param(
    [string]$Engine,
    [string]$ImagePath
  )
  Push-Location $ProjectRoot
  try {
    $output = & cargo run -q -p app-windows --example ocr_probe -- $Engine $ImagePath 2>&1
    if ($LASTEXITCODE -ne 0) {
      throw "OCR probe failed for $Engine $ImagePath`n$output"
    }
    return ($output -join "`n").Trim()
  } finally {
    Pop-Location
  }
}

function Assert-Contains {
  param(
    [string]$Name,
    [string]$Actual,
    [string[]]$ExpectedParts
  )
  foreach ($part in $ExpectedParts) {
    if ($Actual -notlike "*$part*") {
      throw "[$Name] expected to contain '$part' but got:`n$Actual"
    }
  }
  Write-Host "[PASS] $Name => $Actual"
}

$target = Join-Path $ProjectRoot "target\ocr-regression"
New-Item -ItemType Directory -Force -Path $target | Out-Null

$english = Join-Path $target "english.png"
New-TestImage $english 900 240 {
  param($g)
  $font = New-Object System.Drawing.Font "Arial", 42, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
  $brush = [System.Drawing.Brushes]::Black
  $g.DrawString("Cooldown reduction applies to all skills.", $font, $brush, 24, 42)
  $g.DrawString("Press MouseX1 to translate selected text.", $font, $brush, 24, 122)
}

$mixed = Join-Path $target "mixed.png"
New-TestImage $mixed 980 260 {
  param($g)
  $font = New-Object System.Drawing.Font "Microsoft YaHei UI", 40, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
  $brush = [System.Drawing.Brushes]::Black
  $g.DrawString("Cooldown 冷却时间 applies to all skills 技能.", $font, $brush, 24, 44)
  $g.DrawString("Quest 任务 and Inventory 背包 are mixed text.", $font, $brush, 24, 130)
}

$downloadsFull = Join-Path $target "downloads_full.png"
$downloadsCrop = Join-Path $target "downloads_crop.png"
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap 520, 180
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(245, 245, 245))
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit
$fontSmall = New-Object System.Drawing.Font "Microsoft YaHei UI", 18, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
$font = New-Object System.Drawing.Font "Segoe UI", 20, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
$brushGray = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(130, 130, 130))
$brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(90, 95, 100))
$g.DrawString("项目", $fontSmall, $brushGray, 10, 8)
$g.DrawString("Downloads", $font, $brush, 36, 52)
$g.DrawString("这个目录下有几个PDF,是我的银行流水工...", $fontSmall, $brush, 36, 102)
$bmp.Save($downloadsFull, [System.Drawing.Imaging.ImageFormat]::Png)
$rect = New-Object System.Drawing.Rectangle 32, 48, 125, 32
$cropBmp = $bmp.Clone($rect, $bmp.PixelFormat)
$cropBmp.Save($downloadsCrop, [System.Drawing.Imaging.ImageFormat]::Png)
$cropBmp.Dispose()
$g.Dispose()
$bmp.Dispose()

$englishWindows = Invoke-OcrProbe "windows" $english
Assert-Contains "windows english" $englishWindows @("Cooldown reduction", "MouseX1")

$mixedOneOcr = Invoke-OcrProbe "oneocr" $mixed
Assert-Contains "oneocr mixed" $mixedOneOcr @("Cooldown", "冷却时间", "Quest", "任务", "Inventory", "背包")

$downloadsOneOcr = Invoke-OcrProbe "oneocr" $downloadsCrop
Assert-Contains "oneocr tight crop" $downloadsOneOcr @("Downloads")
if ($downloadsOneOcr -like "*项目*" -or $downloadsOneOcr -like "*PDF*" -or $downloadsOneOcr -like "*银行*") {
  throw "[oneocr tight crop] OCR leaked neighboring rows:`n$downloadsOneOcr"
}

Write-Host "OCR regression passed."
