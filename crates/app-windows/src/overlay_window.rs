use anyhow::{bail, Result};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{
        Input::KeyboardAndMouse::ReleaseCapture,
        WindowsAndMessaging::{SendMessageW, HTBOTTOMRIGHT, WM_NCLBUTTONDOWN},
    },
};

#[derive(Debug, Clone, Copy)]
pub enum NativeResizeDirection {
    SouthEast,
}

pub fn start_native_window_resize(hwnd_raw: isize, direction: NativeResizeDirection) -> Result<()> {
    if hwnd_raw == 0 {
        bail!("窗口句柄无效");
    }
    let hit_test = match direction {
        NativeResizeDirection::SouthEast => HTBOTTOMRIGHT,
    };
    unsafe {
        let _ = ReleaseCapture();
        let _ = SendMessageW(
            HWND(hwnd_raw as _),
            WM_NCLBUTTONDOWN,
            WPARAM(hit_test as usize),
            LPARAM(0),
        );
    }
    Ok(())
}
