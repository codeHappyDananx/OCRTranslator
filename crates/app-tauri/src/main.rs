#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use app_core::{
    provider_catalog, translate, AppConfig, ProviderInfo, TranslationRequest, TranslationResponse,
};
use app_windows::{
    available_windows_ocr_languages, capture_rect_png, close_native_selection_windows,
    cursor_position, detect_ocr_engines, install_snippingtool_oneocr_runtime,
    preview_snippingtool_oneocr_package, recognize_png_pipeline, release_cursor_lock,
    select_rect_native, start_native_window_resize, virtual_screen_rect, GlobalInputEvent,
    KeyboardEvent, MouseButton, NativeResizeDirection, OcrEngineStatus, OcrLanguageInfo,
    OcrPipelineRequest, OcrPipelineResult, OcrTextLine, OneOcrPackageInfo, Point, Rect,
};
use base64::{engine::general_purpose, Engine as _};
use chrono::{Local, NaiveDate};
use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    io::Cursor,
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton as TrayMouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as _;
use tokio::sync::mpsc;
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::HWND,
        UI::{
            Shell::{IsUserAnAdmin, ShellExecuteW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

struct AppState {
    config: Mutex<AppConfig>,
    last_overlay: Mutex<Option<OverlayPayload>>,
    selection_active: AtomicBool,
    selection_cancel: AtomicBool,
    exiting: AtomicBool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CloseChoice {
    Tray,
    Exit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloseChoiceRequest {
    choice: CloseChoice,
    dont_ask_again: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManualTranslateRequest {
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SelectionPayload {
    rect: Rect,
    anchor: Point,
}

#[derive(Clone)]
struct FrozenScreen {
    rect: Rect,
    png: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OverlayPayload {
    result_mode: String,
    text: String,
    raw_text: String,
    width: u32,
    image_width: u32,
    image_height: u32,
    source_image_data_url: Option<String>,
    image_blocks: Vec<ImageReplacementBlock>,
    opacity: f32,
    font_size: u32,
    max_height: u32,
    source_background: String,
    translation_background: String,
    double_click_close: bool,
    show_source: bool,
    draggable: bool,
    log_entry_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatusOverlayPayload {
    text: String,
    done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImageReplacementBlock {
    source_text: String,
    translated_text: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    font_size: u32,
    background: String,
    color: String,
    align: String,
    wrap_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OverlayResizeRequest {
    width: u32,
    height: u32,
    #[serde(default)]
    mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SaveTranslationLogRenderRequest {
    entry_id: String,
    translated_image_data_url: String,
}

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    state
        .config
        .lock()
        .map(|cfg| cfg.clone())
        .map_err(|e| format!("读取配置锁失败：{e}"))
}

#[tauri::command]
fn save_config(
    mut config: AppConfig,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    config.normalize();
    config.save().map_err(|e| e.to_string())?;
    let mut guard = state
        .config
        .lock()
        .map_err(|e| format!("写入配置锁失败：{e}"))?;
    *guard = config.clone();
    sync_autostart_setting(&app, &config).map_err(|e| e.to_string())?;
    refresh_overlay_settings(&app, &config);
    Ok(())
}

#[tauri::command]
fn handle_close_choice(
    request: CloseChoiceRequest,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    match request.choice {
        CloseChoice::Tray => {
            if request.dont_ask_again {
                let mut config = state
                    .config
                    .lock()
                    .map_err(|e| format!("写入配置锁失败：{e}"))?;
                config.app.close_to_tray = true;
                config.app.ask_before_close = false;
                config.save().map_err(|e| e.to_string())?;
            }
            hide_main_window(&app)?;
        }
        CloseChoice::Exit => {
            exit_application(&app, &state);
        }
    }
    Ok(())
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    exit_application(&app, &state);
    Ok(())
}

#[tauri::command]
fn get_admin_status() -> bool {
    is_running_as_admin()
}

#[tauri::command]
fn list_providers() -> Vec<ProviderInfo> {
    provider_catalog()
}

#[tauri::command]
fn list_ocr_languages() -> Result<Vec<OcrLanguageInfo>, String> {
    available_windows_ocr_languages().map_err(|e| e.to_string())
}

#[tauri::command]
fn list_ocr_engines() -> Result<Vec<OcrEngineStatus>, String> {
    detect_ocr_engines().map_err(|e| e.to_string())
}

#[tauri::command]
async fn preview_oneocr_runtime() -> Result<Option<OneOcrPackageInfo>, String> {
    preview_snippingtool_oneocr_package()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn install_oneocr_runtime() -> Result<String, String> {
    install_snippingtool_oneocr_runtime()
        .await
        .map(|path| path.display().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn manual_translate(
    request: ManualTranslateRequest,
    state: State<'_, AppState>,
) -> Result<TranslationResponse, String> {
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("读取配置锁失败：{e}"))?
        .clone();
    let settings = cfg
        .provider_settings
        .get(&cfg.translator)
        .cloned()
        .unwrap_or_default();
    translate(TranslationRequest {
        provider_id: cfg.translator,
        text: request.text,
        source_lang: cfg.source_lang,
        target_lang: cfg.target_lang,
        settings,
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn run_ocr_once(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("读取配置锁失败：{e}"))?
        .clone();
    start_selection_window(&app, &cfg).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn selection_done(
    payload: SelectionPayload,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("selection") {
        let _ = window.hide();
    }
    tokio::time::sleep(Duration::from_millis(60)).await;
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("读取配置锁失败：{e}"))?
        .clone();
    run_pipeline(app, cfg, payload, None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn selection_auto_detect(
    anchor: Point,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("selection") {
        let _ = window.hide();
    }
    tokio::time::sleep(Duration::from_millis(80)).await;
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("读取配置锁失败：{e}"))?
        .clone();
    let Some(rect) = auto_detect_selection_rect(&app, anchor).map_err(|e| e.to_string())? else {
        let _ = app.emit("ocr-status", "未识别到可自动扩选的区域");
        return Ok(());
    };
    run_pipeline(app, cfg, SelectionPayload { rect, anchor }, None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn selection_cancel(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("selection") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn close_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.hide().map_err(|e| e.to_string())?;
    }
    clear_overlay_payload(&app);
    Ok(())
}

#[tauri::command]
fn start_overlay_drag(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.start_dragging().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn start_overlay_resize_corner(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        start_native_window_resize(hwnd.0 as isize, NativeResizeDirection::SouthEast)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn resize_overlay_to_content(
    request: OverlayResizeRequest,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("读取配置锁失败：{e}"))?
        .clone();
    let Some(window) = app.get_webview_window("overlay") else {
        return Ok(());
    };
    let current = window.outer_position().map_err(|e| e.to_string())?;
    let mut width = request.width.max(160);
    let mut height = request.height.max(54);
    let mut x = current.x;
    let mut y = current.y;

    if request.mode != "image_replace" {
        clamp_overlay_to_primary_monitor(&app, &cfg, &mut x, &mut y, &mut width, &mut height)
            .map_err(|e| e.to_string())?;
    }

    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn clamp_overlay_to_primary_monitor(
    app: &tauri::AppHandle,
    cfg: &AppConfig,
    x: &mut i32,
    y: &mut i32,
    width: &mut u32,
    height: &mut u32,
) -> anyhow::Result<()> {
    if let Some(monitor) = app.primary_monitor()? {
        let pos = monitor.position();
        let size = monitor.size();
        let margin = cfg.overlay.screen_margin;
        let left = pos.x + margin;
        let top = pos.y + margin;
        let right = pos.x + size.width as i32 - margin;
        let bottom = pos.y + size.height as i32 - margin;
        let max_width = (right - left).max(160) as u32;
        let max_height = (bottom - top).max(54) as u32;
        *width = (*width).min(max_width);
        *height = (*height).min(max_height);
        if *x + *width as i32 > right {
            *x = (right - *width as i32).max(left);
        }
        if *y + *height as i32 > bottom {
            *y = (bottom - *height as i32).max(top);
        }
        *x = (*x).max(left);
        *y = (*y).max(top);
    }
    Ok(())
}

#[tauri::command]
fn get_overlay_payload(state: State<'_, AppState>) -> Result<Option<OverlayPayload>, String> {
    state
        .last_overlay
        .lock()
        .map(|payload| payload.clone())
        .map_err(|e| format!("读取浮窗结果失败：{e}"))
}

#[tauri::command]
fn save_translation_log_render(request: SaveTranslationLogRenderRequest) -> Result<(), String> {
    save_translation_log_render_inner(request).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_cursor_position() -> Result<Point, String> {
    cursor_position().map_err(|e| e.to_string())
}

fn start_selection_window(app: &tauri::AppHandle, _cfg: &AppConfig) -> anyhow::Result<()> {
    let _ = release_cursor_lock();
    cleanup_selection_layers(app);
    hide_status_overlay(app);
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
    clear_overlay_payload(app);
    app.emit("ocr-status", "拖动选择要翻译的文字，右键取消")?;
    start_mouse_selection(app.clone());
    Ok(())
}

fn start_mouse_selection(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Some(state) = app.try_state::<AppState>() {
            state.selection_active.store(true, Ordering::SeqCst);
            state.selection_cancel.store(false, Ordering::SeqCst);
        }
        let restore_main_window = hide_main_window_for_selection(&app);
        if restore_main_window {
            tokio::time::sleep(Duration::from_millis(90)).await;
        }
        let frozen_screen = match capture_frozen_screen() {
            Ok(screen) => screen,
            Err(err) => {
                eprintln!("freeze screen failed: {err}");
                restore_main_window_after_selection(&app, restore_main_window);
                finish_selection_state(&app);
                let _ = app.emit("ocr-status", "没有截到当前画面，请再试一次。");
                return;
            }
        };
        let selection_screen = frozen_screen.clone();
        let selection = tokio::task::spawn_blocking(move || {
            select_rect_native(selection_screen.rect, &selection_screen.png)
        })
        .await;
        let rect = match selection {
            Ok(Ok(Some(rect))) => rect,
            Ok(Ok(None)) => {
                finish_selection_cancel(&app, restore_main_window);
                return;
            }
            Ok(Err(err)) => {
                restore_main_window_after_selection(&app, restore_main_window);
                finish_selection_state(&app);
                let _ = app.emit("ocr-status", format!("选区没有打开成功：{err}"));
                return;
            }
            Err(err) => {
                restore_main_window_after_selection(&app, restore_main_window);
                finish_selection_state(&app);
                let _ = app.emit("ocr-status", format!("选区没有打开成功：{err}"));
                return;
            }
        };
        cleanup_selection_layers(&app);
        finish_selection_state(&app);
        let anchor = Point {
            x: rect.x + rect.width,
            y: rect.y + rect.height,
        };
        let cfg = match app.state::<AppState>().config.lock().map(|cfg| cfg.clone()) {
            Ok(cfg) => cfg,
            Err(_) => {
                restore_main_window_after_selection(&app, restore_main_window);
                return;
            }
        };
        if rect.width < 16 || rect.height < 16 {
            match auto_detect_selection_rect(&app, anchor) {
                Ok(Some(rect)) => {
                    let _ = run_pipeline(
                        app.clone(),
                        cfg,
                        SelectionPayload { rect, anchor },
                        Some(frozen_screen),
                    )
                    .await;
                }
                Ok(None) => {
                    let _ = show_user_message(
                        &app,
                        &cfg,
                        anchor,
                        "没看到可识别的文字，换个区域再试试。",
                    );
                }
                Err(_) => {
                    let _ =
                        show_user_message(&app, &cfg, anchor, "这次没有选到文字，请重新试一次。");
                }
            }
            restore_main_window_after_selection(&app, restore_main_window);
            return;
        }
        let _ = run_pipeline(
            app.clone(),
            cfg,
            SelectionPayload { rect, anchor },
            Some(frozen_screen),
        )
        .await;
        restore_main_window_after_selection(&app, restore_main_window);
    });
}

fn finish_selection_state(app: &tauri::AppHandle) {
    cleanup_selection_layers(app);
    close_native_selection_windows();
    if let Some(state) = app.try_state::<AppState>() {
        state.selection_active.store(false, Ordering::SeqCst);
        state.selection_cancel.store(false, Ordering::SeqCst);
    }
}

fn finish_selection_cancel(app: &tauri::AppHandle, restore_main_window: bool) {
    cleanup_selection_layers(app);
    finish_selection_state(app);
    restore_main_window_after_selection(app, restore_main_window);
    let _ = app.emit("ocr-status", "已取消");
}

fn cleanup_selection_layers(app: &tauri::AppHandle) {
    close_native_selection_windows();
    for label in ["selection", "selection-box", "selection-dim"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
            let _ = window.close();
        }
    }
}

fn hide_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window.hide().map_err(|e| e.to_string())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn hide_main_window_for_selection(app: &tauri::AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    let Ok(true) = window.is_visible() else {
        return false;
    };
    let _ = window.hide();
    true
}

fn restore_main_window_after_selection(app: &tauri::AppHandle, restore: bool) {
    if !restore {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }
}

fn cleanup_runtime_windows(app: &tauri::AppHandle) {
    cleanup_selection_layers(app);
    hide_status_overlay(app);
    let _ = release_cursor_lock();
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
        let _ = window.close();
    }
    clear_overlay_payload(app);
}

fn exit_application(app: &tauri::AppHandle, state: &AppState) {
    state.exiting.store(true, Ordering::SeqCst);
    state.selection_active.store(false, Ordering::SeqCst);
    state.selection_cancel.store(true, Ordering::SeqCst);
    cleanup_runtime_windows(app);
    if let Some(hook) = app.try_state::<Mutex<Option<app_windows::GlobalInputHook>>>() {
        if let Ok(mut hook) = hook.lock() {
            *hook = None;
        }
    }
    app.exit(0);
}

fn maybe_handle_main_close(app: &tauri::AppHandle, api: &tauri::CloseRequestApi) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if state.exiting.load(Ordering::SeqCst) {
        return;
    }
    api.prevent_close();
    let cfg = state.config.lock().map(|cfg| cfg.clone());
    let Ok(cfg) = cfg else {
        let _ = app.emit("main-close-requested", ());
        return;
    };
    if !cfg.app.ask_before_close {
        if cfg.app.close_to_tray {
            let _ = hide_main_window(app);
        } else {
            exit_application(app, &state);
        }
    } else {
        let _ = app.emit("main-close-requested", ());
    }
}

fn capture_frozen_screen() -> anyhow::Result<FrozenScreen> {
    let rect = virtual_screen_rect();
    let png = capture_rect_png(rect)?;
    Ok(FrozenScreen { rect, png })
}

fn crop_frozen_screen_png(screen: &FrozenScreen, rect: Rect) -> anyhow::Result<Vec<u8>> {
    let rect = rect.normalized();
    let left = rect.x.max(screen.rect.x);
    let top = rect.y.max(screen.rect.y);
    let right = (rect.x + rect.width).min(screen.rect.x + screen.rect.width);
    let bottom = (rect.y + rect.height).min(screen.rect.y + screen.rect.height);
    let width = (right - left).max(0);
    let height = (bottom - top).max(0);
    if width <= 2 || height <= 2 {
        anyhow::bail!("选区太小，无法截图");
    }

    let image = image::load_from_memory(&screen.png)?.to_rgba8();
    let cropped = image::imageops::crop_imm(
        &image,
        (left - screen.rect.x) as u32,
        (top - screen.rect.y) as u32,
        width as u32,
        height as u32,
    )
    .to_image();
    let mut out = Cursor::new(Vec::new());
    cropped.write_to(&mut out, ImageFormat::Png)?;
    Ok(out.into_inner())
}

fn translation_log_root() -> anyhow::Result<PathBuf> {
    Ok(app_core::config_dir()?.join("translation-logs"))
}

fn create_translation_log_entry_dir(
    cfg: &AppConfig,
) -> anyhow::Result<Option<(String, PathBuf, chrono::DateTime<Local>)>> {
    if !cfg.developer.translation_log_enabled {
        return Ok(None);
    }

    cleanup_translation_logs(cfg.developer.translation_log_retention_days)?;

    let now = Local::now();
    let day = now.format("%Y-%m-%d").to_string();
    let entry = format!(
        "{}-{:03}",
        now.format("%H%M%S"),
        now.timestamp_subsec_millis()
    );
    let root = translation_log_root()?;
    let entry_dir = root.join(&day).join(&entry);
    fs::create_dir_all(&entry_dir)
        .map_err(|e| anyhow::anyhow!("创建翻译日志目录失败：{}，{e}", entry_dir.display()))?;

    Ok(Some((format!("{day}/{entry}"), entry_dir, now)))
}

fn create_translation_log_entry(
    cfg: &AppConfig,
    source_png: &[u8],
    ocr_result: &OcrPipelineResult,
    selected_rect: Rect,
    capture_rect: Rect,
    raw_text: &str,
    translated_text: &str,
    image_blocks: &[ImageReplacementBlock],
) -> anyhow::Result<Option<String>> {
    let Some((entry_id, entry_dir, now)) = create_translation_log_entry_dir(cfg)? else {
        return Ok(None);
    };

    fs::write(entry_dir.join("source.png"), source_png)
        .map_err(|e| anyhow::anyhow!("写入原图日志失败：{e}"))?;
    if let Some(preprocessed) = &ocr_result.preprocessed_png {
        fs::write(entry_dir.join("ocr_preprocessed.png"), preprocessed)
            .map_err(|e| anyhow::anyhow!("写入 OCR 预处理图失败：{e}"))?;
    }
    let metadata = serde_json::json!({
        "created_at": now.to_rfc3339(),
        "status": "success",
        "stage": "completed",
        "reason": null,
        "selected_rect": selected_rect,
        "capture_rect": capture_rect,
        "configured_ocr_engine": cfg.ocr_engine,
        "used_ocr_engine": ocr_result.engine,
        "source_lang": cfg.source_lang,
        "target_lang": cfg.target_lang,
        "translator": cfg.translator,
        "raw_text": raw_text,
        "translated_text": translated_text,
        "ocr_lines": ocr_result.lines,
        "image_blocks": image_blocks,
        "files": {
            "source": "source.png",
            "translated": "translated.png",
            "ocr_preprocessed": ocr_result.preprocessed_png.as_ref().map(|_| "ocr_preprocessed.png")
        }
    });
    fs::write(
        entry_dir.join("metadata.json"),
        serde_json::to_string_pretty(&metadata)?,
    )
    .map_err(|e| anyhow::anyhow!("写入翻译日志元数据失败：{e}"))?;

    Ok(Some(entry_id))
}

fn create_translation_diagnostic_log_entry(
    cfg: &AppConfig,
    source_png: Option<&[u8]>,
    ocr_result: Option<&OcrPipelineResult>,
    selected_rect: Rect,
    capture_rect: Rect,
    stage: &str,
    reason: &str,
    raw_text: &str,
) -> anyhow::Result<Option<String>> {
    let Some((entry_id, entry_dir, now)) = create_translation_log_entry_dir(cfg)? else {
        return Ok(None);
    };

    let has_source = source_png.is_some();
    if let Some(source_png) = source_png {
        fs::write(entry_dir.join("source.png"), source_png)
            .map_err(|e| anyhow::anyhow!("写入诊断原图日志失败：{e}"))?;
    }
    let has_preprocessed = ocr_result
        .and_then(|result| result.preprocessed_png.as_ref())
        .is_some();
    if let Some(preprocessed) = ocr_result.and_then(|result| result.preprocessed_png.as_ref()) {
        fs::write(entry_dir.join("ocr_preprocessed.png"), preprocessed)
            .map_err(|e| anyhow::anyhow!("写入诊断 OCR 预处理图失败：{e}"))?;
    }

    let metadata = serde_json::json!({
        "created_at": now.to_rfc3339(),
        "status": "failed",
        "stage": stage,
        "reason": reason,
        "selected_rect": selected_rect,
        "capture_rect": capture_rect,
        "configured_ocr_engine": cfg.ocr_engine,
        "used_ocr_engine": ocr_result.map(|result| result.engine.as_str()),
        "source_lang": cfg.source_lang,
        "target_lang": cfg.target_lang,
        "translator": cfg.translator,
        "raw_text": raw_text,
        "translated_text": "",
        "ocr_lines": ocr_result.map(|result| &result.lines),
        "image_blocks": Vec::<ImageReplacementBlock>::new(),
        "files": {
            "source": has_source.then_some("source.png"),
            "translated": null,
            "ocr_preprocessed": has_preprocessed.then_some("ocr_preprocessed.png")
        }
    });
    fs::write(
        entry_dir.join("metadata.json"),
        serde_json::to_string_pretty(&metadata)?,
    )
    .map_err(|e| anyhow::anyhow!("写入诊断日志元数据失败：{e}"))?;

    Ok(Some(entry_id))
}

fn cleanup_translation_logs(retention_days: u32) -> anyhow::Result<()> {
    let root = translation_log_root()?;
    if !root.exists() {
        return Ok(());
    }
    let keep_days = retention_days.clamp(1, 365) as i64;
    let cutoff = Local::now().date_naive() - chrono::Duration::days(keep_days - 1);
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(day) = NaiveDate::parse_from_str(name, "%Y-%m-%d") else {
            continue;
        };
        if day < cutoff {
            fs::remove_dir_all(&path)
                .map_err(|e| anyhow::anyhow!("清理旧翻译日志失败：{}，{e}", path.display()))?;
        }
    }
    Ok(())
}

fn save_translation_log_render_inner(
    request: SaveTranslationLogRenderRequest,
) -> anyhow::Result<()> {
    let entry_dir = resolve_translation_log_entry_dir(&request.entry_id)?;
    fs::create_dir_all(&entry_dir)?;
    let png = decode_png_data_url(&request.translated_image_data_url)?;
    fs::write(entry_dir.join("translated.png"), png)
        .map_err(|e| anyhow::anyhow!("写入翻译后图片日志失败：{e}"))?;
    Ok(())
}

fn resolve_translation_log_entry_dir(entry_id: &str) -> anyhow::Result<PathBuf> {
    let mut parts = entry_id.split('/');
    let Some(day) = parts.next() else {
        anyhow::bail!("翻译日志 ID 无效");
    };
    let Some(entry) = parts.next() else {
        anyhow::bail!("翻译日志 ID 无效");
    };
    if parts.next().is_some()
        || NaiveDate::parse_from_str(day, "%Y-%m-%d").is_err()
        || !entry
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        anyhow::bail!("翻译日志 ID 无效");
    }
    Ok(translation_log_root()?.join(day).join(entry))
}

fn decode_png_data_url(data_url: &str) -> anyhow::Result<Vec<u8>> {
    let encoded = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| anyhow::anyhow!("翻译后图片必须是 PNG data URL"))?;
    general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| anyhow::anyhow!("解析翻译后图片失败：{e}"))
}

fn auto_detect_selection_rect(
    app: &tauri::AppHandle,
    anchor: Point,
) -> anyhow::Result<Option<Rect>> {
    let mut area = Rect {
        x: anchor.x - 380,
        y: anchor.y - 280,
        width: 760,
        height: 560,
    };
    if let Some(monitor) = app.primary_monitor()? {
        let pos = monitor.position();
        let size = monitor.size();
        let left = pos.x;
        let top = pos.y;
        let right = pos.x + size.width as i32;
        let bottom = pos.y + size.height as i32;
        let x1 = area.x.clamp(left, right);
        let y1 = area.y.clamp(top, bottom);
        let x2 = (area.x + area.width).clamp(left, right);
        let y2 = (area.y + area.height).clamp(top, bottom);
        area = Rect {
            x: x1,
            y: y1,
            width: (x2 - x1).max(0),
            height: (y2 - y1).max(0),
        };
    }
    if area.width < 64 || area.height < 64 {
        return Ok(None);
    }
    let png = capture_rect_png(area)?;
    let image = image::load_from_memory(&png)?.to_rgba8();
    let local_anchor = Point {
        x: anchor.x - area.x,
        y: anchor.y - area.y,
    };
    let Some(local_rect) = detect_text_or_ui_region(&image, local_anchor) else {
        return Ok(None);
    };
    Ok(Some(Rect {
        x: area.x + local_rect.x,
        y: area.y + local_rect.y,
        width: local_rect.width,
        height: local_rect.height,
    }))
}

fn detect_text_or_ui_region(image: &RgbaImage, anchor: Point) -> Option<Rect> {
    let (width, height) = image.dimensions();
    if width < 32 || height < 32 {
        return None;
    }
    let width_usize = width as usize;
    let height_usize = height as usize;
    let mut mask = vec![false; width_usize * height_usize];
    let mut luma = vec![0u8; width_usize * height_usize];
    for y in 0..height_usize {
        for x in 0..width_usize {
            let px = image.get_pixel(x as u32, y as u32).0;
            luma[y * width_usize + x] =
                ((px[0] as u32 * 30 + px[1] as u32 * 59 + px[2] as u32 * 11) / 100) as u8;
        }
    }
    for y in 1..height_usize.saturating_sub(1) {
        for x in 1..width_usize.saturating_sub(1) {
            let idx = y * width_usize + x;
            let gx = (luma[idx + 1] as i16 - luma[idx - 1] as i16).abs();
            let gy = (luma[idx + width_usize] as i16 - luma[idx - width_usize] as i16).abs();
            if gx + gy > 42 {
                for dy in -4..=4 {
                    for dx in -8..=8 {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx >= 0 && ny >= 0 && nx < width as i32 && ny < height as i32 {
                            mask[ny as usize * width_usize + nx as usize] = true;
                        }
                    }
                }
            }
        }
    }

    let mut visited = vec![false; mask.len()];
    let mut best: Option<(Rect, i64)> = None;
    let anchor_x = anchor.x.clamp(0, width as i32 - 1);
    let anchor_y = anchor.y.clamp(0, height as i32 - 1);
    let mut queue = std::collections::VecDeque::new();
    for y in 0..height_usize {
        for x in 0..width_usize {
            let start_idx = y * width_usize + x;
            if !mask[start_idx] || visited[start_idx] {
                continue;
            }
            visited[start_idx] = true;
            queue.clear();
            queue.push_back((x as i32, y as i32));
            let mut min_x = x as i32;
            let mut max_x = x as i32;
            let mut min_y = y as i32;
            let mut max_y = y as i32;
            let mut count = 0i32;
            while let Some((cx, cy)) = queue.pop_front() {
                count += 1;
                min_x = min_x.min(cx);
                max_x = max_x.max(cx);
                min_y = min_y.min(cy);
                max_y = max_y.max(cy);
                for (nx, ny) in [(cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)] {
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    let idx = ny as usize * width_usize + nx as usize;
                    if mask[idx] && !visited[idx] {
                        visited[idx] = true;
                        queue.push_back((nx, ny));
                    }
                }
            }
            let rect = Rect {
                x: min_x,
                y: min_y,
                width: max_x - min_x + 1,
                height: max_y - min_y + 1,
            };
            if count < 120 || rect.width < 24 || rect.height < 12 {
                continue;
            }
            let distance = distance_to_rect(anchor_x, anchor_y, rect);
            if distance > 90 {
                continue;
            }
            let area = (rect.width * rect.height).max(1);
            let density = count as f32 / area as f32;
            if density < 0.02 {
                continue;
            }
            let score = distance as i64 * 1000 - area as i64;
            if best.as_ref().map(|(_, s)| score < *s).unwrap_or(true) {
                best = Some((rect, score));
            }
        }
    }
    best.map(|(rect, _)| pad_rect(rect, 14, width as i32, height as i32))
}

fn distance_to_rect(x: i32, y: i32, rect: Rect) -> i32 {
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;
    let dx = if x < left {
        left - x
    } else if x > right {
        x - right
    } else {
        0
    };
    let dy = if y < top {
        top - y
    } else if y > bottom {
        y - bottom
    } else {
        0
    };
    dx.max(dy)
}

fn pad_rect(rect: Rect, padding: i32, width: i32, height: i32) -> Rect {
    let x1 = (rect.x - padding).clamp(0, width);
    let y1 = (rect.y - padding).clamp(0, height);
    let x2 = (rect.x + rect.width + padding).clamp(0, width);
    let y2 = (rect.y + rect.height + padding).clamp(0, height);
    Rect {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0),
        height: (y2 - y1).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn detects_nearby_ui_text_region() {
        let mut image = RgbaImage::from_pixel(320, 220, Rgba([245, 247, 250, 255]));
        for y in 60..150 {
            for x in 50..260 {
                image.put_pixel(x, y, Rgba([210, 225, 242, 255]));
            }
        }
        for x in 50..260 {
            image.put_pixel(x, 60, Rgba([120, 150, 190, 255]));
            image.put_pixel(x, 149, Rgba([120, 150, 190, 255]));
        }
        for y in 60..150 {
            image.put_pixel(50, y, Rgba([120, 150, 190, 255]));
            image.put_pixel(259, y, Rgba([120, 150, 190, 255]));
        }
        for row in 0..4 {
            let y = 82 + row * 14;
            for x in 78..210 {
                if x % 9 < 6 {
                    for yy in y..y + 3 {
                        image.put_pixel(x, yy, Rgba([35, 45, 60, 255]));
                    }
                }
            }
        }
        let rect = detect_text_or_ui_region(&image, Point { x: 120, y: 96 })
            .expect("expected auto-detected region");
        assert!(rect.x <= 70, "{rect:?}");
        assert!(rect.y <= 76, "{rect:?}");
        assert!(rect.width >= 120, "{rect:?}");
        assert!(rect.height >= 45, "{rect:?}");
    }

    #[test]
    fn image_replacement_groups_card_columns_without_screen_wide_bands() {
        let mut lines = Vec::new();
        for x in [86.0, 440.0, 798.0, 1154.0] {
            lines.push(test_ocr_line(
                "Gain immense amounts of",
                x,
                350.0,
                250.0,
                24.0,
            ));
            lines.push(test_ocr_line(
                "execution damage.",
                x + 18.0,
                382.0,
                214.0,
                24.0,
            ));
        }

        let groups = group_ocr_lines(&lines, 1600, 700);

        assert_eq!(groups.len(), 4, "{groups:#?}");
        let units = image_replacement_translation_units(&groups);
        assert_eq!(units.len(), 4, "{units:#?}");
        assert!(
            units
                .iter()
                .all(|unit| unit.kind == TranslationUnitKind::Paragraph),
            "two-line card copy should stay paragraph units, got {units:#?}"
        );
        for group in groups {
            assert_eq!(group.line_count, 2, "{group:#?}");
            assert!(
                group.rect.width < 310.0,
                "replacement block should stay inside its source card, got {group:#?}"
            );
            assert!(
                group.rect.width > 230.0,
                "replacement block should still cover its local source text, got {group:#?}"
            );
        }
    }

    #[test]
    fn image_replacement_translates_full_card_paragraph_instead_of_color_fragments() {
        let lines = vec![
            test_ocr_line("Become immune to most", 264.0, 513.0, 187.0, 19.0),
            test_ocr_line("attacks and 100% ATK and", 257.0, 535.0, 201.0, 17.0),
            test_ocr_line("MATK for the first 15", 278.0, 552.0, 156.0, 19.0),
            test_ocr_line("seconds of the fight. Does", 259.0, 574.0, 196.0, 17.0),
            test_ocr_line("NOT ignore iframe", 287.0, 591.0, 138.0, 18.0),
            test_ocr_line("penetrating attacks", 286.0, 614.0, 147.0, 17.0),
        ];

        let groups = group_ocr_lines(&lines, 1600, 700);

        assert_eq!(groups.len(), 1, "{groups:#?}");
        assert_eq!(groups[0].line_count, 6);
        assert_eq!(
            groups[0].text,
            "Become immune to most\nattacks and 100% ATK and\nMATK for the first 15\nseconds of the fight. Does\nNOT ignore iframe\npenetrating attacks"
        );
        assert_eq!(
            image_replacement_translation_source(&groups[0].text),
            "Become immune to most attacks and 100% ATK and MATK for the first 15 seconds of the fight. Does NOT ignore iframe penetrating attacks"
        );
        let started = std::time::Instant::now();
        let units = image_replacement_translation_units(&groups);
        let elapsed = started.elapsed();
        assert!(
            elapsed <= std::time::Duration::from_millis(20),
            "paragraph strategy should stay lightweight, elapsed {elapsed:?}"
        );
        assert_eq!(units.len(), 1, "{units:#?}");
        assert_eq!(units[0].kind, TranslationUnitKind::Paragraph);
        assert_eq!(
            units[0].translation_source,
            "Become immune to most attacks and 100% ATK and MATK for the first 15 seconds of the fight. Does NOT ignore iframe penetrating attacks"
        );
    }

    #[test]
    fn image_replacement_default_preserves_ui_lines() {
        let groups = vec![test_visual_group(vec![
            test_visual_line("Leave", 2.0, 82.0, 65.0, 26.0),
            test_visual_line("READY", 177.0, 229.0, 68.0, 25.0),
            test_visual_line("Lettie'", 34.0, 260.0, 63.0, 24.0),
            test_visual_line("Dning Ground", 325.0, 477.0, 129.0, 22.0),
        ])];

        let started = std::time::Instant::now();
        let units = image_replacement_translation_units(&groups);
        let elapsed = started.elapsed();
        assert!(
            elapsed <= std::time::Duration::from_millis(20),
            "line-preserve strategy should stay lightweight, elapsed {elapsed:?}"
        );
        assert_eq!(units.len(), 4, "{units:#?}");
        assert!(
            units
                .iter()
                .all(|unit| unit.kind == TranslationUnitKind::LinePreserve),
            "short game UI text should preserve original lines, got {units:#?}"
        );
        assert!(
            units.iter().all(|unit| unit.line_count == 1),
            "line-preserve units should render one OCR line each, got {units:#?}"
        );
        assert!(
            units.iter().all(|unit| unit.rect.height <= 30.0),
            "line-preserve units should not create tall overlay blocks, got {units:#?}"
        );
    }

    #[test]
    fn image_replacement_tooltip_stat_lines_keep_hard_breaks() {
        let groups = tooltip_structured_lines_fixture();

        let started = std::time::Instant::now();
        let units = image_replacement_translation_units(&groups);
        let elapsed = started.elapsed();
        assert!(
            elapsed <= std::time::Duration::from_millis(20),
            "tooltip layout strategy should stay lightweight, elapsed {elapsed:?}"
        );

        let stat_units = units
            .iter()
            .filter(|unit| unit.kind == TranslationUnitKind::StructuredLine)
            .collect::<Vec<_>>();
        assert_eq!(stat_units.len(), 7, "{units:#?}");
        assert!(
            stat_units.iter().all(|unit| unit.line_count == 1),
            "structured tooltip rows should render one source row per unit, got {units:#?}"
        );
        assert!(
            stat_units.iter().all(|unit| unit.rect.height <= 29.0),
            "structured tooltip rows should not become paragraph blocks, got {units:#?}"
        );
        assert!(
            stat_units
                .iter()
                .any(|unit| unit.source_text == "Magic Dmg : 78400~78400"),
            "Magic Dmg row should stay independent, got {units:#?}"
        );
        assert!(
            stat_units
                .iter()
                .any(|unit| unit.source_text == "Adaptive Stat : 410"),
            "Adaptive Stat row should stay independent, got {units:#?}"
        );

        let paragraph_sources = units
            .iter()
            .filter(|unit| unit.kind == TranslationUnitKind::Paragraph)
            .map(|unit| unit.source_text.as_str())
            .collect::<Vec<_>>();
        assert!(
            paragraph_sources
                .iter()
                .any(|source| *source == "Can be equipped on Lv. 70+ equipment."),
            "soft-wrapped requirement text should still merge as a paragraph, got {units:#?}"
        );
        assert!(
            paragraph_sources.iter().any(|source| source
                .contains("After studying slain dragons, Joorji invented the Dragon Gem.")
                && source.contains("Dragon Gems can be used to increase stats significantly.")),
            "lore text should still merge as a paragraph, got {units:#?}"
        );
    }

    #[test]
    fn image_replacement_settings_form_keeps_control_rows() {
        let groups = settings_form_lines_fixture();

        let started = std::time::Instant::now();
        let units = image_replacement_translation_units(&groups);
        let elapsed = started.elapsed();
        assert!(
            elapsed <= std::time::Duration::from_millis(20),
            "settings form layout strategy should stay lightweight, elapsed {elapsed:?}"
        );

        assert_eq!(units.len(), groups[0].line_count, "{units:#?}");
        assert!(
            units
                .iter()
                .all(|unit| unit.kind == TranslationUnitKind::FormLine),
            "settings form should use form line units, got {units:#?}"
        );
        assert!(
            units.iter().all(|unit| unit.line_count == 1),
            "settings controls should render one OCR row per unit, got {units:#?}"
        );
        assert!(
            units.iter().all(|unit| unit.rect.height <= 40.0),
            "settings controls should not become large paragraph blocks, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .all(|unit| unit.kind != TranslationUnitKind::Paragraph),
            "settings form must not be translated as a paragraph, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .any(|unit| unit.source_text == "Remove camera shake"),
            "checkbox row should stay independent, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .any(|unit| unit.source_text == "Mute other players sounds in field"),
            "sound checkbox row should stay independent, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .any(|unit| unit.source_text == "中文(中國)(Chinese(Simplified))"),
            "language dropdown row should stay independent, got {units:#?}"
        );
        let show_details = units
            .iter()
            .find(|unit| unit.source_text == "Show details in Discord presence")
            .expect("show details row should stay independent");
        assert!(
            show_details.rect.width >= 240.0,
            "form rows should expand to the form column so translated labels do not wrap, got {show_details:#?}"
        );
    }

    #[test]
    fn image_replacement_game_ui_glossary_rewrites_artifact_terms() {
        let started = std::time::Instant::now();
        assert_eq!(
            postprocess_image_replacement_translation(
                "Mana Overload\n(NEW)",
                "Mana超载\n(新)",
                "zh-CN"
            ),
            "法力超载\n（新）"
        );
        assert_eq!(
            postprocess_image_replacement_translation(
                "Lose 2% MP every 2 seconds.\nHowever, gain 20% Critical\nDamage and 10% Critical\nChance.",
                "每2秒损失2% MP。但是，增加20%的暴击伤害和10%暴击的机会。",
                "zh-CN",
            ),
            "每2秒损失 2% MP。\n暴击伤害 +20%，暴击率 +10%。"
        );
        assert_eq!(
            postprocess_image_replacement_translation(
                "Camera FOV increased by 5%.\nAdditionally, gain 10% all\nelemental ATK",
                "相机视野增加5%。另外，获得10%的全部收益元素ATK公司",
                "zh-CN",
            ),
            "视野范围 +5%。\n全元素 ATK +10%。"
        );
        assert_eq!(
            postprocess_image_replacement_translation(
                "Increases your FINAL DAMAGE\nby 2%",
                "增加你的最终伤害2%",
                "zh-CN",
            ),
            "最终伤害 +2%。"
        );
        assert_eq!(
            postprocess_image_replacement_translation(
                "Choose your Artifact wisely! You have 1 reroll per slot.\nParty members choose their own Artifacts",
                "明智地选择你的神器！每个槽有1次掷骰。\n党员选择自己的神器",
                "zh-CN",
            ),
            "谨慎选择神器！每个栏位可重掷 1 次。\n队友会选择自己的神器。"
        );
        assert_eq!(
            postprocess_image_replacement_translation(
                "Camera FOV increased by 5%.",
                "相机视野增加5%，元素ATK公司",
                "en",
            ),
            "相机视野增加5%，元素ATK公司"
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed <= std::time::Duration::from_millis(20),
            "game UI glossary rewrite should stay lightweight, elapsed {elapsed:?}"
        );
    }

    #[test]
    fn image_replacement_chat_log_keeps_message_level_units() {
        let lines = vec![
            test_ocr_line("[20:09:31][ekubeTR]: [HELP ]", 28.0, 16.0, 278.0, 23.0),
            test_ocr_line("[20:09:31][ekubeTR]: [HELP ]", 28.0, 44.0, 276.0, 22.0),
            test_ocr_line("[20:09:32][ekubeTR]: [HELP ]", 29.0, 70.0, 280.0, 23.0),
            test_ocr_line(
                "[20:11:32] ekubeTR successfully enhanced [+20",
                28.0,
                96.0,
                450.0,
                24.0,
            ),
            test_ocr_line("Astral Extinction Gauntlet]", 27.0, 124.0, 252.0, 22.0),
            test_ocr_line("[20:11:42] Reche logged out", 27.0, 151.0, 272.0, 23.0),
            test_ocr_line("[20:11:52][ekubeTR]: [HELP ]", 27.0, 177.0, 277.0, 23.0),
            test_ocr_line("[20:12:22] Reche logged in", 26.0, 203.0, 256.0, 24.0),
        ];

        let started = std::time::Instant::now();
        let groups = group_ocr_lines(&lines, 549, 245);
        assert_eq!(groups.len(), 1, "{groups:#?}");

        let units = image_replacement_translation_units(&groups);
        let elapsed = started.elapsed();
        assert!(
            elapsed <= std::time::Duration::from_millis(20),
            "chat log layout strategy should stay lightweight, elapsed {elapsed:?}"
        );
        assert_eq!(units.len(), 7, "{units:#?}");
        assert_eq!(units[3].line_count, 2, "{units:#?}");
        assert_eq!(
            units[3].translation_source,
            "[20:11:32] ekubeTR successfully enhanced [+20 Astral Extinction Gauntlet]"
        );
        assert!(
            units.iter().all(|unit| unit.rect.height < 64.0),
            "chat log units should stay near their source message lines, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .all(|unit| unit.kind == TranslationUnitKind::ChatLog),
            "chat log should use chat translation units, got {units:#?}"
        );
        let hinted_sizes = units
            .iter()
            .map(|unit| {
                unit.font_size_hint
                    .expect("chat unit should carry font hint")
            })
            .collect::<Vec<_>>();
        assert_eq!(hinted_sizes, vec![16; 7]);
        assert!(
            hinted_sizes.iter().all(|size| (16..=18).contains(size)),
            "chat log font hints should stay visually consistent, got {hinted_sizes:?}"
        );
    }

    #[test]
    fn image_replacement_chat_log_absorbs_split_continuation_regions() {
        let groups = vec![
            test_visual_group(vec![
                test_visual_line(
                    "[21:23:33][Raid Member][SmartBoy]: cc",
                    4.0,
                    7.0,
                    310.0,
                    19.0,
                ),
                test_visual_line("[21:23:46][ReenaPlum]>>> : WAITT", 5.0, 29.0, 272.0, 19.0),
                test_visual_line("[21:23:55][ReenaPlum] <<<: o", 5.0, 51.0, 227.0, 18.0),
                test_visual_line(
                    "[21:23:57][ReenaPlum]>>> : dont tank please,",
                    4.0,
                    72.0,
                    356.0,
                    20.0,
                ),
                test_visual_line("theres too many warriors Q-Q", 4.0, 94.0, 237.0, 21.0),
                test_visual_line("[21:24:04][ReenaPlum] <<<: xd", 3.0, 117.0, 239.0, 19.0),
                test_visual_line(
                    "[21:24:08][Raid Leader][ReenaPlum]:who",
                    4.0,
                    138.0,
                    323.0,
                    20.0,
                ),
                test_visual_line("en age)", 2.0, 154.0, 68.0, 15.0),
            ]),
            test_visual_group(vec![test_visual_line(
                "volunteers to tank",
                0.0,
                163.0,
                151.0,
                19.0,
            )]),
        ];
        assert_eq!(groups.len(), 2, "{groups:#?}");

        let started = std::time::Instant::now();
        let units = image_replacement_translation_units(&groups);
        let elapsed = started.elapsed();
        assert!(
            elapsed <= std::time::Duration::from_millis(20),
            "split chat log layout strategy should stay lightweight, elapsed {elapsed:?}"
        );
        assert_eq!(units.len(), 6, "{units:#?}");
        assert_eq!(
            units[5].source_text,
            "[21:24:08][Raid Leader][ReenaPlum]:who\nen age)\nvolunteers to tank"
        );
        assert!(
            units
                .iter()
                .all(|unit| unit.kind == TranslationUnitKind::ChatLog),
            "all split chat regions should still use chat units, got {units:#?}"
        );
        let hinted_sizes = units
            .iter()
            .map(|unit| {
                unit.font_size_hint
                    .expect("chat unit should carry font hint")
            })
            .collect::<Vec<_>>();
        assert_eq!(hinted_sizes, vec![13; 6]);
        assert!(
            hinted_sizes.iter().all(|size| (13..=14).contains(size)),
            "split chat font hints should stay consistent and near source size, got {hinted_sizes:?}"
        );
    }

    #[test]
    fn image_replacement_chat_log_ignores_preamble_ui_line() {
        let groups = vec![test_visual_group(vec![
            test_visual_line("READY", 164.0, 780.0, 68.0, 22.0),
            test_visual_line(
                "23:03:44][23:03:29][JaymisODA] <<< ie=t.C",
                0.0,
                822.0,
                462.0,
                47.0,
            ),
            test_visual_line("用做副手的", 0.0, 844.0, 100.0, 28.0),
            test_visual_line(
                "23:03:46] You have joined the party, (HC)",
                0.0,
                870.0,
                364.0,
                44.0,
            ),
            test_visual_line(
                "23:03:46] KibboNTOD has become the party leader.",
                0.0,
                894.0,
                445.0,
                48.0,
            ),
            test_visual_line("23:03:461甜不辣", 0.0, 918.0, 146.0, 30.0),
            test_visual_line(
                "23:03:46][Loading Tip]: Feel like you're doing less",
                0.0,
                942.0,
                424.0,
                48.0,
            ),
            test_visual_line(
                "amage than other players with similar gear? Might be",
                1.0,
                966.0,
                456.0,
                48.0,
            ),
            test_visual_line("meito improve your gameplay", 0.0, 989.0, 256.0, 38.0),
        ])];

        let started = std::time::Instant::now();
        let units = image_replacement_translation_units(&groups);
        let elapsed = started.elapsed();
        assert!(
            elapsed <= std::time::Duration::from_millis(20),
            "preamble chat log strategy should stay lightweight, elapsed {elapsed:?}"
        );
        assert_eq!(units.len(), 4, "{units:#?}");
        assert!(
            units.iter().all(|unit| !unit.source_text.contains("READY")),
            "chat strategy should drop preamble UI text, got {units:#?}"
        );
        assert!(
            units.iter().all(|unit| unit.rect.height < 96.0),
            "chat units should not cover the full chat panel, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .all(|unit| unit.kind == TranslationUnitKind::ChatLog),
            "preamble chat should use chat units, got {units:#?}"
        );
    }

    #[test]
    fn image_replacement_visual_regression_outputs_game_ui_svg() {
        let groups = game_ui_visual_fixture();
        let started = std::time::Instant::now();
        let units = image_replacement_translation_units(&groups);
        let elapsed = started.elapsed();
        assert!(
            elapsed <= std::time::Duration::from_millis(20),
            "visual fixture layout should stay lightweight, elapsed {elapsed:?}"
        );
        assert!(
            units.iter().all(|unit| unit.rect.height < 96.0),
            "visual fixture should not create a large chat-covering block, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .filter(|unit| unit.kind == TranslationUnitKind::ChatLog)
                .all(|unit| !unit.source_text.contains("READY")),
            "chat units must not absorb READY preamble, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .any(|unit| unit.kind == TranslationUnitKind::LinePreserve),
            "game UI labels should use line-preserve units, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .any(|unit| unit.kind == TranslationUnitKind::ChatLog),
            "chat text should use chat units, got {units:#?}"
        );

        write_visual_regression_svg("game_ui_line_preserve", 824.0, 1110.0, &groups, &units);
    }

    #[test]
    fn image_replacement_visual_regression_outputs_tooltip_svg() {
        let groups = tooltip_structured_lines_fixture();
        let units = image_replacement_translation_units(&groups);
        assert!(
            units
                .iter()
                .any(|unit| unit.kind == TranslationUnitKind::StructuredLine),
            "tooltip fixture should include structured line units, got {units:#?}"
        );
        assert!(
            units
                .iter()
                .any(|unit| unit.kind == TranslationUnitKind::Paragraph),
            "tooltip fixture should keep soft-wrapped paragraphs, got {units:#?}"
        );

        write_visual_regression_svg("tooltip_structured_lines", 390.0, 723.0, &groups, &units);
    }

    #[test]
    fn image_replacement_visual_regression_outputs_settings_form_svg() {
        let groups = settings_form_lines_fixture();
        let units = image_replacement_translation_units(&groups);
        assert!(
            units
                .iter()
                .any(|unit| unit.kind == TranslationUnitKind::FormLine),
            "settings fixture should include form line units, got {units:#?}"
        );
        assert!(
            units.iter().all(|unit| unit.rect.height <= 40.0),
            "settings fixture should not create oversized blocks, got {units:#?}"
        );

        write_visual_regression_svg("settings_form_lines", 407.0, 1328.0, &groups, &units);
    }

    fn game_ui_visual_fixture() -> Vec<VisualTextGroup> {
        vec![
            test_visual_group(vec![test_visual_line("Leave", 2.0, 82.0, 65.0, 26.0)]),
            test_visual_group(vec![test_visual_line(
                "KIbboNTOD",
                32.0,
                166.0,
                114.0,
                26.0,
            )]),
            test_visual_group(vec![test_visual_line("READY", 176.0, 229.0, 69.0, 26.0)]),
            test_visual_group(vec![test_visual_line("Lettie'", 34.0, 260.0, 63.0, 24.0)]),
            test_visual_group(vec![test_visual_line(
                "Dning Ground",
                325.0,
                477.0,
                129.0,
                22.0,
            )]),
            test_visual_group(vec![test_visual_line("READY", 164.0, 780.0, 68.0, 22.0)]),
            test_visual_group(vec![
                test_visual_line(
                    "23:03:44][23:03:29][JaymisODA] <<< ie=t.C",
                    0.0,
                    822.0,
                    462.0,
                    47.0,
                ),
                test_visual_line("用做副手的", 0.0, 844.0, 100.0, 28.0),
                test_visual_line(
                    "23:03:46] You have joined the party, (HC)",
                    0.0,
                    870.0,
                    364.0,
                    44.0,
                ),
                test_visual_line(
                    "23:03:46] KibboNTOD has become the party leader.",
                    0.0,
                    894.0,
                    445.0,
                    48.0,
                ),
                test_visual_line("23:03:461甜不辣", 0.0, 918.0, 146.0, 30.0),
                test_visual_line(
                    "23:03:46][Loading Tip]: Feel like you're doing less",
                    0.0,
                    942.0,
                    424.0,
                    48.0,
                ),
                test_visual_line(
                    "amage than other players with similar gear? Might be",
                    1.0,
                    966.0,
                    456.0,
                    48.0,
                ),
                test_visual_line("meito improve your gameplay", 0.0, 989.0, 256.0, 38.0),
            ]),
        ]
    }

    fn settings_form_lines_fixture() -> Vec<VisualTextGroup> {
        vec![test_visual_group(vec![
            test_visual_line("SETTINGS", 12.0, 3.0, 96.0, 30.0),
            test_visual_line("GRAPHICS", 12.0, 40.0, 67.0, 23.0),
            test_visual_line("Cap FPS", 34.0, 68.0, 50.0, 18.0),
            test_visual_line("144", 14.0, 90.0, 22.0, 18.0),
            test_visual_line("1000", 15.0, 112.0, 26.0, 18.0),
            test_visual_line("20.000", 13.0, 134.0, 39.0, 18.0),
            test_visual_line("Remove camera shake", 34.0, 155.0, 125.0, 18.0),
            test_visual_line("Remove skill radial blur", 35.0, 178.0, 129.0, 18.0),
            test_visual_line("Remove skill FoV", 35.0, 200.0, 98.0, 18.0),
            test_visual_line("Remove skill threshold filter", 34.0, 222.0, 154.0, 18.0),
            test_visual_line("Disable skill cinematic camera", 34.0, 244.0, 180.0, 18.0),
            test_visual_line("Expand camera pivot range", 37.0, 266.0, 147.0, 18.0),
            test_visual_line("Camera cursor offset", 12.0, 288.0, 120.0, 18.0),
            test_visual_line("35", 127.0, 308.0, 14.0, 18.0),
            test_visual_line("70", 128.0, 329.0, 13.0, 18.0),
            test_visual_line("Ignore camera offset look angle", 35.0, 352.0, 174.0, 18.0),
            test_visual_line("Disable player weapon trails", 34.0, 374.0, 175.0, 18.0),
            test_visual_line(
                "Disable advanced performance optimizations",
                35.0,
                396.0,
                244.0,
                18.0,
            ),
            test_visual_line("High", 17.0, 418.0, 25.0, 18.0),
            test_visual_line("Reset game window position", 113.0, 441.0, 155.0, 18.0),
            test_visual_line("PDSHADE", 13.0, 468.0, 64.0, 23.0),
            test_visual_line("Disable Chromatic Aberration", 37.0, 495.0, 181.0, 18.0),
            test_visual_line("Disable Depth of Field", 34.0, 517.0, 121.0, 18.0),
            test_visual_line("Override day/night time in town", 35.0, 561.0, 169.0, 18.0),
            test_visual_line("10.92", 119.0, 584.0, 28.0, 18.0),
            test_visual_line("Use classic character shading", 36.0, 605.0, 164.0, 18.0),
            test_visual_line("SOUND", 13.0, 633.0, 46.0, 23.0),
            test_visual_line("Do dark class audio filters", 35.0, 660.0, 143.0, 18.0),
            test_visual_line(
                "Mute other players sounds in field",
                35.0,
                681.0,
                185.0,
                18.0,
            ),
            test_visual_line("Mute my mount", 34.0, 705.0, 87.0, 18.0),
            test_visual_line("Mute other players' mounts", 35.0, 726.0, 149.0, 18.0),
            test_visual_line("Mute my pet", 35.0, 748.0, 71.0, 18.0),
            test_visual_line("Mute other players' pets", 35.0, 769.0, 132.0, 18.0),
            test_visual_line(
                "Mute other players' costume weapon sounds",
                36.0,
                792.0,
                242.0,
                18.0,
            ),
            test_visual_line("GAMEPLAY", 11.0, 820.0, 70.0, 23.0),
            test_visual_line("QWERTY", 14.0, 847.0, 49.0, 18.0),
            test_visual_line("Bracket keys adjust sensitivity", 36.0, 869.0, 164.0, 18.0),
            test_visual_line(
                "Disable Windows Snipping Tool shortcut",
                35.0,
                890.0,
                220.0,
                18.0,
            ),
            test_visual_line("Enable IME", 34.0, 913.0, 63.0, 18.0),
            test_visual_line("Disable head look", 35.0, 935.0, 99.0, 18.0),
            test_visual_line("Input Settings", 16.0, 956.0, 93.0, 18.0),
            test_visual_line("Macro/F-key Settings", 17.0, 975.0, 133.0, 18.0),
            test_visual_line("INTERFACE", 12.0, 1003.0, 70.0, 23.0),
            test_visual_line("中文(中國)(Chinese(Simplified))", 13.0, 1029.0, 182.0, 18.0),
            test_visual_line("Energy Bar", 15.0, 1051.0, 77.0, 18.0),
            test_visual_line("DPS Meter", 18.0, 1071.0, 74.0, 18.0),
            test_visual_line("Chat", 18.0, 1091.0, 42.0, 18.0),
            test_visual_line("Show decimals in cooldowns", 35.0, 1111.0, 160.0, 18.0),
            test_visual_line(
                "Show cooldowns on passive skills",
                35.0,
                1134.0,
                186.0,
                18.0,
            ),
            test_visual_line("Damage Numbers", 11.0, 1155.0, 99.0, 18.0),
            test_visual_line("Scaling", 258.0, 1177.0, 39.0, 18.0),
            test_visual_line("+ Edit HUD Layout", 138.0, 1197.0, 105.0, 18.0),
            test_visual_line("Always show mail icon", 35.0, 1220.0, 126.0, 18.0),
            test_visual_line("Hide other players' pets", 35.0, 1241.0, 129.0, 18.0),
            test_visual_line("Hide other players' pet chat", 36.0, 1263.0, 150.0, 18.0),
            test_visual_line(
                "Show details in Discord presence",
                35.0,
                1285.0,
                180.0,
                18.0,
            ),
        ])]
    }

    fn tooltip_structured_lines_fixture() -> Vec<VisualTextGroup> {
        vec![
            test_visual_group(vec![test_visual_line(
                "Legacy Mystical Dragon Gem",
                53.0,
                52.0,
                286.0,
                23.0,
            )]),
            test_visual_group(vec![test_visual_line("绑定", 177.0, 100.0, 36.0, 22.0)]),
            test_visual_group(vec![
                test_visual_line("Tier: T6 - Extinction", 36.0, 148.0, 203.0, 20.0),
                test_visual_line("Rarity: Unique", 36.0, 170.0, 132.0, 20.0),
                test_visual_line("(剩余封印次数: 2)", 36.0, 192.0, 154.0, 20.0),
            ]),
            test_visual_group(vec![test_visual_line(
                "Can be put in Server Storage",
                36.0,
                242.0,
                276.0,
                22.0,
            )]),
            test_visual_group(vec![
                test_visual_line("[Enhancement Stats]", 36.0, 288.0, 197.0, 20.0),
                test_visual_line("Cannot be enhanced", 36.0, 310.0, 190.0, 20.0),
            ]),
            test_visual_group(vec![test_visual_line(
                "Crafted Item Basic Stats",
                36.0,
                359.0,
                252.0,
                22.0,
            )]),
            test_visual_group(vec![
                test_visual_line("Magic Dmg : 78400~78400", 54.0, 405.0, 257.0, 21.0),
                test_visual_line("Adaptive Stat : 410", 54.0, 427.0, 189.0, 21.0),
                test_visual_line("Magic Dmg : 8.20%~8.20%", 54.0, 449.0, 267.0, 21.0),
            ]),
            test_visual_group(vec![
                test_visual_line("(Dragon Gem)", 36.0, 499.0, 131.0, 22.0),
                test_visual_line("Can be equipped on Lv. 70+", 36.0, 522.0, 285.0, 21.0),
                test_visual_line("equipment.", 36.0, 545.0, 105.0, 21.0),
            ]),
            test_visual_group(vec![
                test_visual_line("After studying slain dragons,", 36.0, 592.0, 284.0, 21.0),
                test_visual_line("Joorji invented the Dragon Gem.", 36.0, 615.0, 297.0, 21.0),
                test_visual_line("Dragon Gems can be used to", 36.0, 638.0, 275.0, 21.0),
                test_visual_line("increase stats significantly.", 36.0, 661.0, 274.0, 21.0),
            ]),
        ]
    }

    fn write_visual_regression_svg(
        name: &str,
        width: f32,
        height: f32,
        groups: &[VisualTextGroup],
        units: &[TranslationUnit],
    ) {
        let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("visual-regression");
        std::fs::create_dir_all(&out_dir).expect("create visual regression output directory");
        let out_path = out_dir.join(format!("{name}.svg"));
        let mut svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
<rect width="100%" height="100%" fill="#1f2937"/>
<text x="12" y="24" fill="#e5e7eb" font-size="18">Visual regression: {}</text>
"##,
            xml_escape(name)
        );
        for group in groups {
            for line in &group.lines {
                svg.push_str(&format!(
                    r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" stroke="#38bdf8" stroke-width="1"/>
<text x="{:.1}" y="{:.1}" fill="#bae6fd" font-size="12">{}</text>
"##,
                    line.rect.x,
                    line.rect.y,
                    line.rect.width,
                    line.rect.height,
                    line.rect.x,
                    (line.rect.y - 3.0).max(12.0),
                    xml_escape(&line.text)
                ));
            }
        }
        for unit in units {
            let (fill, stroke) = match unit.kind {
                TranslationUnitKind::LinePreserve => ("rgba(34,197,94,0.28)", "#22c55e"),
                TranslationUnitKind::StructuredLine => ("rgba(14,165,233,0.28)", "#0ea5e9"),
                TranslationUnitKind::FormLine => ("rgba(250,204,21,0.28)", "#facc15"),
                TranslationUnitKind::Paragraph => ("rgba(168,85,247,0.25)", "#a855f7"),
                TranslationUnitKind::ChatLog => ("rgba(249,115,22,0.25)", "#f97316"),
            };
            svg.push_str(&format!(
                r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{fill}" stroke="{stroke}" stroke-width="2"/>
<text x="{:.1}" y="{:.1}" fill="{stroke}" font-size="13">{} {:?}</text>
"##,
                unit.rect.x,
                unit.rect.y,
                unit.rect.width,
                unit.rect.height,
                unit.rect.x,
                unit.rect.bottom() + 14.0,
                xml_escape(&unit.source_text),
                unit.kind
            ));
        }
        svg.push_str("</svg>\n");
        std::fs::write(&out_path, svg).expect("write visual regression svg");
    }

    fn xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    fn test_visual_line(text: &str, x: f32, y: f32, width: f32, height: f32) -> VisualTextLine {
        VisualTextLine {
            text: text.to_string(),
            rect: FloatRect {
                x,
                y,
                width,
                height,
            },
        }
    }

    fn test_visual_group(lines: Vec<VisualTextLine>) -> VisualTextGroup {
        let mut iter = lines.iter();
        let first = iter.next().expect("test visual group needs a line");
        let rect = iter.fold(first.rect, |rect, line| rect.union(line.rect));
        VisualTextGroup {
            text: lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            rect,
            line_count: lines.len(),
            lines,
        }
    }

    fn test_ocr_line(text: &str, x: f32, y: f32, width: f32, height: f32) -> OcrTextLine {
        OcrTextLine {
            text: text.to_string(),
            bbox: [x, y, x + width, y, x + width, y + height, x, y + height],
        }
    }

    #[test]
    fn builds_translation_blocks_from_wrapped_ocr_text() {
        let blocks = ocr_translation_blocks(
            "[Passive Benefit]\n\
You can manually trigger the\n\
[Vanguard] versions of [Shield\n\
Charge], [Thrust Force], and\n\
[Overwhelm] by pressing the\n\
[Regular Attack Button] while you\n\
have at least 10 [Guardian's\n\
Graces], consuming them. If you do\n\
not have enough, the normal\n\
execution will occur.\n\
Parrying an attack using a\n\
[Vanguard] reduces the damage of\n\
the incoming attack to a maximum\n\
of 10% of Max HP.\n\
[Buff]\n\
Movement Speed increased by 50%",
        );
        assert_eq!(blocks[0], "[Passive Benefit]");
        assert_eq!(
            blocks[1],
            "You can manually trigger the [Vanguard] versions of [Shield Charge], [Thrust Force], and [Overwhelm] by pressing the [Regular Attack Button] while you have at least 10 [Guardian's Graces], consuming them. If you do not have enough, the normal execution will occur."
        );
        assert_eq!(
            blocks[2],
            "Parrying an attack using a [Vanguard] reduces the damage of the incoming attack to a maximum of 10% of Max HP."
        );
        assert_eq!(blocks[3], "[Buff]");
        assert_eq!(blocks[4], "Movement Speed increased by 50%");
    }

    #[test]
    fn estimate_overlay_size_keeps_configured_width_for_wrapped_text() {
        let (width, height) = estimate_overlay_size(
            "[Passive Benefit]\nYou can manually trigger the\n[Vanguard] versions of [Shield\nCharge]",
            320,
            18,
        );
        assert_eq!(width, 320);
        assert!(height >= 54);
    }

    #[test]
    fn display_text_reflows_ocr_visual_lines() {
        let text = ocr_display_text(
            "[Passive Benefit]\n\
You can manually trigger the\n\
[Vanguard] versions of [Shield\n\
Charge]\n\
[Attributes]\n\
Invulnerable while casting",
        );
        assert_eq!(
            text,
            "[Passive Benefit]\n\nYou can manually trigger the [Vanguard] versions of [Shield Charge]\n\n[Attributes]\n\nInvulnerable while casting"
        );
    }
}

fn clear_overlay_payload(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut last_overlay) = state.last_overlay.lock() {
            *last_overlay = None;
        }
    }
}

fn show_status_overlay(app: &tauri::AppHandle, anchor: Point, text: &str) -> anyhow::Result<()> {
    let width = 188u32;
    let height = 44u32;
    let mut x = anchor.x + 14;
    let mut y = anchor.y + 14;
    let mut window_width = width;
    let mut window_height = height;
    let cfg = app
        .try_state::<AppState>()
        .and_then(|state| state.config.lock().ok().map(|cfg| cfg.clone()))
        .unwrap_or_default();
    let _ = clamp_overlay_to_primary_monitor(
        app,
        &cfg,
        &mut x,
        &mut y,
        &mut window_width,
        &mut window_height,
    );

    let window = if let Some(window) = app.get_webview_window("status") {
        window
    } else {
        WebviewWindowBuilder::new(app, "status", WebviewUrl::App("status.html".into()))
            .title("OCR 状态")
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .focusable(false)
            .resizable(false)
            .skip_taskbar(true)
            .visible(false)
            .inner_size(width as f64, height as f64)
            .build()?
    };
    window.set_size(PhysicalSize::new(width, height))?;
    window.set_position(PhysicalPosition::new(x, y))?;
    let _ = window.set_focusable(false);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_always_on_top(true);
    window.show()?;
    window.emit(
        "status-overlay-update",
        StatusOverlayPayload {
            text: text.to_string(),
            done: false,
        },
    )?;
    Ok(())
}

fn hide_status_overlay(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("status") {
        let _ = window.hide();
    }
}

async fn run_pipeline(
    app: tauri::AppHandle,
    cfg: AppConfig,
    payload: SelectionPayload,
    frozen_screen: Option<FrozenScreen>,
) -> anyhow::Result<()> {
    cleanup_selection_layers(&app);
    let _ = show_status_overlay(&app, payload.anchor, "正在截图...");
    app.emit("ocr-status", "正在截图...")?;
    let selected_rect = payload.rect.normalized();
    let capture_rect = selected_rect;
    let png = match frozen_screen
        .as_ref()
        .map(|screen| crop_frozen_screen_png(screen, capture_rect))
        .unwrap_or_else(|| capture_rect_png(capture_rect))
    {
        Ok(png) => png,
        Err(err) => {
            eprintln!("capture failed: {err}");
            if let Err(log_err) = create_translation_diagnostic_log_entry(
                &cfg,
                None,
                None,
                selected_rect,
                capture_rect,
                "capture",
                &err.to_string(),
                "",
            ) {
                eprintln!("translation diagnostic log failed: {log_err}");
            }
            show_user_message(&app, &cfg, payload.anchor, "截图没有成功，请重新试一次。")?;
            return Ok(());
        }
    };
    let _ = show_status_overlay(&app, payload.anchor, "正在识别文字...");
    app.emit("ocr-status", "正在识别文字...")?;
    let ocr_result = match recognize_png_pipeline(
        &png,
        OcrPipelineRequest {
            engine: cfg.ocr_engine.clone(),
            source_lang: cfg.source_lang.clone(),
            save_preprocessed: true,
        },
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            eprintln!("ocr failed: {err}");
            if let Err(log_err) = create_translation_diagnostic_log_entry(
                &cfg,
                Some(&png),
                None,
                selected_rect,
                capture_rect,
                "ocr",
                &err.to_string(),
                "",
            ) {
                eprintln!("translation diagnostic log failed: {log_err}");
            }
            show_user_message(
                &app,
                &cfg,
                payload.anchor,
                "没看到可识别的文字，换个区域再试试。",
            )?;
            return Ok(());
        }
    };
    if let Ok(dir) = app_core::config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("last_capture.png"), &png);
        let _ = std::fs::write(
            dir.join("last_capture_meta.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "selected_rect": selected_rect,
                "capture_rect": capture_rect,
                "padding": 0,
                "configured_ocr_engine": cfg.ocr_engine,
                "used_ocr_engine": ocr_result.engine,
                "source_lang": cfg.source_lang,
                "target_lang": cfg.target_lang
            }))?,
        );
        if let Some(preprocessed) = &ocr_result.preprocessed_png {
            let _ = std::fs::write(dir.join("last_ocr_preprocessed.png"), preprocessed);
        }
    }
    let raw_text = ocr_result.text.trim().to_string();
    if raw_text.is_empty() {
        if let Err(log_err) = create_translation_diagnostic_log_entry(
            &cfg,
            Some(&png),
            Some(&ocr_result),
            selected_rect,
            capture_rect,
            "ocr_empty",
            "OCR returned empty text",
            &raw_text,
        ) {
            eprintln!("translation diagnostic log failed: {log_err}");
        }
        show_user_message(
            &app,
            &cfg,
            payload.anchor,
            "没看到可识别的文字，换个区域再试试。",
        )?;
        return Ok(());
    }
    app.emit("ocr-status", format!("已识别：{raw_text}"))?;

    let settings = cfg
        .provider_settings
        .get(&cfg.translator)
        .cloned()
        .unwrap_or_default();
    let _ = show_status_overlay(&app, payload.anchor, "正在翻译...");
    app.emit("ocr-status", "正在翻译...")?;
    let image_blocks = if cfg.overlay.result_mode == "image_replace" && !ocr_result.lines.is_empty()
    {
        build_image_replacement_blocks(&cfg, &settings, &png, &ocr_result.lines)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let translated = if !image_blocks.is_empty() {
        image_blocks
            .iter()
            .map(|block| block.translated_text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    } else {
        translate_preserving_lines(&cfg, &settings, &raw_text)
            .await
            .unwrap_or_else(|_| format!("翻译没有成功，请稍后再试。\n\n原文：\n{raw_text}"))
    };
    let log_entry_id = if !image_blocks.is_empty() {
        match create_translation_log_entry(
            &cfg,
            &png,
            &ocr_result,
            selected_rect,
            capture_rect,
            &raw_text,
            &translated,
            &image_blocks,
        ) {
            Ok(entry_id) => entry_id,
            Err(err) => {
                eprintln!("translation log failed: {err}");
                None
            }
        }
    } else {
        None
    };
    let _ = show_status_overlay(&app, payload.anchor, "正在显示结果...");
    show_overlay(
        &app,
        &cfg,
        payload.anchor,
        raw_text,
        translated,
        Some(capture_rect),
        Some(&png),
        image_blocks,
        log_entry_id,
    )?;
    app.emit("ocr-status", "完成")?;
    Ok(())
}

fn show_user_message(
    app: &tauri::AppHandle,
    cfg: &AppConfig,
    anchor: Point,
    message: &str,
) -> anyhow::Result<()> {
    cleanup_selection_layers(app);
    hide_status_overlay(app);
    let _ = app.emit("ocr-status", message);
    show_overlay(
        app,
        cfg,
        anchor,
        String::new(),
        message.to_string(),
        None,
        None,
        Vec::new(),
        None,
    )
}

async fn translate_preserving_lines(
    cfg: &AppConfig,
    settings: &std::collections::HashMap<String, String>,
    raw_text: &str,
) -> anyhow::Result<String> {
    let blocks = ocr_translation_blocks(raw_text);
    if blocks.len() <= 1 {
        return translate(TranslationRequest {
            provider_id: cfg.translator.clone(),
            text: blocks
                .first()
                .cloned()
                .unwrap_or_else(|| raw_text.trim().to_string()),
            source_lang: cfg.source_lang.clone(),
            target_lang: cfg.target_lang.clone(),
            settings: settings.clone(),
        })
        .await
        .map(|r| r.text);
    }

    let mut translated = Vec::with_capacity(blocks.len());
    for block in blocks {
        let text = translate(TranslationRequest {
            provider_id: cfg.translator.clone(),
            text: block,
            source_lang: cfg.source_lang.clone(),
            target_lang: cfg.target_lang.clone(),
            settings: settings.clone(),
        })
        .await
        .map(|r| r.text)?;
        translated.push(text.trim().to_string());
    }
    Ok(translated.join("\n\n"))
}

#[derive(Debug, Clone, Copy)]
struct FloatRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl FloatRect {
    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }

    fn center_x(self) -> f32 {
        self.x + self.width / 2.0
    }

    fn union(self, other: Self) -> Self {
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = self.right().max(other.right());
        let y2 = self.bottom().max(other.bottom());
        Self {
            x: x1,
            y: y1,
            width: x2 - x1,
            height: y2 - y1,
        }
    }

    fn padded(self, px: f32, py: f32, image_width: u32, image_height: u32) -> Self {
        let x1 = (self.x - px).max(0.0);
        let y1 = (self.y - py).max(0.0);
        let x2 = (self.right() + px).min(image_width as f32);
        let y2 = (self.bottom() + py).min(image_height as f32);
        Self {
            x: x1,
            y: y1,
            width: (x2 - x1).max(1.0),
            height: (y2 - y1).max(1.0),
        }
    }
}

#[derive(Debug, Clone)]
struct VisualTextLine {
    text: String,
    rect: FloatRect,
}

#[derive(Debug, Clone)]
struct VisualTextGroup {
    text: String,
    rect: FloatRect,
    line_count: usize,
    lines: Vec<VisualTextLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranslationUnitKind {
    LinePreserve,
    StructuredLine,
    FormLine,
    Paragraph,
    ChatLog,
}

#[derive(Debug, Clone)]
struct TranslationUnit {
    source_text: String,
    translation_source: String,
    rect: FloatRect,
    line_count: usize,
    kind: TranslationUnitKind,
    font_size_hint: Option<u32>,
}

async fn build_image_replacement_blocks(
    cfg: &AppConfig,
    settings: &std::collections::HashMap<String, String>,
    png: &[u8],
    lines: &[OcrTextLine],
) -> anyhow::Result<Vec<ImageReplacementBlock>> {
    let image = image::load_from_memory(png)?.to_rgba8();
    let (image_width, image_height) = image.dimensions();
    let regions = group_ocr_lines(lines, image_width, image_height);
    let units = image_replacement_translation_units(&regions);
    let mut blocks = Vec::with_capacity(units.len());

    for unit in units {
        let translated = translate(TranslationRequest {
            provider_id: cfg.translator.clone(),
            text: unit.translation_source.clone(),
            source_lang: cfg.source_lang.clone(),
            target_lang: cfg.target_lang.clone(),
            settings: settings.clone(),
        })
        .await
        .map(|result| result.text)
        .unwrap_or_else(|_| unit.source_text.clone());
        let translated = postprocess_image_replacement_translation(
            &unit.source_text,
            &translated,
            &cfg.target_lang,
        );
        let (background, color) = sample_replacement_colors(&image, unit.rect);
        let base_font =
            estimate_source_font_size(unit.rect, unit.line_count, cfg.overlay.font_size);
        let font_size = unit
            .font_size_hint
            .unwrap_or_else(|| fit_replacement_font_size(&translated, unit.rect, base_font));
        let wrap_mode = if unit.kind == TranslationUnitKind::Paragraph
            || unit.kind == TranslationUnitKind::ChatLog
        {
            "wrap"
        } else {
            "single"
        };
        let align = if unit.kind == TranslationUnitKind::Paragraph
            && is_centered_title(unit.rect, image_width, image_height, unit.line_count)
        {
            "center"
        } else {
            "left"
        };

        blocks.push(ImageReplacementBlock {
            source_text: unit.source_text,
            translated_text: translated.trim().to_string(),
            x: unit.rect.x,
            y: unit.rect.y,
            width: unit.rect.width,
            height: unit.rect.height,
            font_size,
            background,
            color,
            align: align.to_string(),
            wrap_mode: wrap_mode.to_string(),
        });
    }

    Ok(blocks)
}

fn image_replacement_translation_units(groups: &[VisualTextGroup]) -> Vec<TranslationUnit> {
    let mut ordered_groups = groups.iter().collect::<Vec<_>>();
    ordered_groups.sort_by(|a, b| {
        a.rect
            .y
            .partial_cmp(&b.rect.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.rect
                    .x
                    .partial_cmp(&b.rect.x)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut units = Vec::new();
    let mut index = 0usize;
    while index < ordered_groups.len() {
        let group = ordered_groups[index];
        if looks_like_chat_log_group(group) {
            let mut chat_lines = group.lines.clone();
            let mut bottom = group.rect.bottom();
            index += 1;
            while index < ordered_groups.len()
                && is_chat_continuation_group(ordered_groups[index], bottom)
            {
                let continuation = ordered_groups[index];
                bottom = bottom.max(continuation.rect.bottom());
                chat_lines.extend(continuation.lines.iter().cloned());
                index += 1;
            }
            units.extend(chat_log_translation_units_from_lines(&chat_lines));
        } else if looks_like_form_ui_group(group) {
            units.extend(form_line_translation_units(group));
            index += 1;
        } else if looks_like_structured_line_group(group) {
            units.extend(structured_line_translation_units(group));
            index += 1;
        } else if let Some(split_units) = leading_tooltip_heading_translation_units(group) {
            units.extend(split_units);
            index += 1;
        } else if looks_like_paragraph_group(group) {
            units.push(paragraph_translation_unit(group));
            index += 1;
        } else {
            units.extend(line_preserving_translation_units(group));
            index += 1;
        }
    }

    units
}

fn paragraph_translation_unit(group: &VisualTextGroup) -> TranslationUnit {
    let translation_source = image_replacement_translation_source(&group.text);
    TranslationUnit {
        source_text: translation_source.clone(),
        translation_source,
        rect: group.rect,
        line_count: group.line_count,
        kind: TranslationUnitKind::Paragraph,
        font_size_hint: None,
    }
}

fn line_preserving_translation_units(group: &VisualTextGroup) -> Vec<TranslationUnit> {
    line_translation_units(group, TranslationUnitKind::LinePreserve)
}

fn structured_line_translation_units(group: &VisualTextGroup) -> Vec<TranslationUnit> {
    line_translation_units(group, TranslationUnitKind::StructuredLine)
}

fn form_line_translation_units(group: &VisualTextGroup) -> Vec<TranslationUnit> {
    let row_right = group
        .lines
        .iter()
        .map(|line| line.rect.right())
        .fold(group.rect.right(), f32::max);
    group
        .lines
        .iter()
        .filter_map(|line| {
            let translation_source = image_replacement_translation_source(&line.text);
            if translation_source.is_empty() {
                return None;
            }
            let mut rect = line.rect.padded(3.0, 2.0, u32::MAX, u32::MAX);
            rect.width = (row_right - rect.x).max(rect.width);
            Some(TranslationUnit {
                source_text: line.text.trim().to_string(),
                translation_source,
                rect,
                line_count: 1,
                kind: TranslationUnitKind::FormLine,
                font_size_hint: Some(estimate_line_font_size(line)),
            })
        })
        .collect()
}

fn leading_tooltip_heading_translation_units(
    group: &VisualTextGroup,
) -> Option<Vec<TranslationUnit>> {
    if group.line_count < 3 {
        return None;
    }
    let (first, rest) = group.lines.split_first()?;
    if !looks_like_tooltip_heading_line(&first.text) {
        return None;
    }
    let rest_group = visual_text_group_from_lines(rest.to_vec())?;
    if !looks_like_paragraph_group(&rest_group) {
        return None;
    }

    let mut units = line_translation_units(
        &visual_text_group_from_lines(vec![first.clone()])?,
        TranslationUnitKind::StructuredLine,
    );
    units.push(paragraph_translation_unit(&rest_group));
    Some(units)
}

fn line_translation_units(
    group: &VisualTextGroup,
    kind: TranslationUnitKind,
) -> Vec<TranslationUnit> {
    group
        .lines
        .iter()
        .filter_map(|line| {
            let translation_source = image_replacement_translation_source(&line.text);
            if translation_source.is_empty() {
                return None;
            }
            Some(TranslationUnit {
                source_text: line.text.trim().to_string(),
                translation_source,
                rect: line.rect.padded(3.0, 2.0, u32::MAX, u32::MAX),
                line_count: 1,
                kind,
                font_size_hint: Some(estimate_line_font_size(line)),
            })
        })
        .collect()
}

fn estimate_line_font_size(line: &VisualTextLine) -> u32 {
    (line.rect.height.max(1.0) * 0.78).round().clamp(9.0, 28.0) as u32
}

fn chat_log_translation_units_from_lines(lines: &[VisualTextLine]) -> Vec<TranslationUnit> {
    let mut units = Vec::new();
    let mut current_lines: Vec<&VisualTextLine> = Vec::new();
    let mut sorted_lines = lines.to_vec();
    sorted_lines.sort_by(|a, b| {
        a.rect
            .y
            .partial_cmp(&b.rect.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.rect
                    .x
                    .partial_cmp(&b.rect.x)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let font_size_hint = estimate_chat_log_region_font_size(&sorted_lines);

    for line in &sorted_lines {
        let starts_message = starts_with_time_prefix(&line.text);
        if starts_message && !current_lines.is_empty() {
            units.push(chat_log_unit(&current_lines, font_size_hint));
            current_lines.clear();
        }
        if !starts_message && current_lines.is_empty() {
            continue;
        }
        current_lines.push(line);
    }

    if !current_lines.is_empty() {
        units.push(chat_log_unit(&current_lines, font_size_hint));
    }

    units
}

fn chat_log_unit(lines: &[&VisualTextLine], font_size_hint: u32) -> TranslationUnit {
    let mut iter = lines.iter();
    let first = *iter
        .next()
        .expect("chat_log_unit requires a non-empty line list");
    let rect = iter.fold(first.rect, |rect, line| rect.union(line.rect));
    let source_text = lines
        .iter()
        .map(|line| line.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let translation_source = image_replacement_translation_source(&source_text);
    TranslationUnit {
        source_text,
        translation_source,
        rect: rect.padded(3.0, 2.0, u32::MAX, u32::MAX),
        line_count: lines.len(),
        kind: TranslationUnitKind::ChatLog,
        font_size_hint: Some(font_size_hint),
    }
}

fn estimate_chat_log_region_font_size(lines: &[VisualTextLine]) -> u32 {
    let mut heights = lines
        .iter()
        .map(|line| line.rect.height.max(1.0))
        .collect::<Vec<_>>();
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_height = heights[heights.len() / 2];
    (median_height * 0.68).round().clamp(12.0, 22.0) as u32
}

fn looks_like_chat_log_group(group: &VisualTextGroup) -> bool {
    if group.line_count < 3 {
        return false;
    }
    let timestamp_lines = group
        .lines
        .iter()
        .filter(|line| starts_with_time_prefix(&line.text))
        .count();
    timestamp_lines >= 2 && timestamp_lines * 3 >= group.line_count
}

fn is_chat_continuation_group(group: &VisualTextGroup, previous_bottom: f32) -> bool {
    let has_timestamp = group
        .lines
        .iter()
        .any(|line| starts_with_time_prefix(&line.text));
    if has_timestamp || group.line_count > 3 {
        return false;
    }
    let vertical_gap = group.rect.y - previous_bottom;
    vertical_gap <= 64.0 && group.rect.x <= 140.0
}

fn looks_like_form_ui_group(group: &VisualTextGroup) -> bool {
    if group.line_count < 8 {
        return false;
    }
    if group
        .lines
        .iter()
        .any(|line| starts_with_time_prefix(&line.text))
    {
        return false;
    }

    let lines = group
        .lines
        .iter()
        .map(|line| line.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    if lines.len() < 8 {
        return false;
    }

    let section_lines = lines
        .iter()
        .filter(|line| looks_like_form_section_heading(line))
        .count();
    let control_lines = lines
        .iter()
        .filter(|line| looks_like_form_control_line(line))
        .count();
    let value_lines = lines
        .iter()
        .filter(|line| looks_like_form_value_line(line))
        .count();
    let sentence_lines = lines.iter().filter(|line| ends_sentence(line)).count();

    section_lines >= 2
        && control_lines >= 4
        && control_lines + value_lines + section_lines >= lines.len() / 2
        && sentence_lines * 3 <= lines.len()
}

fn looks_like_form_section_heading(line: &str) -> bool {
    let value = line.trim();
    if value.chars().count() > 32 {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    [
        "settings",
        "graphics",
        "pdshade",
        "sound",
        "gameplay",
        "interface",
        "input settings",
        "macro/f-key settings",
        "damage numbers",
    ]
    .iter()
    .any(|token| lower == *token)
}

fn looks_like_form_control_line(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    if lower.is_empty() || lower.chars().count() > 64 {
        return false;
    }
    [
        "remove ",
        "disable ",
        "enable ",
        "expand ",
        "ignore ",
        "override ",
        "use ",
        "mute ",
        "show ",
        "hide ",
        "always ",
        "do ",
        "bracket ",
        "cap ",
        "camera ",
        "water ",
        "keyboard",
        "language",
        "brightness",
        "scaling",
        "time of day",
        "fps cap",
        "max camera",
    ]
    .iter()
    .any(|token| lower.contains(token))
}

fn looks_like_form_value_line(line: &str) -> bool {
    let value = line.trim();
    if value.is_empty() || value.chars().count() > 24 {
        return false;
    }
    value
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | '/' | '%'))
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "high" | "medium" | "low" | "qwerty"
        )
}

fn looks_like_structured_line_group(group: &VisualTextGroup) -> bool {
    if group.line_count < 2 {
        return false;
    }
    if group
        .lines
        .iter()
        .any(|line| starts_with_time_prefix(&line.text))
    {
        return false;
    }
    let lines = group
        .lines
        .iter()
        .map(|line| line.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    if lines.len() < 2 {
        return false;
    }

    let structured_lines = lines
        .iter()
        .filter(|line| looks_like_structured_tooltip_line(line))
        .count();
    structured_lines >= 2 && structured_lines * 2 >= lines.len()
}

fn looks_like_structured_tooltip_line(line: &str) -> bool {
    let value = line.trim();
    if value.is_empty() || ends_sentence(value) {
        return false;
    }

    let has_separator = value.contains(':') || value.contains('：');
    let has_number = value.chars().any(|ch| ch.is_ascii_digit());
    let has_stat_symbol = value.contains('%')
        || value.contains('~')
        || value.contains('+')
        || value.contains('-')
        || value.contains('/');
    let lower = value.to_ascii_lowercase();
    let has_stat_word = [
        "tier",
        "rarity",
        "stat",
        "dmg",
        "damage",
        "atk",
        "matk",
        "def",
        "hp",
        "mp",
        "level",
        "lv",
        "unique",
        "extinction",
    ]
    .iter()
    .any(|token| lower.contains(token));

    has_separator && (has_number || has_stat_symbol || has_stat_word)
}

fn looks_like_tooltip_heading_line(line: &str) -> bool {
    let value = line.trim();
    value.len() <= 48
        && ((value.starts_with('(') && value.ends_with(')'))
            || (value.starts_with('[') && value.ends_with(']')))
}

fn looks_like_paragraph_group(group: &VisualTextGroup) -> bool {
    if group.line_count < 2 {
        return false;
    }
    if group
        .lines
        .iter()
        .any(|line| starts_with_time_prefix(&line.text))
    {
        return false;
    }
    let lines = group
        .lines
        .iter()
        .map(|line| line.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    if lines.len() < 2 {
        return false;
    }
    let char_counts = lines
        .iter()
        .map(|line| line.chars().count())
        .collect::<Vec<_>>();
    let avg_chars = char_counts.iter().sum::<usize>() as f32 / char_counts.len() as f32;
    let short_lines = char_counts.iter().filter(|count| **count < 10).count();
    if avg_chars < 14.0 || short_lines * 2 > char_counts.len() {
        return false;
    }

    let first = lines.first().copied().unwrap_or_default();
    let last = lines.last().copied().unwrap_or_default();
    let first_continues = !ends_sentence(first) && !is_ocr_heading(first);
    let last_completes = ends_sentence(last) || lines.len() >= 3;
    first_continues && last_completes
}

fn starts_with_time_prefix(text: &str) -> bool {
    parse_time_prefix_end(text).is_some()
}

fn parse_time_prefix_end(text: &str) -> Option<usize> {
    let value = text.trim_start();
    let leading_trim = text.len() - value.len();
    let (time_text, end) = if value.starts_with('[') {
        let end = value.find(']')?;
        (value.get(1..end)?, end + 1)
    } else {
        let end = value.find(']')?;
        (value.get(0..end)?, end + 1)
    };
    let parts = time_text.split(':').collect::<Vec<_>>();
    if !(parts.len() == 2 || parts.len() == 3) {
        return None;
    }
    let valid = parts.iter().enumerate().all(|(index, part)| {
        !part.is_empty()
            && part.len() <= if index == 0 { 2 } else { 2 }
            && part.bytes().all(|b| b.is_ascii_digit())
    });
    valid.then_some(leading_trim + end + 1)
}

fn image_replacement_translation_source(text: &str) -> String {
    ocr_translation_blocks(text)
        .join("\n\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn postprocess_image_replacement_translation(
    source_text: &str,
    translated_text: &str,
    target_lang: &str,
) -> String {
    let translated = translated_text.trim();
    if !is_chinese_target_lang(target_lang) {
        return translated.to_string();
    }
    if let Some(text) = translate_known_game_ui_phrase(source_text) {
        return text;
    }
    apply_game_ui_glossary_corrections(translated)
}

fn is_chinese_target_lang(target_lang: &str) -> bool {
    let lang = target_lang.to_ascii_lowercase();
    lang == "zh" || lang.starts_with("zh-") || lang.starts_with("zh_")
}

fn translate_known_game_ui_phrase(source_text: &str) -> Option<String> {
    let compact = compact_ascii_lower(source_text);
    let has_new = compact.contains("(new)") || compact.contains("[new]");

    let title = if compact.contains("ultimate recovery") {
        Some("终极恢复")
    } else if compact.contains("mana overload") {
        Some("法力超载")
    } else if compact.contains("natures gift") || compact.contains("nature's gift") {
        Some("自然馈赠")
    } else if compact.contains("stat shard: final damage") {
        Some("属性碎片：最终伤害")
    } else if compact == "select an artifact" {
        Some("选择神器")
    } else {
        None
    };
    if let Some(title) = title {
        return Some(if has_new {
            format!("{title}\n（新）")
        } else {
            title.to_string()
        });
    }

    if compact.contains("artifact slots") {
        return Some(source_text.trim().replacen("Artifact Slots", "神器栏位", 1));
    }

    if compact.contains("after casting an ultimate ability") && compact.contains("heal") {
        let amount = percent_values(source_text)
            .first()
            .cloned()
            .unwrap_or_else(|| "10%".to_string());
        return Some(format!("释放终极技能后，恢复 {amount} HP 和 MP。"));
    }

    if compact.contains("lose")
        && compact.contains("mp every")
        && compact.contains("critical damage")
        && compact.contains("critical chance")
    {
        let percents = percent_values(source_text);
        let loss = percents
            .first()
            .cloned()
            .unwrap_or_else(|| "2%".to_string());
        let critical_damage = percents
            .get(1)
            .cloned()
            .unwrap_or_else(|| "20%".to_string());
        let critical_chance = percents
            .get(2)
            .cloned()
            .unwrap_or_else(|| "10%".to_string());
        return Some(format!(
            "每2秒损失 {loss} MP。\n暴击伤害 +{critical_damage}，暴击率 +{critical_chance}。"
        ));
    }

    if compact.contains("camera fov")
        && compact.contains("elemental atk")
        && (compact.contains("increased") || compact.contains("gain"))
    {
        let percents = percent_values(source_text);
        let fov = percents
            .first()
            .cloned()
            .unwrap_or_else(|| "5%".to_string());
        let elemental_atk = percents
            .get(1)
            .cloned()
            .unwrap_or_else(|| "10%".to_string());
        return Some(format!("视野范围 +{fov}。\n全元素 ATK +{elemental_atk}。"));
    }

    if compact.contains("increases your final damage")
        || compact.contains("increase your final damage")
    {
        let amount = percent_values(source_text)
            .last()
            .cloned()
            .unwrap_or_else(|| "2%".to_string());
        return Some(format!("最终伤害 +{amount}。"));
    }

    if compact.contains("choose your artifact wisely") && compact.contains("reroll per slot") {
        let rerolls = source_text
            .split_whitespace()
            .find(|token| token.chars().any(|ch| ch.is_ascii_digit()))
            .map(|token| {
                token
                    .trim_matches(|ch: char| !ch.is_ascii_digit())
                    .to_string()
            })
            .filter(|token| !token.is_empty())
            .unwrap_or_else(|| "1".to_string());
        if compact.contains("party members") {
            return Some(format!(
                "谨慎选择神器！每个栏位可重掷 {rerolls} 次。\n队友会选择自己的神器。"
            ));
        }
        return Some(format!("谨慎选择神器！每个栏位可重掷 {rerolls} 次。"));
    }

    None
}

fn apply_game_ui_glossary_corrections(text: &str) -> String {
    let mut value = text.trim().to_string();
    for (from, to) in [
        ("ATK公司", "ATK"),
        ("atk公司", "ATK"),
        ("Atk公司", "ATK"),
        ("暴击的机会", "暴击率"),
        ("暴击机会", "暴击率"),
        ("暴击几率", "暴击率"),
        ("最终损害", "最终伤害"),
        ("神器槽", "神器栏位"),
        ("掷骰", "重掷"),
        ("重新掷骰", "重掷"),
        ("Mana超载", "法力超载"),
        ("相机视野", "视野范围"),
    ] {
        value = value.replace(from, to);
    }
    value = normalize_latin_game_terms(&value);
    value
}

fn normalize_latin_game_terms(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut word = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            word.push(ch);
        } else {
            push_normalized_game_term(&mut output, &word);
            word.clear();
            output.push(ch);
        }
    }
    push_normalized_game_term(&mut output, &word);
    output
}

fn push_normalized_game_term(output: &mut String, word: &str) {
    if word.is_empty() {
        return;
    }
    match word.to_ascii_lowercase().as_str() {
        "hp" => output.push_str("HP"),
        "mp" => output.push_str("MP"),
        "atk" => output.push_str("ATK"),
        "fov" => output.push_str("FOV"),
        _ => output.push_str(word),
    }
}

fn compact_ascii_lower(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn percent_values(value: &str) -> Vec<String> {
    value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | '.' | ';' | ':' | ')' | '('))
        .filter_map(|token| {
            let token = token.trim();
            if token.ends_with('%')
                && token[..token.len() - 1]
                    .chars()
                    .any(|ch| ch.is_ascii_digit())
            {
                Some(token.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn group_ocr_lines(
    lines: &[OcrTextLine],
    image_width: u32,
    image_height: u32,
) -> Vec<VisualTextGroup> {
    let mut visual_lines = lines
        .iter()
        .filter_map(|line| {
            let text = line.text.trim();
            if text.is_empty() {
                return None;
            }
            ocr_bbox_to_rect(&line.bbox, image_width, image_height).map(|rect| VisualTextLine {
                text: text.to_string(),
                rect,
            })
        })
        .collect::<Vec<_>>();
    visual_lines.sort_by(|a, b| {
        a.rect
            .y
            .partial_cmp(&b.rect.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.rect
                    .x
                    .partial_cmp(&b.rect.x)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    if visual_lines.is_empty() {
        return Vec::new();
    }

    let mut heights = visual_lines
        .iter()
        .map(|line| line.rect.height.max(1.0))
        .collect::<Vec<_>>();
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_height = heights[heights.len() / 2].max(12.0);
    let gap_threshold = (median_height * 0.78).max(12.0);
    let x_threshold = (median_height * 2.5).max(28.0);

    let mut groups: Vec<Vec<VisualTextLine>> = Vec::new();
    for line in visual_lines {
        let mut best_group = None;
        let mut best_score = f32::INFINITY;

        for (index, group) in groups.iter().enumerate() {
            let Some(previous) = group.last() else {
                continue;
            };
            let group_rect = visual_group_rect(group);
            let vertical_gap = line.rect.y - previous.rect.bottom();
            let overlaps_previous_row = line.rect.y < previous.rect.bottom() - median_height * 0.35;

            if overlaps_previous_row
                || vertical_gap > gap_threshold
                || !same_text_column(group_rect, line.rect, median_height, x_threshold)
            {
                continue;
            }

            let score =
                vertical_gap.max(0.0) + (line.rect.center_x() - group_rect.center_x()).abs() * 0.2;
            if score < best_score {
                best_score = score;
                best_group = Some(index);
            }
        }

        if let Some(index) = best_group {
            groups[index].push(line);
        } else {
            groups.push(vec![line]);
        }
    }

    groups
        .into_iter()
        .filter_map(|group| {
            let mut iter = group.iter();
            let first = iter.next()?;
            let mut rect = first.rect;
            let mut texts = vec![first.text.clone()];
            let mut line_count = 1usize;
            for line in iter {
                rect = rect.union(line.rect);
                texts.push(line.text.clone());
                line_count += 1;
            }
            let rect = rect.padded(4.0, 3.0, image_width, image_height);
            let text = texts.join("\n");
            Some(VisualTextGroup {
                text,
                rect,
                line_count,
                lines: group,
            })
        })
        .collect()
}

fn visual_group_rect(group: &[VisualTextLine]) -> FloatRect {
    let mut iter = group.iter();
    let first = iter
        .next()
        .expect("visual_group_rect requires a non-empty group")
        .rect;
    iter.fold(first, |rect, line| rect.union(line.rect))
}

fn visual_text_group_from_lines(lines: Vec<VisualTextLine>) -> Option<VisualTextGroup> {
    lines.first()?;
    let rect = visual_group_rect(&lines);
    let text = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Some(VisualTextGroup {
        text,
        rect,
        line_count: lines.len(),
        lines,
    })
}

fn same_text_column(
    group: FloatRect,
    line: FloatRect,
    median_height: f32,
    x_threshold: f32,
) -> bool {
    let overlap = horizontal_overlap(group, line);
    let narrow_width = group.width.min(line.width).max(1.0);
    let overlap_ratio = overlap / narrow_width;
    let center_delta = (group.center_x() - line.center_x()).abs();
    let tolerated_center_delta = (group.width.max(line.width) * 0.56)
        .max(x_threshold)
        .max(median_height * 2.2);

    overlap_ratio >= 0.18
        || center_delta <= tolerated_center_delta
            && line.right() >= group.x - median_height * 0.75
            && line.x <= group.right() + median_height * 0.75
}

fn horizontal_overlap(a: FloatRect, b: FloatRect) -> f32 {
    (a.right().min(b.right()) - a.x.max(b.x)).max(0.0)
}

fn ocr_bbox_to_rect(bbox: &[f32; 8], image_width: u32, image_height: u32) -> Option<FloatRect> {
    let xs = [bbox[0], bbox[2], bbox[4], bbox[6]];
    let ys = [bbox[1], bbox[3], bbox[5], bbox[7]];
    let x1 = xs.iter().copied().fold(f32::INFINITY, f32::min).max(0.0);
    let y1 = ys.iter().copied().fold(f32::INFINITY, f32::min).max(0.0);
    let x2 = xs
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max)
        .min(image_width as f32);
    let y2 = ys
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max)
        .min(image_height as f32);
    let width = x2 - x1;
    let height = y2 - y1;
    (width >= 2.0 && height >= 2.0).then_some(FloatRect {
        x: x1,
        y: y1,
        width,
        height,
    })
}

fn sample_replacement_colors(image: &RgbaImage, rect: FloatRect) -> (String, String) {
    let (background, foreground) = sample_replacement_rgb(image, rect);
    (rgb_hex(background), rgb_hex(foreground))
}

fn sample_replacement_rgb(image: &RgbaImage, rect: FloatRect) -> ([u8; 3], [u8; 3]) {
    let x1 = rect.x.floor().max(0.0) as u32;
    let y1 = rect.y.floor().max(0.0) as u32;
    let x2 = rect.right().ceil().min(image.width() as f32) as u32;
    let y2 = rect.bottom().ceil().min(image.height() as f32) as u32;
    let mut pixels = Vec::new();
    for y in y1..y2 {
        for x in x1..x2 {
            let p = image.get_pixel(x, y);
            if p[3] > 16 {
                pixels.push([p[0], p[1], p[2]]);
            }
        }
    }
    if pixels.is_empty() {
        return ([5, 5, 5], [244, 240, 232]);
    }
    let background = median_color(&pixels);
    let bg_luma = luma(background);
    let mut text_pixels = pixels
        .iter()
        .copied()
        .filter(|rgb| {
            let delta = (luma(*rgb) - bg_luma).abs();
            delta > 42.0
                && if bg_luma < 128.0 {
                    luma(*rgb) > bg_luma
                } else {
                    luma(*rgb) < bg_luma
                }
        })
        .collect::<Vec<_>>();
    if text_pixels.len() < 6 {
        text_pixels = if bg_luma < 128.0 {
            pixels
                .iter()
                .copied()
                .filter(|rgb| luma(*rgb) > 120.0)
                .collect()
        } else {
            pixels
                .iter()
                .copied()
                .filter(|rgb| luma(*rgb) < 150.0)
                .collect()
        };
    }
    let foreground = if text_pixels.is_empty() {
        if bg_luma < 128.0 {
            [242, 238, 228]
        } else {
            [30, 34, 42]
        }
    } else {
        median_color(&text_pixels)
    };
    (background, foreground)
}

fn median_color(pixels: &[[u8; 3]]) -> [u8; 3] {
    let mut r = pixels.iter().map(|p| p[0]).collect::<Vec<_>>();
    let mut g = pixels.iter().map(|p| p[1]).collect::<Vec<_>>();
    let mut b = pixels.iter().map(|p| p[2]).collect::<Vec<_>>();
    r.sort_unstable();
    g.sort_unstable();
    b.sort_unstable();
    let mid = pixels.len() / 2;
    [r[mid], g[mid], b[mid]]
}

fn luma(rgb: [u8; 3]) -> f32 {
    0.299 * f32::from(rgb[0]) + 0.587 * f32::from(rgb[1]) + 0.114 * f32::from(rgb[2])
}

fn rgb_hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn estimate_source_font_size(rect: FloatRect, line_count: usize, configured_font_size: u32) -> u32 {
    let line_height = rect.height / line_count.max(1) as f32;
    let estimated = (line_height * 0.78).round() as u32;
    estimated.clamp(9, configured_font_size.clamp(12, 32).max(18))
}

fn fit_replacement_font_size(text: &str, rect: FloatRect, base_font_size: u32) -> u32 {
    let max_font = base_font_size.clamp(9, 32);
    for font_size in (9..=max_font).rev() {
        if replacement_text_fits(text, rect, font_size) {
            return font_size;
        }
    }
    9
}

fn replacement_text_fits(text: &str, rect: FloatRect, font_size: u32) -> bool {
    let char_width = (font_size as f32 * 0.92).max(1.0);
    let chars_per_row = (rect.width / char_width).floor().max(1.0) as usize;
    let rows = text
        .lines()
        .map(|line| {
            let count = line.chars().count().max(1);
            (count + chars_per_row - 1) / chars_per_row
        })
        .sum::<usize>()
        .max(1);
    rows as f32 * font_size as f32 * 1.28 <= rect.height.max(font_size as f32)
}

fn is_centered_title(
    rect: FloatRect,
    image_width: u32,
    image_height: u32,
    line_count: usize,
) -> bool {
    if line_count != 1 || rect.y > image_height as f32 * 0.18 {
        return false;
    }
    let center = rect.x + rect.width / 2.0;
    (center - image_width as f32 / 2.0).abs() < image_width as f32 * 0.18
}

fn ocr_translation_blocks(raw_text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut previous_line_ended_sentence = false;

    for raw_line in raw_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            flush_translation_paragraph(&mut blocks, &mut paragraph);
            previous_line_ended_sentence = false;
            continue;
        }

        if is_ocr_heading(line) {
            flush_translation_paragraph(&mut blocks, &mut paragraph);
            blocks.push(line.to_string());
            previous_line_ended_sentence = false;
            continue;
        }

        if previous_line_ended_sentence && starts_like_new_sentence(line) {
            flush_translation_paragraph(&mut blocks, &mut paragraph);
        }

        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(line);
        previous_line_ended_sentence = ends_sentence(line);
    }

    flush_translation_paragraph(&mut blocks, &mut paragraph);
    blocks
}

fn flush_translation_paragraph(blocks: &mut Vec<String>, paragraph: &mut String) {
    let text = paragraph.trim();
    if !text.is_empty() {
        blocks.push(text.to_string());
    }
    paragraph.clear();
}

fn is_ocr_heading(line: &str) -> bool {
    line.starts_with('[')
        && line.ends_with(']')
        && line.chars().count() <= 48
        && line.matches('[').count() == 1
        && line.matches(']').count() == 1
}

fn starts_like_new_sentence(line: &str) -> bool {
    let mut chars = line.chars().filter(|ch| !ch.is_whitespace());
    matches!(chars.next(), Some(ch) if ch.is_ascii_uppercase() || ch == '[')
}

fn ends_sentence(line: &str) -> bool {
    let trimmed = line.trim_end();
    trimmed.ends_with('.')
        || trimmed.ends_with('!')
        || trimmed.ends_with('?')
        || trimmed.ends_with('。')
        || trimmed.ends_with('！')
        || trimmed.ends_with('？')
}

fn show_overlay(
    app: &tauri::AppHandle,
    cfg: &AppConfig,
    anchor: Point,
    raw_text: String,
    text: String,
    image_rect: Option<Rect>,
    source_image_png: Option<&[u8]>,
    image_blocks: Vec<ImageReplacementBlock>,
    log_entry_id: Option<String>,
) -> anyhow::Result<()> {
    cleanup_selection_layers(app);
    hide_status_overlay(app);
    let display_raw_text = ocr_display_text(&raw_text);
    let has_source = cfg.overlay.show_source && !display_raw_text.trim().is_empty();
    let display_text = if has_source {
        format!("{display_raw_text}\n\n{text}")
    } else {
        text.clone()
    };
    let result_mode = if cfg.overlay.result_mode == "image_replace"
        && image_rect.is_some()
        && source_image_png.is_some()
        && !image_blocks.is_empty()
    {
        "image_replace"
    } else {
        "text_overlay"
    };
    let source_image_data_url = if result_mode == "image_replace" {
        source_image_png.map(|png| {
            format!(
                "data:image/png;base64,{}",
                general_purpose::STANDARD.encode(png)
            )
        })
    } else {
        None
    };
    let image_rect = image_rect.map(|rect| rect.normalized());
    let metrics = estimate_overlay_size(&display_text, cfg.overlay.width, cfg.overlay.font_size);
    let (mut width, mut height) = if result_mode == "image_replace" {
        let rect = image_rect.expect("image_replace requires image_rect");
        (rect.width.max(80) as u32, rect.height.max(36) as u32)
    } else {
        metrics
    };
    let mut max_height = cfg.overlay.max_height;

    let mut x = image_rect
        .filter(|_| result_mode == "image_replace")
        .map(|rect| rect.x)
        .unwrap_or(anchor.x + cfg.overlay.offset_x);
    let mut y = image_rect
        .filter(|_| result_mode == "image_replace")
        .map(|rect| rect.y)
        .unwrap_or(anchor.y + cfg.overlay.offset_y);
    if result_mode != "image_replace" {
        clamp_overlay_to_primary_monitor(app, cfg, &mut x, &mut y, &mut width, &mut height)?;
        max_height = max_height.min(height.max(54));
    }

    let window = if let Some(window) = app.get_webview_window("overlay") {
        window
    } else {
        WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
            .title("OCR 翻译")
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .focusable(true)
            .resizable(true)
            .skip_taskbar(true)
            .visible(false)
            .inner_size(width as f64, height as f64)
            .build()?
    };
    let payload = OverlayPayload {
        result_mode: result_mode.to_string(),
        text,
        raw_text: display_raw_text,
        width,
        image_width: width,
        image_height: height,
        source_image_data_url,
        image_blocks: if result_mode == "image_replace" {
            image_blocks
        } else {
            Vec::new()
        },
        opacity: cfg.overlay.opacity,
        font_size: cfg.overlay.font_size,
        max_height,
        source_background: cfg.overlay.source_background.clone(),
        translation_background: cfg.overlay.translation_background.clone(),
        double_click_close: cfg.overlay.double_click_close,
        show_source: cfg.overlay.show_source,
        draggable: cfg.overlay.draggable,
        log_entry_id,
    };
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut last_overlay) = state.last_overlay.lock() {
            *last_overlay = Some(payload.clone());
        }
    }
    window.set_size(PhysicalSize::new(width, height))?;
    let min_height = if result_mode == "image_replace" {
        height.max(36)
    } else if has_source {
        118
    } else {
        54
    };
    let min_width = if result_mode == "image_replace" {
        width.max(80)
    } else {
        180
    };
    let _ = window.set_min_size(Some(PhysicalSize::new(min_width, min_height)));
    window.set_position(PhysicalPosition::new(x, y))?;
    let _ = window.set_focusable(true);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_always_on_top(true);
    window.show()?;
    window.emit("overlay-show", payload)?;
    Ok(())
}

fn refresh_overlay_settings(app: &tauri::AppHandle, cfg: &AppConfig) {
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    let Ok(true) = window.is_visible() else {
        return;
    };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(mut guard) = state.last_overlay.lock() else {
        return;
    };
    let Some(payload) = guard.as_mut() else {
        return;
    };
    payload.result_mode = cfg.overlay.result_mode.clone();
    payload.opacity = cfg.overlay.opacity;
    payload.font_size = cfg.overlay.font_size;
    payload.max_height = cfg.overlay.max_height;
    payload.source_background = cfg.overlay.source_background.clone();
    payload.translation_background = cfg.overlay.translation_background.clone();
    payload.double_click_close = cfg.overlay.double_click_close;
    payload.show_source = cfg.overlay.show_source;
    payload.draggable = cfg.overlay.draggable;
    payload.width = cfg.overlay.width;
    let _ = window.emit("overlay-show", payload.clone());
}

fn estimate_overlay_size(text: &str, default_width: u32, font_size: u32) -> (u32, u32) {
    let font_size = font_size.clamp(12, 48);
    let width = default_width.clamp(180, 900);
    let horizontal_padding = 28;
    let vertical_padding = 24;
    let char_width = ((font_size as f32) * 0.62).ceil() as u32;
    let line_height = ((font_size as f32) * 1.45).ceil() as u32;
    let lines: Vec<&str> = if text.trim().is_empty() {
        vec!["无翻译结果"]
    } else {
        text.lines().collect()
    };
    let chars_per_row = ((width.saturating_sub(horizontal_padding)) / char_width).max(1);
    let rows: u32 = lines
        .iter()
        .map(|line| ((line.chars().count() as u32).max(1) + chars_per_row - 1) / chars_per_row)
        .sum::<u32>()
        .max(1);
    let height = rows
        .saturating_mul(line_height)
        .saturating_add(vertical_padding)
        .clamp(54, 620);
    (width, height)
}

fn ocr_display_text(raw_text: &str) -> String {
    ocr_translation_blocks(raw_text).join("\n\n")
}

fn input_matches_hotkey(hotkey: &str, event: &GlobalInputEvent) -> bool {
    let hotkey = hotkey.trim();
    if hotkey.is_empty() {
        return false;
    }

    match event {
        GlobalInputEvent::Mouse(mouse) => match mouse.button {
            MouseButton::Right => {
                hotkey.eq_ignore_ascii_case("MouseRight")
                    || hotkey.eq_ignore_ascii_case("RightButton")
            }
            MouseButton::X1 => {
                hotkey.eq_ignore_ascii_case("MouseX1") || hotkey.eq_ignore_ascii_case("XButton1")
            }
            MouseButton::X2 => {
                hotkey.eq_ignore_ascii_case("MouseX2") || hotkey.eq_ignore_ascii_case("XButton2")
            }
            MouseButton::Left => hotkey.eq_ignore_ascii_case("MouseLeft"),
        },
        GlobalInputEvent::Keyboard(keyboard) => keyboard_matches_hotkey(hotkey, *keyboard),
    }
}

fn keyboard_matches_hotkey(hotkey: &str, event: KeyboardEvent) -> bool {
    let mut want_ctrl = false;
    let mut want_alt = false;
    let mut want_shift = false;
    let mut key = None;

    for part in hotkey.split('+') {
        let part = part.trim();
        if part.eq_ignore_ascii_case("ctrl") || part.eq_ignore_ascii_case("control") {
            want_ctrl = true;
        } else if part.eq_ignore_ascii_case("alt") {
            want_alt = true;
        } else if part.eq_ignore_ascii_case("shift") {
            want_shift = true;
        } else {
            key = key_name_to_vk(part);
        }
    }

    key == Some(event.vk_code)
        && event.ctrl == want_ctrl
        && event.alt == want_alt
        && event.shift == want_shift
}

fn key_name_to_vk(key: &str) -> Option<u32> {
    let upper = key.trim().to_ascii_uppercase();
    if upper.len() == 1 {
        let ch = upper.as_bytes()[0];
        if ch.is_ascii_alphanumeric() {
            return Some(ch as u32);
        }
    }
    if let Some(number) = upper.strip_prefix('F').and_then(|s| s.parse::<u32>().ok()) {
        if (1..=24).contains(&number) {
            return Some(0x70 + number - 1);
        }
    }
    match upper.as_str() {
        "SPACE" => Some(0x20),
        "TAB" => Some(0x09),
        "ESC" | "ESCAPE" => Some(0x1B),
        "ENTER" | "RETURN" => Some(0x0D),
        "INSERT" | "INS" => Some(0x2D),
        "DELETE" | "DEL" => Some(0x2E),
        "HOME" => Some(0x24),
        "END" => Some(0x23),
        "PAGEUP" | "PGUP" => Some(0x21),
        "PAGEDOWN" | "PGDN" => Some(0x22),
        "LEFT" => Some(0x25),
        "UP" => Some(0x26),
        "RIGHT" => Some(0x27),
        "DOWN" => Some(0x28),
        _ => None,
    }
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let open_item = MenuItemBuilder::with_id("tray_open", "打开").build(app)?;
    let exit_item = MenuItemBuilder::with_id("tray_exit", "退出").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&open_item)
        .item(&exit_item)
        .build()?;
    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("OCR Translator")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray_open" => show_main_window(app),
            "tray_exit" => {
                let state = app.state::<AppState>();
                exit_application(app, &state);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: TrayMouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn sync_autostart_setting(app: &tauri::AppHandle, cfg: &AppConfig) -> anyhow::Result<()> {
    let manager = app.autolaunch();
    let enabled = manager.is_enabled().unwrap_or(false);
    match (cfg.app.launch_at_startup, enabled) {
        (true, false) => manager.enable()?,
        (false, true) => manager.disable()?,
        _ => {}
    }
    Ok(())
}

fn launched_from_autostart() -> bool {
    std::env::args().any(|arg| arg == "--from-autostart")
}

fn maybe_relaunch_as_admin(cfg: &AppConfig) {
    if !cfg.app.auto_elevate || is_running_as_admin() {
        return;
    }
    if relaunch_as_admin().is_ok() {
        std::process::exit(0);
    }
}

fn is_running_as_admin() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

fn relaunch_as_admin() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let exe = wide_null(exe.as_os_str());
    let result = unsafe {
        ShellExecuteW(
            HWND(std::ptr::null_mut()),
            w!("runas"),
            PCWSTR(exe.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        anyhow::bail!("请求管理员权限被取消或失败");
    }
    Ok(())
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn main() {
    let mut config = AppConfig::load().unwrap_or_default();
    config.normalize();
    maybe_relaunch_as_admin(&config);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
            let _ = app.emit("ocr-status", "程序已经在运行，已打开现有窗口。");
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("OCR Translator")
                .arg("--from-autostart")
                .build(),
        )
        .manage(AppState {
            config: Mutex::new(config),
            last_overlay: Mutex::new(None),
            selection_active: AtomicBool::new(false),
            selection_cancel: AtomicBool::new(false),
            exiting: AtomicBool::new(false),
        })
        .setup(|app| {
            setup_tray(app)?;
            {
                let state = app.state::<AppState>();
                let cfg = state.config.lock().map(|cfg| cfg.clone());
                if let Ok(cfg) = cfg {
                    if let Err(err) = sync_autostart_setting(app.handle(), &cfg) {
                        let _ = app.emit("ocr-status", format!("同步开机自启动失败：{err}"));
                    }
                }
            }
            if let Ok(resource_dir) = app.path().resource_dir() {
                let bundled_oneocr = resource_dir.join("SnippingTool");
                if bundled_oneocr.is_dir() {
                    std::env::set_var("OCR_TRANSLATOR_ONEOCR_DIR", bundled_oneocr);
                }
            }
            if launched_from_autostart() {
                let _ = hide_main_window(app.handle());
            }
            let (tx, mut rx) = mpsc::unbounded_channel();
            match app_windows::GlobalInputHook::start(tx) {
                Ok(hook) => {
                    app.manage(Mutex::new(Some(hook)));
                }
                Err(err) => {
                    let _ = app.emit("ocr-status", format!("全局输入监听启动失败：{err}"));
                }
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut last_trigger = Instant::now() - Duration::from_secs(10);
                while let Some(event) = rx.recv().await {
                    let state = handle.state::<AppState>();
                    if matches!(
                        event,
                        GlobalInputEvent::Mouse(app_windows::MouseEvent {
                            button: MouseButton::Right,
                            ..
                        })
                    ) && state.selection_active.load(Ordering::SeqCst)
                    {
                        state.selection_cancel.store(true, Ordering::SeqCst);
                        continue;
                    }
                    let cfg = state.config.lock().map(|cfg| cfg.clone());
                    let Ok(cfg) = cfg else {
                        continue;
                    };
                    if input_matches_hotkey(&cfg.hotkey, &event)
                        && last_trigger.elapsed() >= Duration::from_millis(500)
                    {
                        last_trigger = Instant::now();
                        let _ = handle.emit("ocr-hotkey", event);
                        let _ = start_selection_window(&handle, &cfg);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    maybe_handle_main_close(window.app_handle(), api);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            handle_close_choice,
            exit_app,
            get_admin_status,
            list_providers,
            list_ocr_languages,
            list_ocr_engines,
            preview_oneocr_runtime,
            install_oneocr_runtime,
            manual_translate,
            run_ocr_once,
            selection_done,
            selection_auto_detect,
            selection_cancel,
            close_overlay,
            start_overlay_drag,
            start_overlay_resize_corner,
            resize_overlay_to_content,
            get_overlay_payload,
            save_translation_log_render,
            get_cursor_position
        ])
        .run(tauri::generate_context!())
        .expect("OCR Translator 启动失败");
}
