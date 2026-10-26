use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{LIM_SMALL, LoadIconMetric};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFY_ICON_MESSAGE,
    NOTIFYICONDATAW, Shell_NotifyIconW, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, HICON, MF_CHECKED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SW_SHOWNORMAL, SendMessageW, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::{Event, autostart, log, log_path};

const WM_TRAY: u32 = WM_APP + 1;
const WM_REFRESH: u32 = WM_APP + 2;

const COMMAND_PAIR: usize = 1;
const COMMAND_CANCEL_PAIRING: usize = 2;
const COMMAND_AUTOSTART: usize = 3;
const COMMAND_LOG: usize = 4;
const COMMAND_QUIT: usize = 5;

#[derive(Clone, Default)]
pub struct Status {
    pub connected: bool,
    pub phone: String,
    pub track: Option<String>,
}

pub struct Tray {
    window: isize,
    status: Arc<Mutex<Status>>,
}

impl Tray {
    pub fn spawn(events: Sender<Event>, pairing: Arc<AtomicBool>) -> Self {
        let status = Arc::new(Mutex::new(Status {
            phone: "Ищу телефон…".to_string(),
            ..Status::default()
        }));
        let (ready, window) = mpsc::channel();
        let shared = status.clone();
        thread::spawn(move || run(events, shared, pairing, ready));
        Self {
            window: window.recv().unwrap_or(0),
            status,
        }
    }

    pub fn update(&self, change: impl FnOnce(&mut Status)) {
        if let Ok(mut status) = self.status.lock() {
            change(&mut status);
        }
        let _ = unsafe { PostMessageW(Some(self.hwnd()), WM_REFRESH, WPARAM(0), LPARAM(0)) };
    }

    pub fn remove(&self) {
        unsafe { SendMessageW(self.hwnd(), WM_CLOSE, None, None) };
    }

    fn hwnd(&self) -> HWND {
        HWND(self.window as _)
    }
}

struct State {
    events: Sender<Event>,
    status: Arc<Mutex<Status>>,
    pairing: Arc<AtomicBool>,
    taskbar_created: u32,
    connected_icon: HICON,
    disconnected_icon: HICON,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn run(
    events: Sender<Event>,
    status: Arc<Mutex<Status>>,
    pairing: Arc<AtomicBool>,
    ready: Sender<isize>,
) {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("PhoneMediaTray"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let Ok(window) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("PhoneMediaTray"),
            w!("Phone Media"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            log("не удалось создать окно значка в трее");
            let _ = ready.send(0);
            return;
        };
        let icon = |id: usize| {
            LoadIconMetric(Some(instance.into()), PCWSTR(id as _), LIM_SMALL).unwrap_or_default()
        };
        STATE.set(Some(State {
            events,
            status,
            pairing,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            connected_icon: icon(1),
            disconnected_icon: icon(2),
        }));
        notify(window, NIM_ADD);
        let _ = ready.send(window.0 as isize);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created =
        STATE.with_borrow(|state| state.as_ref().map(|state| state.taskbar_created));
    match message {
        WM_TRAY => {
            if matches!(lparam.0 as u32, WM_LBUTTONUP | WM_RBUTTONUP) {
                show_menu(window);
            }
        }
        WM_REFRESH => notify(window, NIM_MODIFY),
        WM_CLOSE => {
            notify(window, NIM_DELETE);
            let _ = unsafe { DestroyWindow(window) };
        }
        WM_DESTROY => unsafe { PostQuitMessage(0) },
        _ if Some(message) == taskbar_created => notify(window, NIM_ADD),
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    LRESULT(0)
}

fn notify(window: HWND, action: NOTIFY_ICON_MESSAGE) {
    STATE.with_borrow(|state| {
        let Some(state) = state else { return };
        let status = state
            .status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default();
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: if status.connected {
                state.connected_icon
            } else {
                state.disconnected_icon
            },
            ..Default::default()
        };
        let mut tip = format!("Phone Media\n{}", status.phone);
        if let Some(track) = &status.track {
            tip = format!("{tip}\n{track}");
        }
        let tip: Vec<u16> = tip.encode_utf16().take(data.szTip.len() - 1).collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        let _ = unsafe { Shell_NotifyIconW(action, &data) };
    });
}

fn show_menu(window: HWND) {
    let Some((status, pairing, events)) = STATE.with_borrow(|state| {
        state.as_ref().map(|state| {
            let status = state
                .status
                .lock()
                .map(|status| status.clone())
                .unwrap_or_default();
            (
                status,
                state.pairing.load(Ordering::Relaxed),
                state.events.clone(),
            )
        })
    }) else {
        return;
    };
    let autostart = autostart::is_enabled();

    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(
            menu,
            MF_STRING | MF_GRAYED,
            0,
            &HSTRING::from(&status.phone),
        );
        if let Some(track) = &status.track {
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, &HSTRING::from(track));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        if pairing {
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                COMMAND_CANCEL_PAIRING,
                w!("Отменить сопряжение"),
            );
        } else {
            let _ = AppendMenuW(menu, MF_STRING, COMMAND_PAIR, w!("Сопрячь новый телефон…"));
        }
        let checked = if autostart { MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(
            menu,
            MF_STRING | checked,
            COMMAND_AUTOSTART,
            w!("Запускать вместе с Windows"),
        );
        let _ = AppendMenuW(menu, MF_STRING, COMMAND_LOG, w!("Открыть журнал"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, COMMAND_QUIT, w!("Выход"));

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let _ = SetForegroundWindow(window);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            None,
            window,
            None,
        );
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        match command.0 as usize {
            COMMAND_PAIR => {
                let _ = events.send(Event::PairNew);
            }
            COMMAND_CANCEL_PAIRING => {
                let _ = events.send(Event::CancelPairing);
            }
            COMMAND_AUTOSTART => match autostart::set(!autostart) {
                Ok(()) if autostart => log("автозапуск выключен"),
                Ok(()) => log("автозапуск включён"),
                Err(error) => log(format!("не удалось изменить автозапуск: {error}")),
            },
            COMMAND_LOG => {
                ShellExecuteW(
                    None,
                    w!("open"),
                    &HSTRING::from(log_path().as_os_str()),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            }
            COMMAND_QUIT => {
                let _ = events.send(Event::Quit);
            }
            _ => {}
        }
    }
}
