$ErrorActionPreference = "Stop"
$ProjectRoot = "F:\AI\dn-ocr-translator"

$main = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\src\main.rs") -Raw
$nativeSelection = Get-Content (Join-Path $ProjectRoot "crates\app-windows\src\native_selection.rs") -Raw
$config = Get-Content (Join-Path $ProjectRoot "crates\app-core\src\config.rs") -Raw
$settings = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\frontend\src\main.tsx") -Raw
$overlay = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\frontend\src\overlay.tsx") -Raw
$styles = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\frontend\src\styles.css") -Raw
$tauriConfig = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\tauri.conf.json") -Raw
$manifest = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\app.exe.manifest") -Raw
$buildScript = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\build.rs") -Raw
$hotkey = Get-Content (Join-Path $ProjectRoot "crates\app-windows\src\hotkey.rs") -Raw

$checks = @(
  @{ Name = "frontend dist configured"; Source = $tauriConfig; Pattern = '"frontendDist":\s*"frontend/dist"' },
  @{ Name = "frontend build command configured"; Source = $tauriConfig; Pattern = '"beforeBuildCommand":\s*"npm run build"' },
  @{ Name = "overlay loads React page"; Source = $main; Pattern = 'WebviewUrl::App\("overlay\.html"\.into\(\)\)' },
  @{ Name = "shadcn Card used"; Source = $overlay; Pattern = '<Card[\s\S]*translation-card' },
  @{ Name = "shadcn ScrollArea used"; Source = $overlay; Pattern = '<ScrollArea>' },
  @{ Name = "shadcn Resizable panels used"; Source = $overlay; Pattern = 'ResizablePanelGroup[\s\S]*direction="vertical"' },
  @{ Name = "source plus translation setting"; Source = $settings; Pattern = 'show_source' },
  @{ Name = "overlay draggable setting"; Source = $settings; Pattern = 'draggable' },
  @{ Name = "source background color setting"; Source = $settings; Pattern = 'source_background' },
  @{ Name = "translation background color setting"; Source = $settings; Pattern = 'translation_background' },
  @{ Name = "max height setting"; Source = "$settings`n$config"; Pattern = 'max_height' },
  @{ Name = "settings refresh visible overlay"; Source = $main; Pattern = 'fn refresh_overlay_settings' },
  @{ Name = "save emits overlay update"; Source = $main; Pattern = 'refresh_overlay_settings\(&app,\s*&config\)' },
  @{ Name = "overlay content resize command remains initial-fit only"; Source = $main; Pattern = 'resize_overlay_to_content' },
  @{ Name = "overlay drag command"; Source = $main; Pattern = 'start_overlay_drag' },
  @{ Name = "overlay native corner resize command"; Source = $main; Pattern = 'start_overlay_resize_corner' },
  @{ Name = "selection uses native win32 selector"; Source = $main; Pattern = 'select_rect_native\(selection_screen\.rect,\s*&selection_screen\.png\)' },
  @{ Name = "selection freezes screen before dragging"; Source = $main; Pattern = 'capture_frozen_screen\(\)' },
  @{ Name = "OCR crops frozen screen after selection"; Source = $main; Pattern = 'crop_frozen_screen_png' },
  @{ Name = "native selector paints frozen screenshot"; Source = $nativeSelection; Pattern = 'SetDIBitsToDevice' },
  @{ Name = "native selector handles left drag"; Source = $nativeSelection; Pattern = 'WM_LBUTTONDOWN[\s\S]*WM_MOUSEMOVE[\s\S]*WM_LBUTTONUP' },
  @{ Name = "native selector right click cancels"; Source = $nativeSelection; Pattern = 'WM_RBUTTONUP' },
  @{ Name = "native selector cleanup exported"; Source = "$main`n$nativeSelection"; Pattern = 'close_native_selection_windows' },
  @{ Name = "selection cleanup before pipeline"; Source = $main; Pattern = 'async fn run_pipeline[\s\S]{0,180}cleanup_selection_layers\(&app\)' },
  @{ Name = "selection cleanup before overlay"; Source = $main; Pattern = 'fn show_overlay[\s\S]{0,220}cleanup_selection_layers\(app\)' },
  @{ Name = "selection cleanup on cancel"; Source = $main; Pattern = 'fn finish_selection_cancel[\s\S]{0,220}cleanup_selection_layers\(app\)' },
  @{ Name = "selection state cleared"; Source = $main; Pattern = 'selection_active\.store\(false[\s\S]*selection_cancel\.store\(false' },
  @{ Name = "hotkey capture input"; Source = $settings; Pattern = '请按键或鼠标侧键' },
  @{ Name = "OCR translation uses semantic paragraph blocks"; Source = $main; Pattern = 'ocr_translation_blocks[\s\S]*flush_translation_paragraph' },
  @{ Name = "Windows executable starts normally"; Source = "$manifest`n$buildScript"; Pattern = 'requestedExecutionLevel level="asInvoker"[\s\S]*embed_resource::compile\("app\.manifest\.rc"' },
  @{ Name = "admin mode is config driven"; Source = "$main`n$config`n$settings"; Pattern = 'auto_elevate[\s\S]*ShellExecuteW[\s\S]*启动时自动以管理员权限运行' },
  @{ Name = "main close prompt wired"; Source = "$main`n$settings"; Pattern = 'main-close-requested[\s\S]*handle_close_choice' },
  @{ Name = "tray open and exit wired"; Source = $main; Pattern = 'TrayIconBuilder[\s\S]*tray_open[\s\S]*tray_exit' },
  @{ Name = "exit cleanup closes overlay and selection"; Source = $main; Pattern = 'fn exit_application[\s\S]*cleanup_runtime_windows[\s\S]*app\.exit\(0\)' },
  @{ Name = "Windows executable requests PerMonitorV2"; Source = $manifest; Pattern = '<ws2016:dpiAwareness>PerMonitorV2</ws2016:dpiAwareness>' }
)

