use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};
use windows::Win32::UI::Shell::FOLDERID_RoamingAppData;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, PeekMessageW,
    PostMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_APP, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN, WM_USER,
};

use crate::{known_folder, log};

pub const WM_VOLUME_KEY: u32 = WM_APP + 3;
pub const KEY_UP: usize = 1;
pub const KEY_DOWN: usize = 2;
pub const KEY_MUTE: usize = 3;

static TARGET: AtomicIsize = AtomicIsize::new(0);
static AVAILABLE: AtomicBool = AtomicBool::new(false);
static MUTE_HELD: AtomicBool = AtomicBool::new(false);
static THREAD: AtomicU32 = AtomicU32::new(0);

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    volume_keys: bool,
}

impl Settings {
    fn path() -> PathBuf {
        known_folder(&FOLDERID_RoamingAppData)
            .join("phone-media")
            .join("settings.json")
    }

    fn load() -> Self {
        fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let path = Self::path();
        let written = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&path, serde_json::to_vec_pretty(self).unwrap_or_default()));
        if let Err(error) = written {
            log(format!("не удалось сохранить настройки: {error}"));
        }
    }
}

pub fn init(window: HWND) {
    TARGET.store(window.0 as isize, Ordering::Relaxed);
    if Settings::load().volume_keys {
        start();
    }
}

pub fn is_enabled() -> bool {
    THREAD.load(Ordering::Relaxed) != 0
}

pub fn set_enabled(enabled: bool) {
    Settings {
        volume_keys: enabled,
    }
    .save();
    if enabled {
        start();
        log("клавиши громкости управляют телефоном");
    } else {
        stop();
        log("клавиши громкости снова управляют компьютером");
    }
}

pub fn set_available(available: bool) {
    AVAILABLE.store(available, Ordering::Relaxed);
}

pub fn stop() {
    let thread = THREAD.swap(0, Ordering::Relaxed);
    if thread != 0 {
        let _ = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
    }
}

fn start() {
    if is_enabled() {
        return;
    }
    let (ready, started) = mpsc::channel();
    thread::spawn(move || unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), Some(instance.into()), 0) {
            Ok(hook) => hook,
            Err(error) => {
                log(format!("не удалось перехватить клавиши громкости: {error}"));
                let _ = ready.send(0);
                return;
            }
        };
        let mut message = MSG::default();
        let _ = PeekMessageW(&mut message, None, WM_USER, WM_USER, PM_NOREMOVE);
        let _ = ready.send(GetCurrentThreadId());
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&message);
        }
        let _ = UnhookWindowsHookEx(hook);
    });
    THREAD.store(started.recv().unwrap_or(0), Ordering::Relaxed);
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && AVAILABLE.load(Ordering::Relaxed) {
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let key = match VIRTUAL_KEY(info.vkCode as u16) {
            VK_VOLUME_UP => KEY_UP,
            VK_VOLUME_DOWN => KEY_DOWN,
            VK_VOLUME_MUTE => KEY_MUTE,
            _ => 0,
        };
        if key != 0 {
            let pressed = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let repeat = key == KEY_MUTE && MUTE_HELD.swap(pressed, Ordering::Relaxed) && pressed;
            if pressed && !repeat {
                let target = HWND(TARGET.load(Ordering::Relaxed) as _);
                let _ =
                    unsafe { PostMessageW(Some(target), WM_VOLUME_KEY, WPARAM(key), LPARAM(0)) };
            }
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
