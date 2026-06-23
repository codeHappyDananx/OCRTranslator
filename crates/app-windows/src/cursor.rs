use crate::Point;
use anyhow::{bail, Result};
use windows::Win32::{
    Foundation::POINT,
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON},
        WindowsAndMessaging::{ClipCursor, GetCursorPos, ShowCursor},
    },
};

pub fn cursor_position() -> Result<Point> {
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point)?;
    }
    Ok(Point {
        x: point.x,
        y: point.y,
    })
}

pub fn release_cursor_lock() -> Result<()> {
    unsafe {
        ClipCursor(None)?;
        for _ in 0..8 {
            let count = ShowCursor(true);
            if count >= 0 {
                return Ok(());
            }
        }
    }
    bail!("释放鼠标显示状态失败")
}

pub fn left_mouse_down() -> bool {
    unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 }
}

pub fn right_mouse_down() -> bool {
    unsafe { (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0 }
}
