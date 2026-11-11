use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY,
    MONITORINFO, MonitorFromPoint, MonitorFromRect, PAINTSTRUCT, ScreenToClient, UpdateWindow,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    FOLDERID_RoamingAppData, ITaskbarList3, THB_FLAGS, THB_ICON, THB_TOOLTIP, THBF_DISABLED,
    THBF_ENABLED, THBN_CLICKED, THUMBBUTTON, TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, GWL_EXSTYLE, GWL_STYLE, GetClientRect, GetCursorPos, GetWindowLongPtrW,
    GetWindowRect, HICON, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT,
    HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, IsIconic,
    LoadCursorW, LoadIconW, MA_NOACTIVATE, MF_CHECKED, MF_SEPARATOR, MF_STRING, MINMAXINFO,
    PostMessageW, RegisterClassW, RegisterWindowMessageW, SET_WINDOW_POS_FLAGS, SM_CXSMICON,
    SW_HIDE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
    SetWindowPos, SetWindowTextW, ShowWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CAPTURECHANGED, WM_CLOSE, WM_COMMAND,
    WM_CONTEXTMENU, WM_DPICHANGED, WM_DWMCOLORIZATIONCOLORCHANGED,
    WM_DWMSENDICONICLIVEPREVIEWBITMAP, WM_DWMSENDICONICTHUMBNAIL, WM_ERASEBKGND, WM_EXITSIZEMOVE,
    WM_GETMINMAXINFO, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCRBUTTONDOWN,
    WM_NCRBUTTONUP, WM_NULL, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_SIZE,
    WM_SIZING, WM_THEMECHANGED, WM_TIMER, WMSZ_TOP, WMSZ_TOPLEFT, WMSZ_TOPRIGHT, WNDCLASSW,
    WS_CAPTION, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_MINIMIZEBOX,
    WS_POPUP, WS_THICKFRAME,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::tray::Status;
use crate::view::{
    COMMAND_NEXT, COMMAND_PREVIOUS, COMMAND_TOGGLE, Glyph, Layout, Transport, View, fit_height,
    tallest,
};
use crate::{Event, known_folder, log};

const CLASS: PCWSTR = w!("PhoneMediaWindow");
const SIZE: (f32, f32) = (340.0, 170.0);
const MIN_SIZE: (f32, f32) = (260.0, 180.0);
const MAX_WIDTH: f32 = 720.0;
const SCREEN_MARGIN: f32 = 24.0;
const EDGE: f32 = 6.0;
const TICK_TIMER: usize = 1;
const TICK_EVERY: u32 = 250;
const MENU_TOPMOST: usize = 1;
const MENU_CLOSE: usize = 2;

#[derive(Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Saved {
    open: bool,
    topmost: bool,
    rect: Option<[i32; 4]>,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            open: false,
            topmost: true,
            rect: None,
        }
    }
}

impl Saved {
    fn path() -> PathBuf {
        known_folder(&FOLDERID_RoamingAppData)
            .join("phone-media")
            .join("window.json")
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
            log(format!("не удалось сохранить окно мини-плеера: {error}"));
        }
    }
}

type Context = (Arc<Mutex<Status>>, Sender<Event>);

struct Widget {
    view: View,
    saved: Saved,
    title: String,
    taskbar: Option<ITaskbarList3>,
    buttons: Option<(Transport, bool)>,
    icons: Option<Icons>,
    retired: Vec<Icons>,
}

struct Icons {
    size: i32,
    light: bool,
    handles: [HICON; 4],
}

impl Drop for Icons {
    fn drop(&mut self) {
        for icon in self.handles {
            unsafe {
                let _ = DestroyIcon(icon);
            }
        }
    }
}