foreach ($check in $checks) {
  if ($check.Source -notmatch $check.Pattern) {
    throw "[FAIL] $($check.Name)"
  }
  Write-Host "[PASS] $($check.Name)"
}

$removedFiles = @(
  "crates\app-tauri\ui\overlay.html",
  "crates\app-tauri\ui\index.html",
  "crates\app-tauri\ui\main.js",
  "crates\app-tauri\ui\styles.css",
  "crates\app-tauri\ui\selection.html",
  "crates\app-tauri\ui\selection-box.html",
  "crates\app-tauri\ui\selection-dim.html"
)

foreach ($relative in $removedFiles) {
  if (Test-Path (Join-Path $ProjectRoot $relative)) {
    throw "[FAIL] old UI file still exists: $relative"
  }
  Write-Host "[PASS] old UI file absent: $relative"
}

$mustNotHave = @(
  'id="resizeHandle"',
  'id="cornerResizeHandle"',
  'pointermove',
  'start_overlay_resize_width',
  'resize_overlay_width',
  'resize_overlay_manual',
  'manualWidth',
  'manualHeight',
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

$newFrontend = "$settings`n$overlay`n$styles"
foreach ($pattern in $mustNotHave) {
  if ($newFrontend -match [regex]::Escape($pattern) -or $main -match [regex]::Escape($pattern)) {
    throw "[FAIL] removed implementation still exists: $pattern"
  }
  Write-Host "[PASS] removed implementation absent: $pattern"
}

if ($styles -match 'rgba\(82,\s*102,\s*132' -or $styles -match 'rgba\(22,\s*28,\s*38') {
  throw "[FAIL] overlay still uses gray outer backgrounds"
}
Write-Host "[PASS] overlay does not use gray outer backgrounds"

if ($overlay -match 'HoverCard' -or $overlay -match 'react-rnd' -or $overlay -match 'PrimeReact') {
  throw "[FAIL] overlay uses a component outside the chosen shadcn stack"
}
Write-Host "[PASS] overlay uses the chosen shadcn stack only"

Write-Host "UI state regression passed."
