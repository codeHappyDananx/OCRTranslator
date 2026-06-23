$ErrorActionPreference = "Stop"
$ProjectRoot = "F:\AI\dn-ocr-translator"

$main = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\src\main.rs") -Raw
$selection = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\selection.html") -Raw
$index = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\index.html") -Raw
$ui = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\main.js") -Raw
$overlay = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\overlay.html") -Raw
$tauriConfig = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\tauri.conf.json") -Raw

$checks = @(
  @{ Name = "start hides old overlay"; Pattern = 'get_webview_window\("overlay"\)' },
  @{ Name = "start clears overlay payload"; Pattern = 'clear_overlay_payload\(app\)' },
  @{ Name = "start emits selection reset"; Pattern = 'emit\("selection-reset"' },
  @{ Name = "selection listens reset"; Pattern = 'listen\("selection-reset", resetSelection\)' },
  @{ Name = "selection blur reset"; Pattern = 'addEventListener\("blur", resetSelection\)' },
  @{ Name = "selection hint mentions right click cancel"; Pattern = '右键或 Esc 取消' },
  @{ Name = "selection right click cancels"; Pattern = 'event\.button === 2' },
  @{ Name = "selection blocks context menu"; Pattern = 'addEventListener\("contextmenu", cancelSelection\)' },
  @{ Name = "small left click auto detects region"; Pattern = 'selection_auto_detect' }
)

foreach ($check in $checks) {
  $source = if ($check.Name -like "selection*") { $selection } else { $main }
  if ($source -notmatch $check.Pattern) {
    throw "[FAIL] $($check.Name)"
  }
  Write-Host "[PASS] $($check.Name)"
}

$mustHave = @(
  @{ Name = "hotkey capture input"; Pattern = 'id="hotkey" class="hotkey-input" type="text" readonly' },
  @{ Name = "no visible OCR engine select"; Pattern = 'id="ocrStatus"' },
  @{ Name = "source plus translation setting"; Pattern = 'id="showSource"' },
  @{ Name = "overlay draggable setting"; Pattern = 'id="overlayDraggable"' },
  @{ Name = "source background color picker"; Pattern = 'id="sourceBackground" type="color"' },
  @{ Name = "translation background color picker"; Pattern = 'id="translationBackground" type="color"' },
  @{ Name = "overlay content resize command"; Pattern = 'resize_overlay_to_content' },
  @{ Name = "overlay width resize command"; Pattern = 'resize_overlay_width' },
  @{ Name = "overlay drag command"; Pattern = 'start_overlay_drag' },
  @{ Name = "settings autosave function"; Pattern = 'function scheduleSave' },
  @{ Name = "show source participates in autosave"; Pattern = 'els\.showSource' }
)

foreach ($check in $mustHave) {
  if ($index -notmatch $check.Pattern -and $overlay -notmatch $check.Pattern -and $main -notmatch $check.Pattern -and $ui -notmatch $check.Pattern) {
    throw "[FAIL] $($check.Name)"
  }
  Write-Host "[PASS] $($check.Name)"
}

if ($index -notmatch '<title>OCR Translator</title>' -or $index -notmatch '<h1>OCR Translator</h1>' -or $index -match 'DN OCR') {
  throw "[FAIL] app title still contains DN or does not use OCR Translator"
}
Write-Host "[PASS] app title uses OCR Translator"

if ($tauriConfig -notmatch '"productName":\s*"OCR Translator"' -or $tauriConfig -notmatch '"title":\s*"OCR Translator"' -or $tauriConfig -notmatch '"height":\s*970') {
  throw "[FAIL] Tauri window title/product/height are not updated"
}
Write-Host "[PASS] Tauri window title/product/height updated"

if ($overlay -match '#translationText\s*\{[^}]*overflow:\s*hidden') {
  throw "[FAIL] overlay translation text still hides overflow"
}
Write-Host "[PASS] overlay translation text does not hide overflow"

if ($overlay -notmatch 'overflow-y:\s*auto' -or $overlay -notmatch 'scrollbar-width:\s*thin' -or $overlay -notmatch '::-webkit-scrollbar-thumb') {
  throw "[FAIL] overlay does not use styled internal scrolling"
}
Write-Host "[PASS] overlay uses styled internal scrolling"

if ($overlay -notmatch 'id="sourceBlock"' -or $overlay -notmatch 'id="translationBlock"') {
  throw "[FAIL] overlay source/translation blocks missing"
}
Write-Host "[PASS] overlay source/translation blocks present"

if ($overlay -notmatch 'id="card"' -or $overlay -notmatch '#card\s*\{[^}]*display:\s*flex') {
  throw "[FAIL] overlay does not use a single card layout for content and resize handle"
}
Write-Host "[PASS] overlay uses a single card layout"

if ($overlay -notmatch '#blocks\s*\{[^}]*gap:\s*0') {
  throw "[FAIL] overlay blocks are not joined"
}
Write-Host "[PASS] overlay source/translation blocks are joined"

if ($overlay -match '#blocks\s*\{[^}]*min-height:\s*100vh') {
  throw "[FAIL] overlay blocks still stretch to full window height"
}
Write-Host "[PASS] overlay blocks do not stretch to full height"

if ($overlay -notmatch 'display:\s*flex' -or $overlay -notmatch 'flex-direction:\s*column' -or $overlay -notmatch 'justify-content:\s*flex-start' -or $overlay -notmatch 'align-self:\s*start') {
  throw "[FAIL] overlay blocks are not pinned to top-left content flow"
}
Write-Host "[PASS] overlay blocks are pinned to top-left content flow"

