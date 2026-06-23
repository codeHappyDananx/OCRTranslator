#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use app_core::{
    provider_catalog, translate, AppConfig, ProviderInfo, TranslationRequest, TranslationResponse,
};
use app_windows::{
    available_windows_ocr_languages, capture_rect_png, cursor_position, detect_ocr_engines,
    install_snippingtool_oneocr_runtime, preview_snippingtool_oneocr_package,
    recognize_png_pipeline, release_cursor_lock, GlobalInputEvent, KeyboardEvent, MouseButton,
    OcrEngineStatus, OcrLanguageInfo, OcrPipelineRequest, OneOcrPackageInfo, Point, Rect,
};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{
    Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder,
};
use tokio::sync::mpsc;

struct AppState {
    config: Mutex<AppConfig>,
    last_overlay: Mutex<Option<OverlayPayload>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManualTranslateRequest {
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OcrDiagnosticResponse {
    engine: String,
    image_path: String,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SelectionPayload {
    rect: Rect,
    anchor: Point,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OverlayPayload {
    text: String,
    raw_text: String,
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
fn save_config(mut config: AppConfig, state: State<'_, AppState>) -> Result<(), String> {
    config.normalize();
    config.save().map_err(|e| e.to_string())?;
    let mut guard = state
        .config
        .lock()
        .map_err(|e| format!("写入配置锁失败：{e}"))?;
    *guard = config;
    Ok(())
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
async fn diagnose_last_capture(
    state: State<'_, AppState>,
) -> Result<OcrDiagnosticResponse, String> {
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("读取配置锁失败：{e}"))?
        .clone();
    let dir = app_core::config_dir().map_err(|e| e.to_string())?;
    let path = dir.join("last_capture.png");
    if !path.exists() {
        return Err(format!(
            "还没有上一张 OCR 截图。请先在游戏里框选一次，或点击“进行一次 OCR”。\n预期路径：{}",
            path.display()
        ));
    }
    let png = std::fs::read(&path).map_err(|e| format!("读取上一张 OCR 截图失败：{e}"))?;
    let result = recognize_png_pipeline(
        &png,
        OcrPipelineRequest {
            engine: cfg.ocr_engine.clone(),
            source_lang: cfg.source_lang.clone(),
            save_preprocessed: true,
        },
    )
    .await
    .map_err(|e| format!("OCR 诊断失败：{e}"))?;
    Ok(OcrDiagnosticResponse {
        engine: result.engine,
        image_path: path.display().to_string(),
        text: result.text,
    })
}

#[tauri::command]
fn open_last_capture() -> Result<(), String> {
    let dir = app_core::config_dir().map_err(|e| e.to_string())?;
    let path = dir.join("last_capture.png");
    if !path.exists() {
        return Err(format!(
            "还没有上一张 OCR 截图。请先框选一次。\n预期路径：{}",
            path.display()
        ));
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map_err(|e| format!("打开截图失败：{e}"))?;
    Ok(())
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
fn show_test_overlay(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("读取配置锁失败：{e}"))?
        .clone();
    let anchor = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .map(|monitor| {
            let pos = monitor.position();
            let size = monitor.size();
            Point {
                x: pos.x + (size.width as i32 / 2),
                y: pos.y + (size.height as i32 / 2),
            }
        })
        .unwrap_or(Point { x: 240, y: 160 });
    show_overlay(
        &app,
        &cfg,
        anchor,
        "Overlay self test".to_string(),
        "浮窗测试：如果你能看到这个透明浮窗，说明浮窗窗口本身正常。".to_string(),
    )
    .map_err(|e| e.to_string())
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
    run_pipeline(app, cfg, payload)
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
    run_pipeline(app, cfg, SelectionPayload { rect, anchor })
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
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
    clear_overlay_payload(app);
    if let Some(window) = app.get_webview_window("selection") {
        window.show()?;
        let _ = window.emit("selection-reset", ());
        window.set_focus()?;
        return Ok(());
    }
    let (x, y, width, height) = if let Some(monitor) = app.primary_monitor()? {
        let pos = monitor.position();
        let size = monitor.size();
        (pos.x, pos.y, size.width, size.height)
    } else {
        (0, 0, 1920, 1080)
    };
    let window =
        WebviewWindowBuilder::new(app, "selection", WebviewUrl::App("selection.html".into()))
            .title("选择 OCR 区域")
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .position(x as f64, y as f64)
            .inner_size(width as f64, height as f64)
            .build()?;
    window.show()?;
    let _ = window.emit("selection-reset", ());
    window.set_focus()?;
    Ok(())
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
) -> anyhow::Result<()> {
    app.emit("ocr-status", "正在截图...")?;
    let selected_rect = payload.rect.normalized();
    let capture_rect = selected_rect;
    let png = match capture_rect_png(capture_rect) {
        Ok(png) => png,
        Err(err) => {
            let message = format!("截图失败：{err}");
            let _ = app.emit("ocr-status", &message);
            show_overlay(&app, &cfg, payload.anchor, String::new(), message)?;
            return Ok(());
        }
    };
    app.emit("ocr-status", "正在 OCR 识别...")?;
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
            let message = format!("OCR 失败：{err}");
            let _ = app.emit("ocr-status", "OCR 失败");
            show_overlay(&app, &cfg, payload.anchor, String::new(), message)?;
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
        let message =
            "未识别到文本。请框选更清晰的英文区域，或者确认 Windows OCR 语言包可用。".to_string();
        let _ = app.emit("ocr-status", &message);
        show_overlay(&app, &cfg, payload.anchor, String::new(), message)?;
        return Ok(());
    }
    app.emit("ocr-status", format!("OCR：{raw_text}"))?;

    let settings = cfg
        .provider_settings
        .get(&cfg.translator)
        .cloned()
        .unwrap_or_default();
    app.emit("ocr-status", "正在翻译...")?;
    let translated = translate_preserving_lines(&cfg, &settings, &raw_text)
        .await
        .unwrap_or_else(|err| format!("翻译失败：{err}\n\n原文：\n{raw_text}"));
    show_overlay(&app, &cfg, payload.anchor, raw_text, translated)?;
    app.emit("ocr-status", "完成")?;
    Ok(())
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
) -> anyhow::Result<()> {
    let display_text = if cfg.overlay.show_source && !raw_text.trim().is_empty() {
        format!("{raw_text}\n\n{text}")
    } else {
        text.clone()
    };
    let metrics = estimate_overlay_size(&display_text, cfg.overlay.width, cfg.overlay.font_size);
    let width = metrics.0;
    let height = metrics.1;
    let mut max_height = 620;

    let mut x = anchor.x + cfg.overlay.offset_x;
    let mut y = anchor.y + cfg.overlay.offset_y;
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
            .focusable(false)
            .resizable(true)
            .skip_taskbar(true)
            .visible(false)
            .inner_size(width as f64, height as f64)
            .build()?
    };
    let payload = OverlayPayload {
        text,
        raw_text,
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
    window.set_position(PhysicalPosition::new(x, y))?;
    let _ = window.set_focusable(false);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_always_on_top(true);
    window.show()?;
    window.emit("overlay-show", payload)?;
    Ok(())
}

fn estimate_overlay_size(text: &str, default_width: u32, font_size: u32) -> (u32, u32) {
    let font_size = font_size.clamp(12, 48);
    let max_width = default_width.clamp(180, 900);
    let min_width = 160;
    let horizontal_padding = 28;
    let vertical_padding = 24;
    let char_width = ((font_size as f32) * 0.62).ceil() as u32;
    let line_height = ((font_size as f32) * 1.45).ceil() as u32;
    let lines: Vec<&str> = if text.trim().is_empty() {
        vec!["无翻译结果"]
    } else {
        text.lines().collect()
    };
    let longest = lines
        .iter()
        .map(|line| line.chars().count() as u32)
        .max()
        .unwrap_or(1);
    let content_width = longest
        .saturating_mul(char_width)
        .saturating_add(horizontal_padding);
    let width = content_width.clamp(min_width, max_width);
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

fn input_matches_hotkey(hotkey: &str, event: &GlobalInputEvent) -> bool {
    let hotkey = hotkey.trim();
    if hotkey.is_empty() {
        return false;
    }

    match event {
        GlobalInputEvent::Mouse(mouse) => match mouse.button {
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

fn main() {
    let mut config = AppConfig::load().unwrap_or_default();
    config.normalize();
    tauri::Builder::default()
        .manage(AppState {
            config: Mutex::new(config),
            last_overlay: Mutex::new(None),
        })
        .setup(|app| {
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
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            list_providers,
            list_ocr_languages,
            list_ocr_engines,
            preview_oneocr_runtime,
            install_oneocr_runtime,
            manual_translate,
            diagnose_last_capture,
            open_last_capture,
            run_ocr_once,
            show_test_overlay,
            selection_done,
            selection_auto_detect,
            selection_cancel,
            close_overlay,
            start_overlay_drag,
            resize_overlay_to_content,
            get_overlay_payload,
            get_cursor_position
        ])
        .run(tauri::generate_context!())
        .expect("OCR Translator 启动失败");
}
