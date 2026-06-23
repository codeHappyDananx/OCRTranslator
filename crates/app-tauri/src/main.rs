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
    OcrPipelineRequest, OcrTextLine, OneOcrPackageInfo, Point, Rect,
};
use base64::{engine::general_purpose, Engine as _};
use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    io::Cursor,
    os::windows::ffi::OsStrExt,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OverlayResizeRequest {
    width: u32,
    height: u32,
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

    if let Some(monitor) = app.primary_monitor().map_err(|e| e.to_string())? {
        let pos = monitor.position();
        let size = monitor.size();
        let margin = cfg.overlay.screen_margin;
        let left = pos.x + margin;
        let top = pos.y + margin;
        let right = pos.x + size.width as i32 - margin;
        let bottom = pos.y + size.height as i32 - margin;
        let max_width = (right - left).max(160) as u32;
        let max_height = (bottom - top).max(54) as u32;
        width = width.min(max_width);
        height = height.min(max_height);
        if x + width as i32 > right {
            x = (right - width as i32).max(left);
        }
        if y + height as i32 > bottom {
            y = (bottom - height as i32).max(top);
        }
        x = x.max(left);
        y = y.max(top);
    }

    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
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
fn get_cursor_position() -> Result<Point, String> {
    cursor_position().map_err(|e| e.to_string())
}

fn start_selection_window(app: &tauri::AppHandle, _cfg: &AppConfig) -> anyhow::Result<()> {
    let _ = release_cursor_lock();
    cleanup_selection_layers(app);
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

async fn run_pipeline(
    app: tauri::AppHandle,
    cfg: AppConfig,
    payload: SelectionPayload,
    frozen_screen: Option<FrozenScreen>,
) -> anyhow::Result<()> {
    cleanup_selection_layers(&app);
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
            show_user_message(&app, &cfg, payload.anchor, "截图没有成功，请重新试一次。")?;
            return Ok(());
        }
    };
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
    show_overlay(
        &app,
        &cfg,
        payload.anchor,
        raw_text,
        translated,
        Some(capture_rect),
        Some(&png),
        image_blocks,
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
    background: [u8; 3],
    foreground: [u8; 3],
}

#[derive(Debug, Clone)]
struct VisualTextGroup {
    text: String,
    rect: FloatRect,
    line_count: usize,
}

async fn build_image_replacement_blocks(
    cfg: &AppConfig,
    settings: &std::collections::HashMap<String, String>,
    png: &[u8],
    lines: &[OcrTextLine],
) -> anyhow::Result<Vec<ImageReplacementBlock>> {
    let image = image::load_from_memory(png)?.to_rgba8();
    let (image_width, image_height) = image.dimensions();
    let groups = group_ocr_lines(&image, lines, image_width, image_height);
    let mut blocks = Vec::with_capacity(groups.len());

    for group in groups {
        let translated = translate_preserving_lines(cfg, settings, &group.text)
            .await
            .unwrap_or_else(|_| group.text.clone());
        let (background, color) = sample_replacement_colors(&image, group.rect);
        let base_font =
            estimate_source_font_size(group.rect, group.line_count, cfg.overlay.font_size);
        let font_size = fit_replacement_font_size(&translated, group.rect, base_font);
        let align = if is_centered_title(group.rect, image_width, image_height, group.line_count) {
            "center"
        } else {
            "left"
        };

        blocks.push(ImageReplacementBlock {
            source_text: group.text,
            translated_text: translated.trim().to_string(),
            x: group.rect.x,
            y: group.rect.y,
            width: group.rect.width,
            height: group.rect.height,
            font_size,
            background,
            color,
            align: align.to_string(),
        });
    }

    Ok(blocks)
}

fn group_ocr_lines(
    image: &RgbaImage,
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
            ocr_bbox_to_rect(&line.bbox, image_width, image_height).map(|rect| {
                let (background, foreground) = sample_replacement_rgb(image, rect);
                VisualTextLine {
                    text: text.to_string(),
                    rect,
                    background,
                    foreground,
                }
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
    let mut current = Vec::new();
    for line in visual_lines {
        let should_split = current
            .last()
            .map(|previous: &VisualTextLine| {
                let vertical_gap = line.rect.y - previous.rect.bottom();
                let overlaps_previous_row =
                    line.rect.y < previous.rect.bottom() - median_height * 0.35;
                vertical_gap > gap_threshold
                    || overlaps_previous_row
                    || (line.rect.x - previous.rect.x).abs() > x_threshold
                    || !similar_line_style(previous, &line)
            })
            .unwrap_or(false);
        if should_split {
            groups.push(std::mem::take(&mut current));
        }
        current.push(line);
    }
    if !current.is_empty() {
        groups.push(current);
    }

    groups
        .into_iter()
        .filter_map(|group| {
            let mut iter = group.into_iter();
            let first = iter.next()?;
            let mut rect = first.rect;
            let mut texts = vec![first.text];
            let mut line_count = 1usize;
            for line in iter {
                rect = rect.union(line.rect);
                texts.push(line.text);
                line_count += 1;
            }
            let mut rect = rect.padded(3.0, 2.0, image_width, image_height);
            if line_count > 1 {
                let right_padding = 24.0;
                rect.width = rect
                    .width
                    .max((image_width as f32 - rect.x - right_padding).max(rect.width))
                    .min(image_width as f32 - rect.x);
            }
            Some(VisualTextGroup {
                text: texts.join("\n"),
                rect,
                line_count,
            })
        })
        .collect()
}

fn similar_line_style(a: &VisualTextLine, b: &VisualTextLine) -> bool {
    let height_ratio = a.rect.height.min(b.rect.height) / a.rect.height.max(b.rect.height).max(1.0);
    height_ratio > 0.72
        && color_distance(a.foreground, b.foreground) < 72.0
        && color_distance(a.background, b.background) < 48.0
}

fn color_distance(a: [u8; 3], b: [u8; 3]) -> f32 {
    let dr = f32::from(a[0]) - f32::from(b[0]);
    let dg = f32::from(a[1]) - f32::from(b[1]);
    let db = f32::from(a[2]) - f32::from(b[2]);
    (dr * dr + dg * dg + db * db).sqrt()
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
) -> anyhow::Result<()> {
    cleanup_selection_layers(app);
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
    let (width, height) = if result_mode == "image_replace" {
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
    if let Some(monitor) = app.primary_monitor()? {
        let pos = monitor.position();
        let size = monitor.size();
        let margin = cfg.overlay.screen_margin;
        let left = pos.x + margin;
        let top = pos.y + margin;
        let right = pos.x + size.width as i32 - margin;
        let bottom = pos.y + size.height as i32 - margin;
        max_height = max_height.min((bottom - top).max(54) as u32);
        if x + width as i32 > right {
            x = (right - width as i32).max(left);
        }
        if y + height as i32 > bottom {
            y = (bottom - height as i32).max(top);
        }
        x = x.max(left);
        y = y.max(top);
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
        .manage(AppState {
            config: Mutex::new(config),
            last_overlay: Mutex::new(None),
            selection_active: AtomicBool::new(false),
            selection_cancel: AtomicBool::new(false),
            exiting: AtomicBool::new(false),
        })
        .setup(|app| {
            setup_tray(app)?;
            if let Ok(resource_dir) = app.path().resource_dir() {
                let bundled_oneocr = resource_dir.join("SnippingTool");
                if bundled_oneocr.is_dir() {
                    std::env::set_var("OCR_TRANSLATOR_ONEOCR_DIR", bundled_oneocr);
                }
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
            get_cursor_position
        ])
        .run(tauri::generate_context!())
        .expect("OCR Translator 启动失败");
}
