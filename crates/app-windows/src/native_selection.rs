use crate::{Point, Rect};
use anyhow::{bail, Context, Result};
use image::RgbaImage;
use std::{
    ffi::c_void,
    sync::mpsc::{self, Sender},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreatePen, DeleteObject, EndPaint, GetStockObject, InvalidateRect,
            Rectangle, SelectObject, SetDIBitsToDevice, UpdateWindow, BITMAPINFO, BITMAPINFOHEADER,
            BI_RGB, DIB_RGB_COLORS, HGDIOBJ, HOLLOW_BRUSH, PAINTSTRUCT, PS_SOLID,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus},
        UI::WindowsAndMessaging::{
            BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
            FindWindowExW, GetClientRect, GetMessageW, LoadCursorW, PostMessageW, PostQuitMessage,
            RegisterClassW, SetForegroundWindow, SetWindowPos, ShowWindow, TranslateMessage,
            CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOPMOST, IDC_CROSS, MSG, SWP_NOMOVE,
            SWP_NOSIZE, SW_SHOW, WM_CLOSE, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MOUSEMOVE, WM_NCCREATE, WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST, WS_POPUP,
        },
    },
};

const CLASS_NAME: PCWSTR = w!("OCRTranslatorNativeSelectionWindow");
const DIM_ALPHA: f32 = 0.28;
const BORDER_WIDTH: i32 = 2;

pub fn select_rect_native(screen_rect: Rect, frozen_png: &[u8]) -> Result<Option<Rect>> {
    let image = image::load_from_memory(frozen_png)
        .context("读取冻结截图失败")?
        .to_rgba8();
    if image.width() as i32 != screen_rect.width || image.height() as i32 != screen_rect.height {
        bail!("冻结截图尺寸和屏幕尺寸不一致");
    }
    let original_bgra = rgba_to_bgra(&image);
    let dim_bgra = dim_bgra(&original_bgra, DIM_ALPHA);
    let (tx, rx) = mpsc::channel();
    let mut state = Box::new(SelectionState {
        screen_rect,
        width: screen_rect.width,
        height: screen_rect.height,
        original_bgra,
        dim_bgra,
        selecting: false,
        start: Point { x: 0, y: 0 },
        end: Point { x: 0, y: 0 },
        sender: Some(tx),
    });

    unsafe {
        let instance = GetModuleHandleW(None)?.into();
        register_selection_class(instance)?;
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!("OCR 选区"),
            WS_POPUP,
            screen_rect.x,
            screen_rect.y,
            screen_rect.width,
            screen_rect.height,
            None,
            None,
            instance,
            Some(state.as_mut() as *mut SelectionState as *const _),
        )?;
        let _state_owner = Box::into_raw(state);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
        let _ = UpdateWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(rx.try_recv().unwrap_or(None))
}

