use std::cell::RefCell;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
    PAINTSTRUCT, PtInRect, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowRect, HTCLIENT,
    HWND_TOPMOST, IDC_ARROW, IsWindowVisible, KillTimer, LoadCursorW, MA_NOACTIVATE,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    SendMessageW, SetTimer, SetWindowPos, ShowWindow, WM_CAPTURECHANGED,
    WM_DWMCOLORIZATIONCOLORCHANGED, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_PAINT, WM_SETTINGCHANGE,
    WM_THEMECHANGED, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, Result, w};

use crate::tray::Status;
use crate::view::{View, WIDTH};
use crate::{Event, log};

const CLASS: PCWSTR = w!("PhoneMediaPopup");
const MARGIN: f32 = 12.0;
const LEAVE_DELAY: Duration = Duration::from_millis(400);
const WATCH_TIMER: usize = 1;
const WATCH_EVERY: u32 = 100;
const ANIMATION_TIMER: usize = 2;
const ANIMATION_EVERY: u32 = 10;
const SHOW_TIME: Duration = Duration::from_millis(180);
const HIDE_TIME: Duration = Duration::from_millis(120);
const SLIDE: f32 = 12.0;
const HIDE_SLIDE: f32 = 6.0;

struct Animation {
    start: Instant,
    duration: Duration,
    from: f32,
    to: f32,
    from_offset: i32,
    to_offset: i32,
}

impl Animation {
    fn progress(&self) -> f32 {
        let time = self.start.elapsed().as_secs_f32() / self.duration.as_secs_f32();
        1.0 - (1.0 - time.min(1.0)).powi(3)
    }

    fn offset(&self) -> i32 {
        let shift = (self.to_offset - self.from_offset) as f32 * self.progress();
        self.from_offset + shift.round() as i32
    }
}

struct Popup {
    view: View,
    anchor: RECT,
    origin: (i32, i32),
    direction: i32,
    animation: Option<Animation>,
    visible: bool,
    inside_at: Instant,
}

thread_local! {
    static POPUP: RefCell<Option<Popup>> = const { RefCell::new(None) };
}

pub fn create(status: Arc<Mutex<Status>>, events: Sender<Event>) {
    match Popup::new(status, events) {
        Ok(popup) => POPUP.set(Some(popup)),
        Err(error) => log(format!("мини-плеер недоступен: {error}")),
    }
}

pub fn show(anchor: RECT) {
    if let Some(Some(window)) = with(|popup| popup.place(anchor).then_some(popup.view.window)) {
        unsafe {
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            SendMessageW(window, WM_NCACTIVATE, Some(WPARAM(1)), Some(LPARAM(0)));
            let _ = UpdateWindow(window);
        }
    }
}

pub fn refresh() {
    with(Popup::refresh);
}

pub fn hide() {
    with(Popup::hide);
}

pub fn destroy() {
    if let Some(popup) = POPUP.take() {
        unsafe {
            let _ = DestroyWindow(popup.view.window);
        }
    }
}

fn with<T>(action: impl FnOnce(&mut Popup) -> T) -> Option<T> {
    POPUP.with(|cell| {
        let mut popup = cell.try_borrow_mut().ok()?;
        popup.as_mut().map(action)
    })
}

