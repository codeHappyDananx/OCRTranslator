use crate::Rect;
use anyhow::{anyhow, bail, Context, Result};
use image::{ImageBuffer, ImageFormat, Rgba};
use std::{io::Cursor, ptr::null_mut};
use windows::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HBITMAP, HDC, HGDIOBJ, SRCCOPY,
    },
    UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    },
};

struct ScreenDc(HDC);
impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseDC(HWND(null_mut()), self.0);
        }
    }
}

struct MemoryDc(HDC);
impl Drop for MemoryDc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

struct Bitmap(HBITMAP);
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0 .0));
        }
    }
}

pub fn capture_rect_png(rect: Rect) -> Result<Vec<u8>> {
    let rect = clamp_rect_to_virtual_screen(rect.normalized());
    if rect.width <= 2 || rect.height <= 2 {
        bail!("选区太小，无法截图");
    }
    unsafe {
        let screen_dc = ScreenDc(GetDC(HWND(null_mut())));
        if screen_dc.0 .0.is_null() {
            bail!("获取屏幕 DC 失败");
        }
        let mem_dc = MemoryDc(CreateCompatibleDC(screen_dc.0));
        if mem_dc.0 .0.is_null() {
            bail!("创建内存 DC 失败");
        }
        let bitmap = Bitmap(CreateCompatibleBitmap(screen_dc.0, rect.width, rect.height));
        if bitmap.0 .0.is_null() {
            bail!("创建截图位图失败");
        }
        let old = SelectObject(mem_dc.0, HGDIOBJ(bitmap.0 .0));
        if old.0.is_null() {
            bail!("选择截图位图失败");
        }
        if BitBlt(
            mem_dc.0,
            0,
            0,
            rect.width,
            rect.height,
            screen_dc.0,
            rect.x,
            rect.y,
            SRCCOPY,
        )
        .is_err()
        {
            bail!("屏幕截图失败");
        }

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: rect.width,
                biHeight: -rect.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (rect.width * rect.height * 4) as usize];
        let lines = GetDIBits(
            mem_dc.0,
            bitmap.0,
            0,
            rect.height as u32,
            Some(bgra.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        if lines == 0 {
            bail!("读取截图像素失败");
        }
        let mut rgba = Vec::with_capacity(bgra.len());
        for px in bgra.chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
        let image: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_vec(rect.width as u32, rect.height as u32, rgba)
                .ok_or_else(|| anyhow!("构造截图图像失败"))?;
        let mut out = Cursor::new(Vec::new());
        image
            .write_to(&mut out, ImageFormat::Png)
            .context("编码截图 PNG 失败")?;
        Ok(out.into_inner())
    }
}

fn clamp_rect_to_virtual_screen(rect: Rect) -> Rect {
    unsafe {
        let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let right = left + GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let bottom = top + GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let x1 = rect.x.clamp(left, right);
        let y1 = rect.y.clamp(top, bottom);
        let x2 = (rect.x + rect.width).clamp(left, right);
        let y2 = (rect.y + rect.height).clamp(top, bottom);
        Rect {
            x: x1,
            y: y1,
            width: (x2 - x1).max(0),
            height: (y2 - y1).max(0),
        }
    }
}
