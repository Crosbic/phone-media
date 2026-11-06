use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{LIM_SMALL, LoadIconMetric};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NIN_POPUPOPEN, NIN_SELECT, NINF_KEY, NOTIFY_ICON_MESSAGE, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect, Shell_NotifyIconW,
    ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, HICON, MF_CHECKED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SW_SHOWNORMAL, SendMessageW, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE,
    WM_CONTEXTMENU, WM_DESTROY, WM_NULL, WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::phone::Player;
use crate::{Event, autostart, log, log_path, popup, update, widget};

const WM_TRAY: u32 = WM_APP + 1;
const WM_REFRESH: u32 = WM_APP + 2;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
const ICON_ID: u32 = 1;
const MENU_REPEAT: Duration = Duration::from_millis(300);

const COMMAND_PAIR: usize = 1;
const COMMAND_CANCEL_PAIRING: usize = 2;
const COMMAND_AUTOSTART: usize = 3;
const COMMAND_LOG: usize = 4;
const COMMAND_QUIT: usize = 5;
const COMMAND_UPDATE: usize = 6;
const COMMAND_WINDOW: usize = 7;

#[derive(Clone, Default)]
pub struct Status {
    pub connected: bool,
    pub phone: String,
    pub name: Option<String>,
    pub track: Option<String>,
    pub player: Option<(Player, Instant)>,
    pub art: Option<Arc<[u8]>>,
    pub balloon: Option<(String, String)>,
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
    static MENU_OPEN: Cell<bool> = const { Cell::new(false) };
    static MENU_CLOSED: Cell<Option<Instant>> = const { Cell::new(None) };
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
        popup::create(status.clone(), events.clone());
        widget::init(status.clone(), events.clone());
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
        WM_TRAY => match (lparam.0 & 0xFFFF) as u32 {
            NIN_POPUPOPEN | NIN_SELECT if !MENU_OPEN.get() => popup::show(icon_rect(window)),
            NIN_KEYSELECT if menu_just_closed() => {}
            NIN_KEYSELECT | WM_CONTEXTMENU if !MENU_OPEN.get() => {
                popup::hide();
                MENU_OPEN.set(true);
                show_menu(window);
                MENU_OPEN.set(false);
                MENU_CLOSED.set(Some(Instant::now()));
            }
            _ => {}
        },
        WM_REFRESH => {
            notify(window, NIM_MODIFY);
            popup::refresh();
            widget::refresh();
        }
        WM_CLOSE => {
            popup::destroy();
            widget::shutdown();
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
        let (status, balloon) = state
            .status
            .lock()
            .map(|mut status| {
                let balloon = status.balloon.take();
                (status.clone(), balloon)
            })
            .unwrap_or_default();
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: ICON_ID,
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
        if let Some((title, text)) = balloon {
            data.uFlags |= NIF_INFO;
            data.dwInfoFlags = NIIF_INFO;
            let title: Vec<u16> = title
                .encode_utf16()
                .take(data.szInfoTitle.len() - 1)
                .collect();
            data.szInfoTitle[..title.len()].copy_from_slice(&title);
            let text: Vec<u16> = text.encode_utf16().take(data.szInfo.len() - 1).collect();
            data.szInfo[..text.len()].copy_from_slice(&text);
        }
        let _ = unsafe { Shell_NotifyIconW(action, &data) };
        if action == NIM_ADD {
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) };
        }
    });
}

fn icon_rect(window: HWND) -> RECT {
    let mut cursor = POINT::default();
    let _ = unsafe { GetCursorPos(&mut cursor) };
    let identifier = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: window,
        uID: ICON_ID,
        ..Default::default()
    };
    match unsafe { Shell_NotifyIconGetRect(&identifier) } {
        Ok(rect)
            if rect.right > rect.left
                && (rect.left - 24..=rect.right + 24).contains(&cursor.x)
                && (rect.top - 24..=rect.bottom + 24).contains(&cursor.y) =>
        {
            rect
        }
        _ => RECT {
            left: cursor.x - 8,
            top: cursor.y - 8,
            right: cursor.x + 8,
            bottom: cursor.y + 8,
        },
    }
}

fn menu_just_closed() -> bool {
    MENU_CLOSED
        .get()
        .is_some_and(|closed| closed.elapsed() < MENU_REPEAT)
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
        let window_open = if widget::is_open() {
            MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(
            menu,
            MF_STRING | window_open,
            COMMAND_WINDOW,
            w!("Окно мини-плеера"),
        );
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
        let _ = AppendMenuW(menu, MF_STRING, COMMAND_UPDATE, w!("Проверить обновления"));
        let _ = AppendMenuW(menu, MF_STRING, COMMAND_LOG, w!("Открыть журнал"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let about = format!(
            "Phone Media {} · © 2026 Crosbic · GNU GPL v3",
            update::VERSION
        );
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, &HSTRING::from(about));
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
            COMMAND_UPDATE => {
                let _ = events.send(Event::CheckUpdates);
            }
            COMMAND_WINDOW => widget::toggle(),
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
