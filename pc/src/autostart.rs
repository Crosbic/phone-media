use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const NAME: &str = "Phone Media";

pub fn is_enabled() -> bool {
    read(RUN_KEY, NAME).is_some()
}

pub fn set(enabled: bool) -> Result<()> {
    if enabled {
        let exe = std::env::current_exe().unwrap_or_default();
        write(RUN_KEY, NAME, &format!("\"{}\"", exe.display()))
    } else {
        delete(RUN_KEY, NAME)
    }
}

fn read(key: PCWSTR, name: &str) -> Option<String> {
    let name = HSTRING::from(name);
    let mut size = 0u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            &name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    let mut buffer = vec![0u16; size as usize / 2];
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            &name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    let text = &buffer[..size as usize / 2];
    Some(String::from_utf16_lossy(
        text.strip_suffix(&[0]).unwrap_or(text),
    ))
}

fn write(key: PCWSTR, name: &str, value: &str) -> Result<()> {
    let wide: Vec<u16> = value.encode_utf16().chain([0]).collect();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key,
            &HSTRING::from(name),
            REG_SZ.0,
            Some(wide.as_ptr().cast()),
            (wide.len() * 2) as u32,
        )
    }
    .ok()
}

fn delete(key: PCWSTR, name: &str) -> Result<()> {
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, &HSTRING::from(name)) } {
        ERROR_FILE_NOT_FOUND => Ok(()),
        result => result.ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::RegDeleteKeyW;

    #[test]
    fn write_read_delete() {
        let key = w!(r"Software\Crosbic\PhoneMediaTest");
        write(key, NAME, r#""C:\путь с пробелами\phone-media.exe""#).unwrap();
        assert_eq!(
            read(key, NAME).as_deref(),
            Some(r#""C:\путь с пробелами\phone-media.exe""#)
        );
        delete(key, NAME).unwrap();
        assert_eq!(read(key, NAME), None);
        delete(key, NAME).unwrap();
        let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, key) };
        let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, w!(r"Software\Crosbic")) };
    }
}
