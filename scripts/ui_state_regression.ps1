$ErrorActionPreference = "Stop"
$ProjectRoot = "F:\AI\dn-ocr-translator"

$main = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\src\main.rs") -Raw
$nativeSelection = Get-Content (Join-Path $ProjectRoot "crates\app-windows\src\native_selection.rs") -Raw
$config = Get-Content (Join-Path $ProjectRoot "crates\app-core\src\config.rs") -Raw
$settings = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\frontend\src\main.tsx") -Raw
$overlay = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\frontend\src\overlay.tsx") -Raw
$statusOverlay = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\frontend\src\status.tsx") -Raw
$styles = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\frontend\src\styles.css") -Raw
$tauriConfig = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\tauri.conf.json") -Raw
$manifest = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\app.exe.manifest") -Raw
$buildScript = Get-Content (Join-Path $ProjectRoot "crates\app-tauri\build.rs") -Raw
$hotkey = Get-Content (Join-Path $ProjectRoot "crates\app-windows\src\hotkey.rs") -Raw

$checks = @(
  @{ Name = "frontend dist configured"; Source = $tauriConfig; Pattern = '"frontendDist":\s*"frontend/dist"' },
  @{ Name = "frontend build command configured"; Source = $tauriConfig; Pattern = '"beforeBuildCommand":\s*"npm run build"' },
  @{ Name = "overlay loads React page"; Source = $main; Pattern = 'WebviewUrl::App\("overlay\.html"\.into\(\)\)' },
  @{ Name = "status overlay loads React page"; Source = "$main`n$statusOverlay"; Pattern = 'WebviewUrl::App\("status\.html"\.into\(\)\)[\s\S]*status-overlay-update' },
  @{ Name = "shadcn Card used"; Source = $overlay; Pattern = '<Card[\s\S]*translation-card' },
  @{ Name = "shadcn ScrollArea used"; Source = $overlay; Pattern = '<ScrollArea>' },
  @{ Name = "shadcn Resizable panels used"; Source = $overlay; Pattern = 'ResizablePanelGroup[\s\S]*direction="vertical"' },
  @{ Name = "source plus translation setting"; Source = $settings; Pattern = 'show_source' },
  @{ Name = "result display mode setting"; Source = "$settings`n$config"; Pattern = 'result_mode[\s\S]*image_replace' },
  @{ Name = "image replace overlay branch"; Source = "$overlay`n$styles"; Pattern = 'imageReplaceMode[\s\S]*image_blocks[\s\S]*image-replace-source[\s\S]*image-replace-block-layer[\s\S]*image-replace-block' },
  @{ Name = "image replace resize preserves selected size"; Source = "$overlay`n$main"; Pattern = 'mode:\s*"image_replace"[\s\S]*request\.mode != "image_replace"' },
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
  @{ Name = "pipeline shows transient status overlay"; Source = $main; Pattern = 'show_status_overlay[\s\S]*正在截图[\s\S]*正在识别文字[\s\S]*正在翻译[\s\S]*hide_status_overlay' },
  @{ Name = "selection cleanup before overlay"; Source = $main; Pattern = 'fn show_overlay[\s\S]{0,420}cleanup_selection_layers\(app\)' },
  @{ Name = "selection cleanup on cancel"; Source = $main; Pattern = 'fn finish_selection_cancel[\s\S]{0,220}cleanup_selection_layers\(app\)' },
  @{ Name = "selection state cleared"; Source = $main; Pattern = 'selection_active\.store\(false[\s\S]*selection_cancel\.store\(false' },
  @{ Name = "hotkey capture input"; Source = $settings; Pattern = '请按键或鼠标侧键' },
  @{ Name = "OCR translation uses semantic paragraph blocks"; Source = $main; Pattern = 'ocr_translation_blocks[\s\S]*flush_translation_paragraph' },
  @{ Name = "Windows executable starts normally"; Source = "$manifest`n$buildScript"; Pattern = 'requestedExecutionLevel level="asInvoker"[\s\S]*embed_resource::compile\("app\.manifest\.rc"' },
  @{ Name = "single instance prevents duplicate tray processes"; Source = $main; Pattern = 'tauri_plugin_single_instance::init[\s\S]*show_main_window' },
  @{ Name = "startup autostart setting wired"; Source = "$main`n$config`n$settings"; Pattern = 'tauri_plugin_autostart[\s\S]*--from-autostart[\s\S]*sync_autostart_setting[\s\S]*launch_at_startup[\s\S]*开机时自动启动到托盘' },
  @{ Name = "admin mode is config driven"; Source = "$main`n$config`n$settings"; Pattern = 'auto_elevate[\s\S]*ShellExecuteW[\s\S]*启动时自动以管理员权限运行' },
  @{ Name = "developer translation image logs wired"; Source = "$config`n$settings`n$main`n$overlay"; Pattern = 'translation_log_enabled[\s\S]*translation_log_retention_days[\s\S]*记录原图替换翻译日志[\s\S]*save_translation_log_render' },
  @{ Name = "translation logs are daily and retained"; Source = $main; Pattern = 'translation-logs[\s\S]*%Y-%m-%d[\s\S]*cleanup_translation_logs' },
  @{ Name = "translation diagnostic logs capture failures"; Source = $main; Pattern = 'create_translation_diagnostic_log_entry[\s\S]*"status": "failed"[\s\S]*"stage": stage[\s\S]*"reason": reason' },
  @{ Name = "OCR failure path writes diagnostic log"; Source = $main; Pattern = 'Err\(err\) => \{[\s\S]*ocr failed[\s\S]*create_translation_diagnostic_log_entry[\s\S]*"ocr"' },
  @{ Name = "empty OCR path writes diagnostic log"; Source = $main; Pattern = 'raw_text\.is_empty\(\)[\s\S]*create_translation_diagnostic_log_entry[\s\S]*"ocr_empty"' },
  @{ Name = "image replacement uses translation units"; Source = $main; Pattern = 'struct TranslationUnit[\s\S]*image_replacement_translation_units[\s\S]*chat_log_translation_units' },
  @{ Name = "chat log replacement preserves message units"; Source = $main; Pattern = 'image_replacement_chat_log_keeps_message_level_units[\s\S]*assert_eq!\(units\.len\(\),\s*7' },
  @{ Name = "chat log scene has performance budget"; Source = $main; Pattern = 'image_replacement_chat_log_keeps_message_level_units[\s\S]*Instant::now[\s\S]*Duration::from_millis\(20\)' },
  @{ Name = "split chat continuation stays in chat strategy"; Source = $main; Pattern = 'image_replacement_chat_log_absorbs_split_continuation_regions[\s\S]*volunteers to tank[\s\S]*assert_eq!\(units\.len\(\),\s*6' },
  @{ Name = "chat log ignores preamble ui line"; Source = $main; Pattern = 'image_replacement_chat_log_ignores_preamble_ui_line[\s\S]*READY[\s\S]*!unit\.source_text\.contains\("READY"\)' },
  @{ Name = "tooltip stat lines preserve hard breaks"; Source = $main; Pattern = 'image_replacement_tooltip_stat_lines_keep_hard_breaks[\s\S]*StructuredLine[\s\S]*Magic Dmg : 78400~78400[\s\S]*Adaptive Stat : 410' },
  @{ Name = "tooltip scene keeps soft wrapped paragraphs"; Source = $main; Pattern = 'image_replacement_tooltip_stat_lines_keep_hard_breaks[\s\S]*Can be equipped on Lv\. 70\+ equipment\.[\s\S]*After studying slain dragons' },
  @{ Name = "tooltip scene has performance budget"; Source = $main; Pattern = 'image_replacement_tooltip_stat_lines_keep_hard_breaks[\s\S]*Instant::now[\s\S]*Duration::from_millis\(20\)' },
  @{ Name = "settings form keeps control rows"; Source = $main; Pattern = 'image_replacement_settings_form_keeps_control_rows[\s\S]*FormLine[\s\S]*Remove camera shake[\s\S]*Mute other players sounds in field' },
  @{ Name = "settings form rows expand to avoid wrapping"; Source = $main; Pattern = 'image_replacement_settings_form_keeps_control_rows[\s\S]*Show details in Discord presence[\s\S]*rect\.width >= 240\.0' },
  @{ Name = "settings form scene has performance budget"; Source = $main; Pattern = 'image_replacement_settings_form_keeps_control_rows[\s\S]*Instant::now[\s\S]*Duration::from_millis\(20\)' },
  @{ Name = "game UI glossary rewrites artifact terms"; Source = $main; Pattern = 'image_replacement_game_ui_glossary_rewrites_artifact_terms[\s\S]*法力超载[\s\S]*暴击率[\s\S]*全元素 ATK[\s\S]*每个栏位可重掷' },
  @{ Name = "game UI glossary scene has performance budget"; Source = $main; Pattern = 'image_replacement_game_ui_glossary_rewrites_artifact_terms[\s\S]*Instant::now[\s\S]*Duration::from_millis\(20\)' },
  @{ Name = "image replacement supports single-line block wrapping"; Source = "$main`n$overlay`n$styles"; Pattern = 'wrap_mode[\s\S]*data-wrap=\{block\.wrap_mode \|\| "wrap"\}[\s\S]*image-replace-block\[data-wrap="single"\]' },
  @{ Name = "image replacement has visual regression scene test"; Source = $main; Pattern = 'image_replacement_visual_regression_outputs_game_ui_svg[\s\S]*game_ui_line_preserve' },
  @{ Name = "tooltip image replacement has visual regression scene test"; Source = $main; Pattern = 'image_replacement_visual_regression_outputs_tooltip_svg[\s\S]*tooltip_structured_lines' },
  @{ Name = "settings form image replacement has visual regression scene test"; Source = $main; Pattern = 'image_replacement_visual_regression_outputs_settings_form_svg[\s\S]*settings_form_lines' },
  @{ Name = "image replacement writes visual regression output"; Source = $main; Pattern = 'write_visual_regression_svg[\s\S]*target[\s\S]*visual-regression' },
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
  'image-replace-translation',
  'image-replace-text',
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