if ($overlay -notmatch 'white-space:\s*normal' -or $overlay -notmatch 'white-space:\s*pre-wrap') {
  throw "[FAIL] overlay whitespace handling can still create layout gaps"
}
Write-Host "[PASS] overlay whitespace is scoped to text sections"

if ($overlay -match 'rgba\(82,\s*102,\s*132' -or $overlay -match 'rgba\(22,\s*28,\s*38') {
  throw "[FAIL] overlay still uses gray/outer block background colors"
}
Write-Host "[PASS] overlay block colors are not gray outer backgrounds"

if ($overlay -notmatch 'payload\.source_background' -or $overlay -notmatch 'payload\.translation_background' -or $overlay -notmatch 'function hexToRgba') {
  throw "[FAIL] overlay does not apply configurable background colors"
}
Write-Host "[PASS] overlay applies configurable background colors"

if ($overlay -notmatch 'font-weight:\s*700' -or $overlay -notmatch 'color:\s*rgba\(255,\s*255,\s*255,\s*0\.96\)') {
  throw "[FAIL] overlay labels are still low contrast"
}
Write-Host "[PASS] overlay labels are high contrast"

if ($index -notmatch 'id="opacity"[^>]*min="0\.05"') {
  throw "[FAIL] opacity setting does not allow low transparency"
}
Write-Host "[PASS] opacity setting allows low transparency"

if ($overlay -notmatch 'function cleanDisplayText' -or $overlay -notmatch '\\n\{3,\}') {
  throw "[FAIL] overlay does not compact excessive OCR blank lines"
}
Write-Host "[PASS] overlay compacts excessive OCR blank lines"

if ($overlay -notmatch 'getBoundingClientRect\(\)' -or $overlay -notmatch 'contentHeight\s*>\s*maxHeight') {
  throw "[FAIL] overlay does not measure rendered content and scrolling state"
}
Write-Host "[PASS] overlay measures rendered content and scrolling state"

if ($overlay -notmatch 'addEventListener\("resize"' -or $overlay -notmatch 'setTimeout\(scheduleResizeToContent,\s*80\)' -or $overlay -notmatch 'window\.innerWidth' -or $main -notmatch '\.resizable\(true\)') {
  throw "[FAIL] overlay does not reflow when manually resized"
}
Write-Host "[PASS] overlay reflows when manually resized"

if ($overlay -notmatch 'id="resizeHandle"' -or $overlay -notmatch 'pointerdown' -or $overlay -notmatch 'ew-resize' -or $overlay -notmatch 'flex:\s*0 0 6px') {
  throw "[FAIL] overlay does not expose a custom width resize handle"
}
Write-Host "[PASS] overlay exposes custom width resize handle"

if ($main -match 'request\.width\.clamp\(160,\s*cfg\.overlay\.width') {
  throw "[FAIL] overlay resize still clamps manual width to configured width"
}
Write-Host "[PASS] overlay manual width is not clamped to configured width"

if ($main -notmatch 'let width = default_width\.clamp\(180,\s*900\)' -or $main -match 'content_width\.clamp') {
  throw "[FAIL] overlay initial width can still shrink below configured width"
}
Write-Host "[PASS] overlay initial width keeps configured width"

if ($main -notmatch 'ocr_translation_blocks' -or $main -notmatch 'flush_translation_paragraph' -or $main -match 'for line in lines') {
  throw "[FAIL] OCR translation still uses visual line-by-line translation"
}
Write-Host "[PASS] OCR translation uses semantic paragraph blocks"

if ($overlay -notmatch 'manualWidth' -or $overlay -notmatch 'blocks\.scrollHeight' -or $overlay -notmatch 'resize_overlay_width') {
  throw "[FAIL] overlay manual resize state or full content height measurement is missing"
}
Write-Host "[PASS] overlay preserves manual width and measures full content height"

if ($overlay -match 'pointermove[\s\S]{0,900}resize_overlay_to_content' -or $overlay -match 'resize_overlay_width[\s\S]{0,240}height:') {
  throw "[FAIL] overlay width drag still changes content height during pointer move"
}
Write-Host "[PASS] overlay width drag does not change content height"

if ($overlay -notmatch 'max-width:\s*100%' -or $overlay -notmatch 'box-sizing:\s*border-box') {
  throw "[FAIL] overlay text sections are not constrained to resized width"
}
Write-Host "[PASS] overlay text sections follow resized width"

$mustNotHave = @(
  'id="ocrEngine"',
  'id="ocrBtn"',
  'id="hotkeyRecordBtn"',
  'id="hotkeyMouseX1Btn"',
  'id="hotkeyMouseX2Btn"',
  'id="saveBtn"',
  'id="offsetX"',
  'id="offsetY"',
  'id="diagnoseOcrBtn"',
  'id="openCaptureBtn"',
  'id="overlayTestBtn"',
  'id="testText"',
  'id="testBtn"',
  'DN OCR Translator',
  'DN OCR',
  '测试翻译',
  '进行一次 OCR',
  '诊断上一张截图',
  '打开截图文件',
  '测试浮窗',
  '保存设置',
  '侧键 1',
  '侧键 2',
  '偏移 X',
  '偏移 Y'
)

foreach ($pattern in $mustNotHave) {
  if ($index -match [regex]::Escape($pattern) -or $ui -match [regex]::Escape($pattern)) {
    throw "[FAIL] removed UI still exists: $pattern"
  }
  Write-Host "[PASS] removed UI absent: $pattern"
}

Write-Host "UI state regression passed."
