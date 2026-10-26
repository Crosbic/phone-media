use std::cell::{Cell, RefCell};
use std::time::Duration;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_WINDOW, CreateFontIndirectW, DeleteObject, FW_SEMIBOLD, GetSysColor, GetSysColorBrush,
    HDC, HFONT, SetBkColor,
};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::EM_SETLIMITTEXT;
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForSystem, SystemParametersInfoForDpi,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW, DC_HASDEFID, DM_GETDEFID, DefWindowProcW,
    DestroyWindow, DispatchMessageW, ES_CENTER, ES_NUMBER, GetMessageW, GetWindowTextW, HMENU,
    IDC_ARROW, IDCANCEL, IDOK, IsDialogMessageW, KillTimer, LoadCursorW, LoadIconW, MB_ICONWARNING,
    MSG, NONCLIENTMETRICSW, RegisterClassW, SPI_GETNONCLIENTMETRICS, SPI_GETWORKAREA, SW_SHOW,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW, SetForegroundWindow, SetTimer, ShowWindow,
    SystemParametersInfoW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORSTATIC, WM_SETFONT, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE,
    WS_EX_CONTROLPARENT, WS_EX_DLGMODALFRAME, WS_EX_TOPMOST, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, w};

const TIMER: usize = 1;
const CODE_LENGTH: usize = 6;

thread_local! {
    static ANSWER: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
    static EDIT: Cell<isize> = const { Cell::new(0) };
}

pub fn ask_code(phone_name: &str, timeout: Duration) -> Option<String> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: w!("PhoneMediaCode"),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            hIcon: LoadIconW(Some(instance), PCWSTR(1 as _)).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);

        let dpi = GetDpiForSystem();
        let px = |dips: i32| dips * dpi as i32 / 96;
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        let _ = SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            metrics.cbSize,
            Some((&raw mut metrics).cast()),
            0,
            dpi,
        );
        let font = CreateFontIndirectW(&metrics.lfMessageFont);
        let mut big = metrics.lfMessageFont;
        big.lfHeight *= 2;
        big.lfWeight = FW_SEMIBOLD.0 as i32;
        let big_font = CreateFontIndirectW(&big);

        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let ex_style = WS_EX_DLGMODALFRAME | WS_EX_TOPMOST | WS_EX_CONTROLPARENT;
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: px(420),
            bottom: px(200),
        };
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, ex_style, dpi);
        let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
        let mut area = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut area).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let window = CreateWindowExW(
            ex_style,
            w!("PhoneMediaCode"),
            w!("Phone Media — сопряжение"),
            style,
            area.left + (area.right - area.left - width) / 2,
            area.top + (area.bottom - area.top - height) / 2,
            width,
            height,
            None,
            None,
            Some(instance),
            None,
        )
        .ok()?;

        let child = |class: PCWSTR,
                     text: &str,
                     style: i32,
                     ex_style: WINDOW_EX_STYLE,
                     bounds: [i32; 4],
                     id: i32,
                     font: HFONT| {
            let [x, y, width, height] = bounds.map(px);
            let control = CreateWindowExW(
                ex_style,
                class,
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style as u32),
                x,
                y,
                width,
                height,
                Some(window),
                Some(HMENU(id as _)),
                Some(instance),
                None,
            )
            .unwrap_or_default();
            SendMessageW(
                control,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
            control
        };
        let text = format!(
            "Телефон «{phone_name}» хочет подключиться к этому ПК.\n\
             Введите код, который показывает приложение Phone Media на телефоне."
        );
        child(
            w!("STATIC"),
            &text,
            0,
            WINDOW_EX_STYLE(0),
            [20, 20, 380, 56],
            0,
            font,
        );
        let edit = child(
            w!("EDIT"),
            "",
            WS_TABSTOP.0 as i32 | ES_NUMBER | ES_CENTER,
            WS_EX_CLIENTEDGE,
            [130, 86, 160, 44],
            100,
            big_font,
        );
        SendMessageW(edit, EM_SETLIMITTEXT, Some(WPARAM(CODE_LENGTH)), None);
        let buttons = WS_TABSTOP.0 as i32;
        child(
            w!("BUTTON"),
            "Подключить",
            buttons | BS_DEFPUSHBUTTON,
            WINDOW_EX_STYLE(0),
            [150, 150, 120, 30],
            IDOK.0,
            font,
        );
        child(
            w!("BUTTON"),
            "Отмена",
            buttons | BS_PUSHBUTTON,
            WINDOW_EX_STYLE(0),
            [280, 150, 120, 30],
            IDCANCEL.0,
            font,
        );
        EDIT.set(edit.0 as isize);
        ANSWER.set(None);

        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(edit));
        SetTimer(Some(window), TIMER, timeout.as_millis() as u32, None);

        let mut message = MSG::default();
        while ANSWER.with_borrow(Option::is_none) && GetMessageW(&mut message, None, 0, 0).as_bool()
        {
            if !IsDialogMessageW(window, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let _ = KillTimer(Some(window), TIMER);
        let _ = DestroyWindow(window);
        let _ = DeleteObject(font.into());
        let _ = DeleteObject(big_font.into());
        ANSWER.take().flatten()
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_COMMAND if (wparam.0 & 0xffff) as i32 == IDOK.0 => {
                let mut text = [0u16; 16];
                let length = GetWindowTextW(HWND(EDIT.get() as _), &mut text);
                let code: String = String::from_utf16_lossy(&text[..length as usize])
                    .chars()
                    .filter(char::is_ascii_digit)
                    .collect();
                if code.len() == CODE_LENGTH {
                    ANSWER.set(Some(Some(code)));
                } else {
                    let _ = MessageBeep(MB_ICONWARNING);
                }
            }
            WM_COMMAND if (wparam.0 & 0xffff) as i32 == IDCANCEL.0 => ANSWER.set(Some(None)),
            WM_TIMER | WM_CLOSE => ANSWER.set(Some(None)),
            WM_CTLCOLORSTATIC => {
                SetBkColor(HDC(wparam.0 as _), COLORREF(GetSysColor(COLOR_WINDOW)));
                return LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize);
            }
            DM_GETDEFID => return LRESULT(((DC_HASDEFID as isize) << 16) | IDOK.0 as isize),
            _ => return DefWindowProcW(window, message, wparam, lparam),
        }
        LRESULT(0)
    }
}