thread_local! {
    static WIDGET: RefCell<Option<Widget>> = const { RefCell::new(None) };
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

pub fn init(status: Arc<Mutex<Status>>, events: Sender<Event>) {
    CONTEXT.set(Some((status, events)));
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else {
            return;
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            hIcon: LoadIconW(Some(instance.into()), PCWSTR(1 as _)).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassW(&class);
    }
    if Saved::load().open {
        open();
    }
}

pub fn is_open() -> bool {
    WIDGET.with(|cell| cell.try_borrow().map_or(true, |widget| widget.is_some()))
}

pub fn toggle() {
    if is_open() {
        close();
    } else {
        open();
    }
}

pub fn refresh() {
    with(|widget| {
        widget.view.invalidate();
        widget.retitle();
        if !widget.pinned() {
            widget.view.invalidate_iconic();
            widget.sync_buttons(false);
        }
    });
}

pub fn shutdown() {
    if let Some(mut widget) = take() {
        widget.store();
        unsafe {
            let _ = DestroyWindow(widget.view.window);
        }
    }
}

fn open() {
    let Some((status, events)) = CONTEXT.with_borrow(|context| context.clone()) else {
        return;
    };
    match Widget::create(status, events) {
        Ok(widget) => {
            let window = widget.view.window;
            WIDGET.set(Some(widget));
            unsafe {
                let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
                SendMessageW(window, WM_NCACTIVATE, Some(WPARAM(1)), Some(LPARAM(0)));
                let _ = UpdateWindow(window);
            }
        }
        Err(error) => log(format!("не удалось открыть окно мини-плеера: {error}")),
    }
}

fn close() {
    if let Some(mut widget) = take() {
        widget.saved.open = false;
        widget.store();
        unsafe {
            let _ = DestroyWindow(widget.view.window);
        }
    }
}

fn take() -> Option<Widget> {
    WIDGET.with(|cell| cell.try_borrow_mut().ok()?.take())
}

fn with<T>(action: impl FnOnce(&mut Widget) -> T) -> Option<T> {
    WIDGET.with(|cell| {
        let mut widget = cell.try_borrow_mut().ok()?;
        widget.as_mut().map(action)
    })
}

impl Widget {
    fn create(status: Arc<Mutex<Status>>, events: Sender<Event>) -> Result<Self> {
        let mut saved = Saved::load();
        saved.open = true;
        let (x, y, width, height) = initial_rect(&saved);
        let (style, extended) = styles(saved.topmost);
        let window = unsafe {
            CreateWindowExW(
                extended,
                CLASS,
                w!("Phone Media"),
                style,
                x,
                y,
                width,
                height,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        let mut view = match View::new(window, status, events, Layout::Window) {
            Ok(view) => view,
            Err(error) => {
                unsafe {
                    let _ = DestroyWindow(window);
                }
                return Err(error);
            }
        };
        view.rescale(unsafe { GetDpiForWindow(window) } as f32 / 96.0);
        view.set_iconic(!saved.topmost);
        unsafe { SetTimer(Some(window), TICK_TIMER, TICK_EVERY, None) };
        let mut widget = Self {
            view,
            saved,
            title: String::new(),
            taskbar: None,
            buttons: None,
            icons: None,
            retired: Vec::new(),
        };
        widget.store();
        widget.retitle();
        Ok(widget)
    }

    fn store(&mut self) {
        let window = self.view.window;
        let mut rect = RECT::default();
        let minimized = unsafe { IsIconic(window) }.as_bool();
        if !minimized && unsafe { GetWindowRect(window, &mut rect) }.is_ok() {
            self.saved.rect = Some([rect.left, rect.top, rect.right, rect.bottom]);
        }
        self.saved.save();
    }

    fn toggle_topmost(&mut self) {
        let pinned = !self.saved.topmost;
        self.saved.topmost = pinned;
        self.view.set_iconic(!pinned);
        self.buttons = None;
        let window = self.view.window;
        let (style, extended) = styles(pinned);
        let switched = WS_MINIMIZEBOX.0 as isize;
        let switched_extended = (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_APPWINDOW).0 as isize;
        unsafe {
            let _ = ShowWindow(window, SW_HIDE);
            let current = GetWindowLongPtrW(window, GWL_STYLE);
            let style = (current & !switched) | (style.0 as isize & switched);
            SetWindowLongPtrW(window, GWL_STYLE, style);
            let current = GetWindowLongPtrW(window, GWL_EXSTYLE);
            let extended =
                (current & !switched_extended) | (extended.0 as isize & switched_extended);
            SetWindowLongPtrW(window, GWL_EXSTYLE, extended);
            let (order, activation) = if pinned {
                (HWND_TOPMOST, SWP_NOACTIVATE)
            } else {
                (HWND_NOTOPMOST, SET_WINDOW_POS_FLAGS(0))
            };
            let _ = SetWindowPos(
                window,
                Some(order),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW | activation,
            );
            SendMessageW(window, WM_NCACTIVATE, Some(WPARAM(1)), Some(LPARAM(0)));
        }
        self.store();
    }

    fn pinned(&self) -> bool {
        self.saved.topmost
    }

    fn retitle(&mut self) {
        let title = self.view.title();
        if title != self.title {
            unsafe {
                let _ = SetWindowTextW(self.view.window, &HSTRING::from(&title));
            }
            self.title = title;
        }
    }

    fn icons(&mut self) -> Option<[HICON; 4]> {
        let window = self.view.window;
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(window)) };
        let light = self.view.look.light;
        if let Some(icons) = &self.icons
            && icons.size == size
            && icons.light == light
        {
            return Some(icons.handles);
        }
        let glyphs = [Glyph::Previous, Glyph::Play, Glyph::Pause, Glyph::Next];
        let mut handles = [HICON::default(); 4];
        for (handle, glyph) in handles.iter_mut().zip(glyphs) {
            match self.view.glyph(glyph, size) {
                Ok(icon) => *handle = icon,
                Err(error) => {
                    log(format!("не удалось нарисовать кнопки превью: {error}"));
                    drop(Icons {
                        size,
                        light,
                        handles,
                    });
                    return None;
                }
            }
        }
        if let Some(old) = self.icons.replace(Icons {
            size,
            light,
            handles,
        }) {
            self.retired.push(old);
        }
        Some(handles)
    }

    fn sync_buttons(&mut self, created: bool) {
        if self.pinned() || (!created && self.buttons.is_none()) {
            return;
        }
        let transport = self.view.transport();
        let light = self.view.look.light;
        if !created && self.buttons == Some((transport, light)) {
            return;
        }
        let Some(icons) = self.icons() else {
            return;
        };
        if self.taskbar.is_none() {
            self.taskbar = unsafe {
                CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
            }
            .and_then(|list| unsafe { list.HrInit() }.map(|_| list))
            .map_err(|error| log(format!("панель задач недоступна: {error}")))
            .ok();
        }
        let Some(taskbar) = &self.taskbar else {
            return;
        };
        let toggle = if transport.playing {
            button(COMMAND_TOGGLE, icons[2], "Пауза", transport.toggle)
        } else {
            button(COMMAND_TOGGLE, icons[1], "Воспроизвести", transport.toggle)
        };
        let buttons = [
            button(COMMAND_PREVIOUS, icons[0], "Предыдущий", transport.previous),
            toggle,
            button(COMMAND_NEXT, icons[3], "Следующий", transport.next),
        ];
        let window = self.view.window;
        let result = unsafe {
            if created {
                taskbar.ThumbBarAddButtons(window, &buttons)
            } else {
                taskbar.ThumbBarUpdateButtons(window, &buttons)
            }
        };
        match result {
            Ok(()) => {
                self.buttons = Some((transport, light));
                self.retired.clear();
            }
            Err(error) => log(format!("не удалось показать кнопки превью: {error}")),
        }
    }

    fn hit_test(&self, lparam: LPARAM) -> u32 {
        let window = self.view.window;
        let mut point = POINT {
            x: (lparam.0 & 0xFFFF) as i16 as i32,
            y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
        };
        let mut client = RECT::default();
        unsafe {
            let _ = ScreenToClient(window, &mut point);
            let _ = GetClientRect(window, &mut client);
        }
        let edge = (EDGE * self.view.scale).round() as i32;
        let left = point.x < edge;
        let right = point.x >= client.right - edge;
        let top = point.y < edge;
        let bottom = point.y >= client.bottom - edge;
        match (left, right, top, bottom) {
            (true, _, true, _) => HTTOPLEFT,
            (_, true, true, _) => HTTOPRIGHT,
            (true, _, _, true) => HTBOTTOMLEFT,
            (_, true, _, true) => HTBOTTOMRIGHT,
            (true, ..) => HTLEFT,
            (_, true, ..) => HTRIGHT,
            (_, _, true, _) => HTTOP,
            (_, _, _, true) => HTBOTTOM,
            _ if self.view.interactive(point.x, point.y) => HTCLIENT,
            _ => HTCAPTION,
        }
    }
}

fn button(id: u32, icon: HICON, tip: &str, enabled: bool) -> THUMBBUTTON {
    let mut button = THUMBBUTTON {
        dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
        iId: id,
        hIcon: icon,
        dwFlags: if enabled { THBF_ENABLED } else { THBF_DISABLED },
        ..Default::default()
    };
    let tip: Vec<u16> = tip.encode_utf16().take(button.szTip.len() - 1).collect();
    button.szTip[..tip.len()].copy_from_slice(&tip);
    button
}

fn taskbar_button_created() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) })
}

