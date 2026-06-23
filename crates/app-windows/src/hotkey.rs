use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex, OnceLock},
    thread,
};
use tokio::sync::mpsc;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT},
            WindowsAndMessaging::{
                CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
                UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL,
                WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_SYSKEYDOWN, WM_XBUTTONDOWN,
            },
        },
    },
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    X1,
    X2,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MouseEvent {
    pub button: MouseButton,
    pub x: i32,
    pub y: i32,
    pub time: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct KeyboardEvent {
    pub vk_code: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub time: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum GlobalInputEvent {
    Mouse(MouseEvent),
    Keyboard(KeyboardEvent),
}

static SENDER: OnceLock<Arc<Mutex<mpsc::UnboundedSender<GlobalInputEvent>>>> = OnceLock::new();

pub struct GlobalInputHook {
    mouse_hook: Arc<Mutex<Option<isize>>>,
    keyboard_hook: Arc<Mutex<Option<isize>>>,
}

impl GlobalInputHook {
    pub fn start(sender: mpsc::UnboundedSender<GlobalInputEvent>) -> Result<Self> {
        let mouse_hook = Arc::new(Mutex::new(None));
        let keyboard_hook = Arc::new(Mutex::new(None));
        let mouse_hook_for_thread = mouse_hook.clone();
        let keyboard_hook_for_thread = keyboard_hook.clone();
        let sender = Arc::new(Mutex::new(sender));

        thread::Builder::new()
            .name("ocr-translator-global-input-hook".to_string())
            .spawn(move || unsafe {
                let _ = SENDER.set(sender);
                let module = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
                let mouse_hook =
                    SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), HINSTANCE(module.0), 0).ok();
                let keyboard_hook =
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), HINSTANCE(module.0), 0)
                        .ok();
                if let Ok(mut slot) = mouse_hook_for_thread.lock() {
                    *slot = mouse_hook.map(|h| h.0 as isize);
                }
                if let Ok(mut slot) = keyboard_hook_for_thread.lock() {
                    *slot = keyboard_hook.map(|h| h.0 as isize);
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })
            .map_err(|e| anyhow!("启动全局输入监听线程失败：{e}"))?;

        Ok(Self {
            mouse_hook,
            keyboard_hook,
        })
    }
}

impl Drop for GlobalInputHook {
    fn drop(&mut self) {
        if let Ok(mut hook) = self.mouse_hook.lock() {
            if let Some(hook) = hook.take() {
                unsafe {
                    let _ = UnhookWindowsHookEx(HHOOK(hook as _));
                }
            }
        }
        if let Ok(mut hook) = self.keyboard_hook.lock() {
            if let Some(hook) = hook.take() {
                unsafe {
                    let _ = UnhookWindowsHookEx(HHOOK(hook as _));
                }
            }
        }
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = *(lparam.0 as *const MSLLHOOKSTRUCT);
        let button = match wparam.0 as u32 {
            WM_LBUTTONDOWN => Some(MouseButton::Left),
            WM_XBUTTONDOWN => {
                let xbutton = ((info.mouseData >> 16) & 0xffff) as u16;
                if xbutton == 1 {
                    Some(MouseButton::X1)
                } else if xbutton == 2 {
                    Some(MouseButton::X2)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let (Some(sender), Some(button)) = (SENDER.get(), button) {
            if let Ok(sender) = sender.lock() {
                let _ = sender.send(GlobalInputEvent::Mouse(MouseEvent {
                    button,
                    x: info.pt.x,
                    y: info.pt.y,
                    time: info.time,
                }));
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let info = *(lparam.0 as *const KBDLLHOOKSTRUCT);
        if let Some(sender) = SENDER.get() {
            if let Ok(sender) = sender.lock() {
                let _ = sender.send(GlobalInputEvent::Keyboard(KeyboardEvent {
                    vk_code: info.vkCode,
                    ctrl: key_down(VK_CONTROL.0 as i32),
                    alt: key_down(VK_MENU.0 as i32),
                    shift: key_down(VK_SHIFT.0 as i32),
                    time: info.time,
                }));
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn key_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}
