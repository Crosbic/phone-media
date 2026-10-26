use std::path::PathBuf;

use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVAR_CHANGE_FLAGS, PROPVARIANT, PropVariantChangeType,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    FOLDERID_Programs, IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink,
};
use windows::core::{HSTRING, Interface, Result, w};

use crate::known_folder;

pub const APP_ID: &str = "Crosbic.PhoneMedia";

pub fn register() -> Result<()> {
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_ID))? };
    save_shortcut()
}

fn save_shortcut() -> Result<()> {
    let exe = std::env::current_exe().map_err(|error| {
        windows::core::Error::new(windows::core::HRESULT(-1), error.to_string())
    })?;
    let shortcut = start_menu().join("Phone Media.lnk");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        link.SetDescription(w!("Музыка с телефона в медиаплашке Windows"))?;

        let mut id = PROPVARIANT::default();
        PropVariantChangeType(
            &mut id,
            &PROPVARIANT::from(APP_ID),
            PROPVAR_CHANGE_FLAGS(0),
            VT_LPWSTR,
        )?;
        let properties: IPropertyStore = link.cast()?;
        properties.SetValue(&PKEY_AppUserModel_ID, &id)?;
        properties.Commit()?;

        link.cast::<IPersistFile>()?
            .Save(&HSTRING::from(shortcut.as_os_str()), true)
    }
}

fn start_menu() -> PathBuf {
    known_folder(&FOLDERID_Programs)
}