fn styles(pinned: bool) -> (WINDOW_STYLE, WINDOW_EX_STYLE) {
    let frame = WS_POPUP | WS_CAPTION | WS_THICKFRAME;
    if pinned {
        (frame, WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST)
    } else {
        (frame | WS_MINIMIZEBOX, WS_EX_APPWINDOW)
    }
}

fn initial_rect(saved: &Saved) -> (i32, i32, i32, i32) {
    unsafe {
        if let Some([left, top, right, bottom]) = saved.rect {
            let rect = RECT {
                left,
                top,
                right,
                bottom,
            };
            let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONULL);
            if right > left && bottom > top && !monitor.is_invalid() {
                let (mut dpi, mut dpi_y) = (96, 96);
                let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
                let limit = |length: f32| (length * dpi as f32 / 96.0).round() as i32;
                let width = (right - left).min(limit(MAX_WIDTH));
                let height = (bottom - top)
                    .min(limit(fit_height(width as f32 * 96.0 / dpi as f32)))
                    .max(limit(MIN_SIZE.1));
                return (left, top, width, height);
            }
        }
        let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi, mut dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
        let pixels = |length: f32| (length * dpi as f32 / 96.0).round() as i32;
        let (width, height) = (pixels(SIZE.0), pixels(SIZE.1));
        let work = info.rcWork;
        (
            work.right - pixels(SCREEN_MARGIN) - width,
            work.bottom - pixels(SCREEN_MARGIN) - height,
            width,
            height,
        )
    }
}

