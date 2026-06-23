$ErrorActionPreference = "Stop"
$ProjectRoot = "F:\AI\dn-ocr-translator"

$main = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\src\main.rs") -Raw
$selection = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\selection.html") -Raw
$selectionBox = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\selection-box.html") -Raw
$selectionDim = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\selection-dim.html") -Raw
$index = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\index.html") -Raw
$ui = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\main.js") -Raw
$overlay = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\ui\overlay.html") -Raw
$tauriConfig = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\tauri.conf.json") -Raw
$manifest = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\app.exe.manifest") -Raw
$buildScript = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\build.rs") -Raw
$hotkey = Get-Content (Join-Path $ProjectRoot "crates\app-windows\src\hotkey.rs") -Raw

$checks = @(
  @{ Name = "start hides old overlay"; Pattern = 'get_webview_window\("overlay"\)' },
  @{ Name = "start clears overlay payload"; Pattern = 'clear_overlay_payload\(app\)' },
  @{ Name = "preload selection box"; Pattern = 'get_or_create_selection_box\(app\.handle\(\)\)' },
  @{ Name = "mouse selection mode"; Pattern = 'start_mouse_selection\(app\.clone\(\)\)' },
  @{ Name = "selection uses left mouse polling"; Pattern = 'left_mouse_down\(\)' },
  @{ Name = "selection uses small box window"; Pattern = 'selection-box\.html' },
  @{ Name = "selection uses fullscreen dim window"; Pattern = 'selection-dim\.html' },
  @{ Name = "selection shows entry hint"; Pattern = 'show_selection_hint' },
  @{ Name = "selection freezes screen before dragging"; Pattern = 'capture_frozen_screen\(\)' },
  @{ Name = "selection hides main window before capture"; Pattern = 'hide_main_window_for_selection\(&app\)' },
  @{ Name = "OCR crops frozen screen after selection"; Pattern = 'crop_frozen_screen_png' },
  @{ Name = "selection listens reset"; Pattern = 'listen\("selection-reset", resetSelection\)' },
  @{ Name = "selection blur reset"; Pattern = 'addEventListener\("blur", resetSelection\)' },
  @{ Name = "selection hint mentions right click cancel"; Pattern = '右键或 Esc 取消' },
  @{ Name = "selection right click cancels"; Pattern = 'event\.button === 2' },
  @{ Name = "global right click cancels selection"; Pattern = 'selection_cancel\.store\(true,\s*Ordering::SeqCst\)' },
  @{ Name = "selection blocks context menu"; Pattern = 'addEventListener\("contextmenu", cancelSelection\)' },
  @{ Name = "small left click auto detects region"; Pattern = 'selection_auto_detect' }
)