impl Popup {
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
            view: View::new(window, status, events, true, false)?,
            anchor: RECT::default(),
            origin: (0, 0),
            direction: 1,
            animation: None,
            visible: false,
            inside_at: Instant::now(),
        })
    }

    fn refresh(&mut self) {
        if self.visible {
            self.place(self.anchor);
        }
    }

    fn place(&mut self, anchor: RECT) -> bool {
        self.anchor = anchor;
        self.view.restyle();
        let window = self.view.window;
        unsafe {
            let monitor = MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST);
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
            let (width, height) = (pixels(WIDTH), pixels(height));
            let (x, y) = position(anchor, info.rcWork, width, height, pixels(MARGIN));
            self.origin = (x, y);
            if !self.visible {
                let hidden = !IsWindowVisible(window).as_bool();
                let current = self.offset();
                self.direction = if y < anchor.top { 1 } else { -1 };
                self.animation = None;
                let mut opacity = if hidden { 0.0 } else { self.view.opacity };
                if self.view.look.animated {
                    self.animation = Some(Animation {
                        start: Instant::now(),
                        duration: SHOW_TIME,
                        from: opacity,
                        to: 1.0,
                        from_offset: if hidden {
                            self.direction * pixels(SLIDE)
                        } else {
                            current
                        },
                        to_offset: 0,
                    });
                    SetTimer(Some(window), ANIMATION_TIMER, ANIMATION_EVERY, None);
                } else {
                    opacity = 1.0;
                }
                self.view.set_opacity(opacity);
            }
            let _ = SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                x,
                y + self.offset(),
                width,
                height,
                SWP_NOACTIVATE,
            );
        }
        self.view.invalidate();
        if self.visible {
            return false;
        }
        self.visible = true;
        self.inside_at = Instant::now();
        self.view.reset();
        unsafe {
            SetTimer(Some(window), WATCH_TIMER, WATCH_EVERY, None);
            !IsWindowVisible(window).as_bool()
        }
    }

    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        let window = self.view.window;
        self.visible = false;
        self.view.reset();
        unsafe {
            let _ = KillTimer(Some(window), WATCH_TIMER);
        }
        if !self.view.look.animated {
            self.animation = None;
            self.view.set_opacity(0.0);
            unsafe {
                let _ = ShowWindow(window, SW_HIDE);
            }
            return;
        }
        self.animation = Some(Animation {
            start: Instant::now(),
            duration: HIDE_TIME,
            from: self.view.opacity,
            to: 0.0,
            from_offset: self.offset(),
            to_offset: self.direction * (HIDE_SLIDE * self.view.scale).round() as i32,
        });
        unsafe { SetTimer(Some(window), ANIMATION_TIMER, ANIMATION_EVERY, None) };
    }

    fn offset(&self) -> i32 {
        self.animation.as_ref().map_or(0, Animation::offset)
    }

    fn animate(&mut self) {
        let window = self.view.window;
        let Some(animation) = &self.animation else {
            unsafe {
                let _ = KillTimer(Some(window), ANIMATION_TIMER);
            }
            return;
        };
        let progress = animation.progress();
        let offset = animation.offset();
        let opacity = animation.from + (animation.to - animation.from) * progress;
        self.view.set_opacity(opacity);
        let (x, y) = self.origin;
        unsafe {
            let _ = SetWindowPos(
                window,
                None,
                x,
                y + offset,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        if progress < 1.0 {
            return;
        }
        self.animation = None;
        unsafe {
            let _ = KillTimer(Some(window), ANIMATION_TIMER);
            if !self.visible {
                let _ = ShowWindow(window, SW_HIDE);
            }
        }
    }

    fn watch(&mut self) {
        let mut cursor = POINT::default();
        let mut frame = RECT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
            let _ = GetWindowRect(self.view.window, &mut frame);
        }
        let near = RECT {
            left: self.anchor.left - 4,
            top: self.anchor.top - 4,
            right: self.anchor.right + 4,
            bottom: self.anchor.bottom + 4,
        };
        let over_popup = unsafe { PtInRect(&frame, cursor) }.as_bool();
        let over_icon = unsafe { PtInRect(&near, cursor) }.as_bool();
        let busy = self.view.busy();
        if over_popup || over_icon || busy {
            self.inside_at = Instant::now();
        } else if self.inside_at.elapsed() > LEAVE_DELAY {
            self.hide();
            return;
        }
        if !over_popup {
            self.view.clear_hover();
        }
        self.view.tick();
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
            with(|popup| popup.view.paint(paint.hdc));
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
            with(|popup| popup.view.mouse_move(lparam));
        }
        WM_MOUSELEAVE => {
            with(|popup| popup.view.mouse_leave());
        }
        WM_LBUTTONDOWN => {
            with(|popup| popup.view.mouse_down(lparam));
        }
        WM_LBUTTONUP => {
            with(|popup| popup.view.mouse_up(lparam));
        }
        WM_CAPTURECHANGED => {
            with(|popup| popup.view.lost_capture());
        }
        WM_TIMER if wparam.0 == WATCH_TIMER => {
            with(Popup::watch);
        }
        WM_TIMER if wparam.0 == ANIMATION_TIMER => {
            with(Popup::animate);
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED | WM_DWMCOLORIZATIONCOLORCHANGED => {
            with(Popup::refresh);
            return unsafe { DefWindowProcW(window, message, wparam, lparam) };
        }
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    LRESULT(0)
}

fn position(anchor: RECT, work: RECT, width: i32, height: i32, margin: i32) -> (i32, i32) {
    let centered_x = (anchor.left + anchor.right) / 2 - width / 2;
    let centered_y = (anchor.top + anchor.bottom) / 2 - height / 2;
    let (x, y) = if anchor.top >= work.bottom {
        (centered_x, work.bottom - margin - height)
    } else if anchor.bottom <= work.top {
        (centered_x, work.top + margin)
    } else if anchor.left >= work.right {
        (work.right - margin - width, centered_y)
    } else if anchor.right <= work.left {
        (work.left + margin, centered_y)
    } else if anchor.top - margin - height >= work.top {
        (centered_x, anchor.top - margin - height)
    } else {
        (centered_x, anchor.bottom + margin)
    };
    (
        x.min(work.right - margin - width).max(work.left + margin),
        y.min(work.bottom - margin - height).max(work.top + margin),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1032,
    };

    fn icon(left: i32, top: i32) -> RECT {
        RECT {
            left,
            top,
            right: left + 24,
            bottom: top + 24,
        }
    }

    #[test]
    fn sits_above_bottom_taskbar() {
        assert_eq!(position(icon(1700, 1044), WORK, 320, 160, 12), (1552, 860));
    }

    #[test]
    fn stays_inside_screen() {
        assert_eq!(position(icon(1890, 1044), WORK, 320, 160, 12), (1588, 860));
    }

    #[test]
    fn sits_below_top_taskbar() {
        let work = RECT { top: 48, ..WORK };
        assert_eq!(position(icon(1700, 12), work, 320, 160, 12), (1552, 60));
    }

    #[test]
    fn sits_above_icon_in_overflow_menu() {
        assert_eq!(position(icon(1700, 900), WORK, 320, 160, 12), (1552, 728));
    }
}
