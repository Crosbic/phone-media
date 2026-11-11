use std::cell::RefCell;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
    PAINTSTRUCT, PtInRect, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowRect, HTCLIENT,
    HWND_TOPMOST, IDC_ARROW, IsWindowVisible, KillTimer, LoadCursorW, MA_NOACTIVATE,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SendMessageW, SetTimer,
    SetWindowPos, ShowWindow, WM_CAPTURECHANGED, WM_DWMCOLORIZATIONCOLORCHANGED, WM_ERASEBKGND,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCACTIVATE,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_PAINT, WM_SETTINGCHANGE, WM_THEMECHANGED, WM_TIMER, WNDCLASSW,
    WS_CAPTION, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, Result, w};

use crate::tray::Status;
use crate::view::{Layout, OSD_WIDTH, View};
use crate::{Event, log};

const CLASS: PCWSTR = w!("PhoneMediaVolume");
const SHOW_FOR: Duration = Duration::from_millis(1500);
const FADE_IN: Duration = Duration::from_millis(150);
const FADE_OUT: Duration = Duration::from_millis(200);
const BOTTOM: f32 = 24.0;
const TIMER: usize = 1;
const TIMER_EVERY: u32 = 15;

struct Fade {
    start: Instant,
    duration: Duration,
    from: f32,
    to: f32,
}

impl Fade {
    fn progress(&self) -> f32 {
        let time = self.start.elapsed().as_secs_f32() / self.duration.as_secs_f32();
        1.0 - (1.0 - time.min(1.0)).powi(3)
    }
}

struct Osd {
    view: View,
    visible: bool,
    hide_at: Instant,
    fade: Option<Fade>,
}

thread_local! {
    static OSD: RefCell<Option<Osd>> = const { RefCell::new(None) };
}

pub fn create(status: Arc<Mutex<Status>>, events: Sender<Event>) {
    match Osd::new(status, events) {
        Ok(osd) => OSD.set(Some(osd)),
        Err(error) => log(format!("индикатор громкости недоступен: {error}")),
    }
}

pub fn show() {
    if let Some(Some(window)) = with(|osd| osd.show().then_some(osd.view.window)) {
        unsafe {
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            SendMessageW(window, WM_NCACTIVATE, Some(WPARAM(1)), Some(LPARAM(0)));
            let _ = UpdateWindow(window);
        }
    }
}

pub fn refresh() {
    with(|osd| {
        if osd.visible {
            osd.place();
        }
    });
}

pub fn destroy() {
    if let Some(osd) = OSD.take() {
        unsafe {
            let _ = DestroyWindow(osd.view.window);
        }
    }
}

fn with<T>(action: impl FnOnce(&mut Osd) -> T) -> Option<T> {
    OSD.with(|cell| {
        let mut osd = cell.try_borrow_mut().ok()?;
        osd.as_mut().map(action)
    })
}

impl Osd {
    fn new(status: Arc<Mutex<Status>>, events: Sender<Event>) -> Result<Self> {
        let window = unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassW(&class);
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                CLASS,
                w!("Phone Media"),
                WS_POPUP | WS_CAPTION,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };
        Ok(Self {
            view: View::new(window, status, events, Layout::Osd)?,
            visible: false,
            hide_at: Instant::now(),
            fade: None,
        })
    }

    fn place(&mut self) -> bool {
        self.view.restyle();
        unsafe {
            let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(monitor, &mut info);
            let (mut dpi, mut dpi_y) = (96, 96);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
            let scale = dpi as f32 / 96.0;
            self.view.rescale(scale);
            let Some(height) = self.view.layout() else {
                return false;
            };
            let pixels = |length: f32| (length * scale).round() as i32;
            let (width, height) = (pixels(OSD_WIDTH), pixels(height));
            let work = info.rcWork;
            let x = (work.left + work.right) / 2 - width / 2;
            let y = work.bottom - pixels(BOTTOM) - height;
            let _ = SetWindowPos(
                self.view.window,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
        }
        self.view.invalidate();
        true
    }

    fn show(&mut self) -> bool {
        if !self.place() {
            return false;
        }
        self.hide_at = Instant::now() + SHOW_FOR;
        if self.visible {
            return false;
        }
        self.visible = true;
        let window = self.view.window;
        let hidden = unsafe { !IsWindowVisible(window).as_bool() };
        let from = if hidden { 0.0 } else { self.view.opacity };
        self.fade = None;
        if self.view.look.animated {
            self.view.set_opacity(from);
            self.fade = Some(Fade {
                start: Instant::now(),
                duration: FADE_IN,
                from,
                to: 1.0,
            });
        } else {
            self.view.set_opacity(1.0);
        }
        unsafe { SetTimer(Some(window), TIMER, TIMER_EVERY, None) };
        hidden
    }

    fn hide(&mut self) {
        self.visible = false;
        self.view.reset();
        if self.view.look.animated {
            self.fade = Some(Fade {
                start: Instant::now(),
                duration: FADE_OUT,
                from: self.view.opacity,
                to: 0.0,
            });
            return;
        }
        self.fade = None;
        self.view.set_opacity(0.0);
        self.finish();
    }

    fn finish(&mut self) {
        let window = self.view.window;
        unsafe {
            let _ = KillTimer(Some(window), TIMER);
            let _ = ShowWindow(window, SW_HIDE);
        }
    }

    fn tick(&mut self) {
        if let Some(fade) = &self.fade {
            let progress = fade.progress();
            let opacity = fade.from + (fade.to - fade.from) * progress;
            self.view.set_opacity(opacity);
            if progress >= 1.0 {
                self.fade = None;
                if !self.visible {
                    self.finish();
                    return;
                }
            }
        }
        if !self.visible {
            return;
        }
        let mut cursor = POINT::default();
        let mut frame = RECT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
            let _ = GetWindowRect(self.view.window, &mut frame);
        }
        let over = unsafe { PtInRect(&frame, cursor) }.as_bool();
        if over || self.view.busy() {
            self.hide_at = Instant::now() + SHOW_FOR;
        } else {
            self.view.clear_hover();
            if Instant::now() >= self.hide_at {
                self.hide();
            }
        }
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
            with(|osd| osd.view.paint(paint.hdc));
            unsafe {
                let _ = EndPaint(window, &paint);
            }
        }
        WM_NCCALCSIZE if wparam.0 != 0 => {}
        WM_NCHITTEST => return LRESULT(HTCLIENT as isize),
        WM_NCACTIVATE => return unsafe { DefWindowProcW(window, message, WPARAM(1), lparam) },
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            with(|osd| osd.view.mouse_move(lparam));
        }
        WM_MOUSELEAVE => {
            with(|osd| osd.view.mouse_leave());
        }
        WM_LBUTTONDOWN => {
            with(|osd| osd.view.mouse_down(lparam));
        }
        WM_LBUTTONUP => {
            with(|osd| osd.view.mouse_up(lparam));
        }
        WM_MOUSEWHEEL => {
            with(|osd| osd.view.wheel(((wparam.0 >> 16) & 0xFFFF) as i16));
        }
        WM_CAPTURECHANGED => {
            with(|osd| osd.view.lost_capture());
        }
        WM_TIMER if wparam.0 == TIMER => {
            with(Osd::tick);
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED | WM_DWMCOLORIZATIONCOLORCHANGED => {
            with(|osd| osd.view.restyle());
            return unsafe { DefWindowProcW(window, message, wparam, lparam) };
        }
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    LRESULT(0)
}