pub fn close_native_selection_windows() {
    unsafe {
        let mut hwnd = HWND::default();
        loop {
            let Ok(found) = FindWindowExW(None, hwnd, CLASS_NAME, None) else {
                break;
            };
            hwnd = found;
            if hwnd.0.is_null() {
                break;
            }
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe fn register_selection_class(instance: HINSTANCE) -> Result<()> {
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(selection_wnd_proc),
        hInstance: instance,
        hCursor: LoadCursorW(None, IDC_CROSS)?,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    let atom = RegisterClassW(&class);
    if atom == 0 {
        // RegisterClassW also returns 0 when the class already exists. Creating
        // the window below is the real validation path.
        return Ok(());
    }
    Ok(())
}

struct SelectionState {
    screen_rect: Rect,
    width: i32,
    height: i32,
    original_bgra: Vec<u8>,
    dim_bgra: Vec<u8>,
    selecting: bool,
    start: Point,
    end: Point,
    sender: Option<Sender<Option<Rect>>>,
}

fn send_result(state: &mut SelectionState, result: Option<Rect>) {
    if let Some(sender) = state.sender.take() {
        let _ = sender.send(result);
    }
}

unsafe extern "system" fn selection_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let createstruct =
            lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
        let state = (*createstruct).lpCreateParams as *mut SelectionState;
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            state as isize,
        );
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let state_ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
        as *mut SelectionState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;

    match msg {
        WM_PAINT => {
            paint_selection(hwnd, state);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            state.start = lparam_point(lparam);
            state.end = state.start;
            state.selecting = true;
            let _ = SetCapture(hwnd);
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if state.selecting {
                state.end = lparam_point(lparam);
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if state.selecting {
                state.end = lparam_point(lparam);
                state.selecting = false;
                let _ = ReleaseCapture();
                let rect = rect_from_client_points(state.screen_rect, state.start, state.end);
                if rect.width >= 2 && rect.height >= 2 {
                    send_result(state, Some(rect));
                } else {
                    send_result(state, None);
                }
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            send_result(state, None);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B {
                send_result(state, None);
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            send_result(state, None);
            let _ = Box::from_raw(state_ptr);
            windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_selection(hwnd: HWND, state: &SelectionState) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let mut frame = state.dim_bgra.clone();

    if state.selecting {
        let rect = client_rect_from_points(state.start, state.end, state.width, state.height);
        if rect.width > 0 && rect.height > 0 {
            copy_bgra_region(&state.original_bgra, &mut frame, state.width, rect);
            draw_bgra(hdc, state.width, state.height, &frame);
            let pen = CreatePen(PS_SOLID, BORDER_WIDTH, COLORREF(0x00ff9d39));
            let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
            let old_brush = SelectObject(hdc, GetStockObject(HOLLOW_BRUSH));
            let _ = Rectangle(
                hdc,
                rect.x,
                rect.y,
                rect.x + rect.width,
                rect.y + rect.height,
            );
            let _ = SelectObject(hdc, old_pen);
            let _ = SelectObject(hdc, old_brush);
            let _ = DeleteObject(HGDIOBJ(pen.0));
        } else {
            draw_bgra(hdc, state.width, state.height, &frame);
        }
    } else {
        draw_bgra(hdc, state.width, state.height, &frame);
    }
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn draw_bgra(hdc: windows::Win32::Graphics::Gdi::HDC, width: i32, height: i32, bgra: &[u8]) {
    let bmi = bitmap_info(width, height);
    let _ = SetDIBitsToDevice(
        hdc,
        0,
        0,
        width as u32,
        height as u32,
        0,
        0,
        0,
        height as u32,
        bgra.as_ptr() as *const c_void,
        &bmi,
        DIB_RGB_COLORS,
    );
}

fn bitmap_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn copy_bgra_region(source: &[u8], target: &mut [u8], image_width: i32, rect: Rect) {
    let image_width = image_width.max(1) as usize;
    for y in rect.y.max(0)..(rect.y + rect.height).max(rect.y) {
        let start = (y as usize * image_width + rect.x.max(0) as usize) * 4;
        let len = rect.width.max(0) as usize * 4;
        let end = start.saturating_add(len);
        if end <= source.len() && end <= target.len() {
            target[start..end].copy_from_slice(&source[start..end]);
        }
    }
}

fn rgba_to_bgra(image: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(image.as_raw().len());
    for pixel in image.as_raw().chunks_exact(4) {
        out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0]);
    }
    out
}

fn dim_bgra(bgra: &[u8], alpha: f32) -> Vec<u8> {
    let factor = (1.0 - alpha).clamp(0.0, 1.0);
    bgra.chunks_exact(4)
        .flat_map(|px| {
            [
                (px[0] as f32 * factor) as u8,
                (px[1] as f32 * factor) as u8,
                (px[2] as f32 * factor) as u8,
                0,
            ]
        })
        .collect()
}

fn lparam_point(lparam: LPARAM) -> Point {
    let raw = lparam.0 as u32;
    Point {
        x: (raw & 0xffff) as i16 as i32,
        y: ((raw >> 16) & 0xffff) as i16 as i32,
    }
}

fn rect_from_client_points(screen_rect: Rect, start: Point, end: Point) -> Rect {
    let rect = client_rect_from_points(start, end, screen_rect.width, screen_rect.height);
    Rect {
        x: screen_rect.x + rect.x,
        y: screen_rect.y + rect.y,
        width: rect.width,
        height: rect.height,
    }
}

fn client_rect_from_points(start: Point, end: Point, width: i32, height: i32) -> Rect {
    let left = start.x.min(end.x).clamp(0, width);
    let top = start.y.min(end.y).clamp(0, height);
    let right = start.x.max(end.x).clamp(0, width);
    let bottom = start.y.max(end.y).clamp(0, height);
    Rect {
        x: left,
        y: top,
        width: (right - left).max(0),
        height: (bottom - top).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn client_rect_is_clamped_and_normalized() {
        let rect =
            client_rect_from_points(Point { x: 120, y: 80 }, Point { x: -10, y: 220 }, 200, 160);
        assert_eq!(
            rect,
            Rect {
                x: 0,
                y: 80,
                width: 120,
                height: 80
            }
        );
    }

    #[test]
    fn client_rect_converts_to_absolute_screen_rect() {
        let rect = rect_from_client_points(
            Rect {
                x: -100,
                y: 50,
                width: 800,
                height: 600,
            },
            Point { x: 10, y: 20 },
            Point { x: 110, y: 220 },
        );
        assert_eq!(
            rect,
            Rect {
                x: -90,
                y: 70,
                width: 100,
                height: 200
            }
        );
    }

    #[test]
    fn bgra_conversion_and_dim_are_stable() {
        let mut image = RgbaImage::new(1, 1);
        image.put_pixel(0, 0, Rgba([100, 150, 200, 255]));
        let bgra = rgba_to_bgra(&image);
        assert_eq!(bgra, vec![200, 150, 100, 0]);
        let dimmed = dim_bgra(&bgra, 0.25);
        assert_eq!(dimmed, vec![150, 112, 75, 0]);
    }
}