foreach ($check in $checks) {
  $source = "$main`n$selection`n$selectionBox`n$selectionDim`n$hotkey"
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
  @{ Name = "overlay manual size resize command"; Pattern = 'resize_overlay_manual' },
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

if ($selection -match 'background:\s*rgba\(0,\s*0,\s*0' -or $selection -match '9999px') {
  throw "[FAIL] selection overlay still dims the whole screen and can flash in fullscreen games"
}
Write-Host "[PASS] selection overlay does not dim the whole screen"

if ($main -match 'create_selection_window' -or $main -match 'WebviewUrl::App\("selection\.html' -or $main -match 'WebviewWindowBuilder::new\([^)]*"selection"') {
  throw "[FAIL] OCR selection still creates or shows a fullscreen webview"
}
Write-Host "[PASS] OCR selection does not use a fullscreen webview"

if ($selectionBox -notmatch 'pointer-events:\s*none' -or $selectionBox -notmatch 'border:\s*2px solid') {
  throw "[FAIL] selection box is not a passive small rectangle overlay"
}
Write-Host "[PASS] selection box is a passive small rectangle overlay"

if ($selectionDim -notmatch 'background:\s*rgba\(0,\s*0,\s*0,\s*0\.22\)' -or $selectionDim -notmatch 'pointer-events:\s*auto') {
  throw "[FAIL] OCR selection dim layer is missing or does not receive mouse input"
}
Write-Host "[PASS] OCR selection uses a mouse-blocking dim layer"

if ($selectionDim -notmatch 'contextmenu' -or $selectionDim -notmatch 'mousedown' -or $selectionDim -notmatch 'preventDefault\(\)' -or $main -notmatch 'set_ignore_cursor_events\(false\)') {
  throw "[FAIL] OCR dim layer does not block background clicks like a screenshot surface"
}
Write-Host "[PASS] OCR dim layer blocks background clicks like a screenshot surface"

if ($selectionDim -notmatch 'id="frozenScreen"' -or $selectionDim -notmatch 'selection-dim-frame' -or $main -notmatch 'selection_dim_frame_data_url' -or $main -notmatch 'image_data_url') {
  throw "[FAIL] OCR dim layer does not display the frozen screenshot"
}
Write-Host "[PASS] OCR dim layer displays the frozen screenshot"

if ($main -notmatch 'fn selection_dim_frame_data_url' -or $main -notmatch 'source_x = \(x - offset_x\)\.clamp' -or $main -notmatch 'source_y = \(y - offset_y\)\.clamp' -or $selectionDim -notmatch 'width:\s*100%' -or $selectionDim -notmatch 'height:\s*100%') {
  throw "[FAIL] OCR dim layer does not use a precomposited frozen screenshot frame"
}
Write-Host "[PASS] OCR dim layer uses a precomposited frozen screenshot frame"

if ($main -notmatch 'fn hide_main_window_for_selection' -or $main -notmatch 'get_webview_window\("main"\)' -or $main -notmatch 'tokio::time::sleep\(Duration::from_millis\(90\)\)' -or $main -notmatch 'fn restore_main_window_after_selection') {
  throw "[FAIL] OCR selection does not hide and restore the main window around capture"
}
Write-Host "[PASS] OCR selection hides and restores the main window around capture"

if ($main -notmatch 'let overscan = 24' -or $main -notmatch 'rect\.x - overscan' -or $main -notmatch 'rect\.width \+ overscan \* 2') {
  throw "[FAIL] OCR dim layer does not overscan the virtual screen edges"
}
Write-Host "[PASS] OCR dim layer overscans virtual screen edges"

if ($selectionBox -notmatch '拖动选择文字' -or $selectionBox -notmatch 'selection-box-mode' -or $selectionBox -notmatch 'body\.hint') {
  throw "[FAIL] OCR selection does not show a lightweight entry hint"
}
Write-Host "[PASS] OCR selection shows a lightweight entry hint"

if ($selectionBox -notmatch 'pointer-events:\s*none' -or $main -notmatch 'set_ignore_cursor_events\(true\)' -or $main -notmatch 'show_selection_dim\(&app,\s*frozen_screen\.as_ref\(\)\)') {
  throw "[FAIL] selection box visual layer can still capture mouse input"
}
Write-Host "[PASS] selection box visual layer ignores mouse input"

if ($hotkey -notmatch 'WM_RBUTTONDOWN' -or $hotkey -notmatch 'MouseButton::Right' -or $main -notmatch 'selection_active\.load\(Ordering::SeqCst\)') {
  throw "[FAIL] right click is not handled by the global selection cancel path"
}
Write-Host "[PASS] right click is handled by the global selection cancel path"

if ($main -notmatch 'capture_frozen_screen\(\)[\s\S]{0,420}show_selection_dim\(&app,\s*frozen_screen\.as_ref\(\)\)') {
  throw "[FAIL] OCR dim layer can be captured before the frozen screenshot"
}
Write-Host "[PASS] OCR freezes the screen before showing the dim layer"

if ($main -match 'OCR 失败：\{err\}' -or $main -match 'format!\("OCR 失败' -or $main -match 'show_overlay\(&app,\s*&cfg,\s*payload\.anchor,\s*String::new\(\),\s*message\)') {
  throw "[FAIL] OCR failure messages still expose internal engine/error details"
}
Write-Host "[PASS] OCR failure messages are user friendly"

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

if ($overlay -notmatch 'addEventListener\("resize"' -or $overlay -notmatch 'setTimeout\(scheduleResizeToContent,\s*80\)' -or $main -notmatch '\.resizable\(false\)') {
  throw "[FAIL] overlay does not reflow when manually resized"
}
Write-Host "[PASS] overlay reflows when manually resized"

if ($overlay -notmatch 'id="resizeHandle"' -or $overlay -notmatch 'pointerdown' -or $overlay -notmatch 'ew-resize' -or $overlay -notmatch 'flex:\s*0 0 16px') {
  throw "[FAIL] overlay does not expose a custom width resize handle"
}
Write-Host "[PASS] overlay exposes custom width resize handle"

if ($overlay -notmatch 'id="cornerResizeHandle"' -or $overlay -notmatch 'nwse-resize' -or $overlay -notmatch 'resize_overlay_manual') {
  throw "[FAIL] overlay does not expose a custom width/height resize handle"
}
Write-Host "[PASS] overlay exposes custom width/height resize handle"

if ($overlay -notmatch 'margin:\s*10px 7px' -or $overlay -notmatch 'width:\s*2px') {
  throw "[FAIL] overlay resize handle hit target is not wider than its visual grip"
}
Write-Host "[PASS] overlay resize handle has a wide hit target"

if ($main -match 'request\.width\.clamp\(160,\s*cfg\.overlay\.width') {
  throw "[FAIL] overlay resize still clamps manual width to configured width"
}
Write-Host "[PASS] overlay manual width is not clamped to configured width"

if ($main -notmatch 'let width = default_width\.clamp\(180,\s*900\)' -or $main -match 'content_width\.clamp') {
  throw "[FAIL] overlay initial width can still shrink below configured width"
}
Write-Host "[PASS] overlay initial width keeps configured width"

if ($main -notmatch 'width,\s*[\r\n]+\s*opacity:' -or $overlay -notmatch 'payload\.width' -or $overlay -match 'Math\.max\(blocks\.scrollHeight,\s*contentHeight,\s*cardRect\.height\)' -or $overlay -match 'window\.innerWidth') {
  throw "[FAIL] overlay still derives initial size from stale window dimensions"
}
Write-Host "[PASS] overlay initial size comes from payload and content"

if ($main -notmatch '\.focusable\(true\)' -or $main -notmatch 'set_focusable\(true\)') {
  throw "[FAIL] overlay window cannot reliably receive mouse events for drag/resize"
}
Write-Host "[PASS] overlay window can receive mouse events for drag/resize"

if ($main -notmatch 'ocr_translation_blocks' -or $main -notmatch 'flush_translation_paragraph' -or $main -match 'for line in lines') {
  throw "[FAIL] OCR translation still uses visual line-by-line translation"
}
Write-Host "[PASS] OCR translation uses semantic paragraph blocks"

if ($overlay -notmatch 'manualWidth' -or $overlay -notmatch 'manualHeight' -or $overlay -notmatch 'blocks\.scrollHeight' -or $overlay -notmatch 'resize_overlay_width') {
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

if ($manifest -notmatch 'requestedExecutionLevel level="requireAdministrator"' -or $buildScript -notmatch 'embed_resource::compile\("app\.manifest\.rc"') {
  throw "[FAIL] Windows executable does not request administrator privileges"
}
Write-Host "[PASS] Windows executable requests administrator privileges"

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