fn context_menu(window: HWND) {
    let Some(topmost) = with(|widget| widget.saved.topmost) else {
        return;
    };
    match menu(window, topmost) {
        MENU_TOPMOST => {
            with(Widget::toggle_topmost);
        }
        MENU_CLOSE => close(),
        _ => {}
    }
}

fn menu(window: HWND, topmost: bool) -> usize {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return 0;
        };
        let pinned = if topmost { MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(
            menu,
            MF_STRING | pinned,
            MENU_TOPMOST,
            w!("Поверх всех окон"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, MENU_CLOSE, w!("Закрыть"));
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
        command.0 as usize
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            unsafe { BeginPaint(window, &mut paint) };
            with(|widget| widget.view.paint(paint.hdc));
            unsafe {
                let _ = EndPaint(window, &paint);
            }
        }
        WM_NCCALCSIZE if wparam.0 != 0 => {}
        WM_NCHITTEST => {
            let hit = with(|widget| widget.hit_test(lparam)).unwrap_or(HTCLIENT);
            return LRESULT(hit as isize);
        }
        WM_NCACTIVATE => return unsafe { DefWindowProcW(window, message, WPARAM(1), lparam) },
        WM_NCLBUTTONDBLCLK => {}
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEACTIVATE => {
            if with(|widget| widget.pinned()).unwrap_or(true) {
                return LRESULT(MA_NOACTIVATE as isize);
            }
            return unsafe { DefWindowProcW(window, message, wparam, lparam) };
        }
        WM_MOUSEWHEEL => {
            with(|widget| widget.view.wheel(((wparam.0 >> 16) & 0xFFFF) as i16));
        }
        WM_MOUSEMOVE => {
            with(|widget| widget.view.mouse_move(lparam));
        }
        WM_MOUSELEAVE => {
            with(|widget| widget.view.mouse_leave());
        }
        WM_LBUTTONDOWN => {
            with(|widget| widget.view.mouse_down(lparam));
        }
        WM_LBUTTONUP => {
            with(|widget| widget.view.mouse_up(lparam));
        }
        WM_CAPTURECHANGED => {
            with(|widget| widget.view.lost_capture());
        }
        WM_RBUTTONDOWN | WM_NCRBUTTONDOWN => {}
        WM_RBUTTONUP | WM_NCRBUTTONUP | WM_CONTEXTMENU => context_menu(window),
        WM_CLOSE => close(),
        WM_TIMER if wparam.0 == TICK_TIMER => {
            with(|widget| {
                if widget.view.tick() && !widget.pinned() {
                    widget.view.invalidate_iconic();
                }
            });
        }
        WM_DWMSENDICONICTHUMBNAIL => {
            let width = ((lparam.0 >> 16) & 0xFFFF) as i32;
            let height = (lparam.0 & 0xFFFF) as i32;
            with(|widget| widget.view.send_thumbnail(width, height));
        }
        WM_DWMSENDICONICLIVEPREVIEWBITMAP => {
            with(|widget| widget.view.send_preview());
        }
        WM_SIZE => {
            with(|widget| widget.view.invalidate());
        }
        WM_EXITSIZEMOVE => {
            with(Widget::store);
        }
        WM_GETMINMAXINFO => {
            let scale = unsafe { GetDpiForWindow(window) }.max(96) as f32 / 96.0;
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            let pixels = |size: (f32, f32)| POINT {
                x: (size.0 * scale).round() as i32,
                y: (size.1 * scale).round() as i32,
            };
            info.ptMinTrackSize = pixels(MIN_SIZE);
            info.ptMaxTrackSize = pixels((MAX_WIDTH, tallest()));
        }
        WM_SIZING => {
            let scale = unsafe { GetDpiForWindow(window) }.max(96) as f32 / 96.0;
            let rect = unsafe { &mut *(lparam.0 as *mut RECT) };
            let width = (rect.right - rect.left) as f32 / scale;
            let limit = (fit_height(width) * scale).round() as i32;
            if rect.bottom - rect.top > limit {
                match wparam.0 as u32 {
                    WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT => rect.top = rect.bottom - limit,
                    _ => rect.bottom = rect.top + limit,
                }
            }
            return LRESULT(1);
        }
        WM_DPICHANGED => {
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    window,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            let scale = (wparam.0 & 0xFFFF) as f32 / 96.0;
            with(|widget| widget.view.rescale(scale));
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED | WM_DWMCOLORIZATIONCOLORCHANGED => {
            with(|widget| {
                widget.view.restyle();
                widget.sync_buttons(false);
            });
            return unsafe { DefWindowProcW(window, message, wparam, lparam) };
        }
        WM_COMMAND if ((wparam.0 >> 16) & 0xFFFF) as u32 == THBN_CLICKED => {
            with(|widget| widget.view.command((wparam.0 & 0xFFFF) as u32));
        }
        _ if message == taskbar_button_created() => {
            with(|widget| widget.sync_buttons(true));
        }
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    LRESULT(0)
}
