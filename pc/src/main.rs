#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod dialog;
mod flyout;
mod identity;
mod pairing;
mod phone;
mod tray;
mod update;

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::json;
use windows::Media::SystemMediaTransportControlsButton as Button;
use windows::Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
use windows::core::GUID;
use windows::core::w;

use flyout::Flyout;
use phone::PhoneMessage;
use tray::Tray;

pub enum Event {
    Button(Button),
    Seek(Duration),
    Connected {
        name: String,
        address: SocketAddr,
        link: TcpStream,
    },
    Message(PhoneMessage),
    Disconnected,
    PairNew,
    CancelPairing,
    CheckUpdates,
    Restart(PathBuf),
    Quit,
}

const SEARCHING: &str = "Ищу телефон…";
const WAITING_FOR_NEW: &str = "Жду новый телефон: откройте на нём Phone Media";

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let updated_from = args
        .iter()
        .position(|arg| arg == "--updated")
        .and_then(|index| args.get(index + 1).cloned());
    let wait = if updated_from.is_some() {
        Duration::from_secs(15)
    } else {
        Duration::ZERO
    };
    if !acquire_instance(wait) {
        unsafe {
            MessageBoxW(
                None,
                w!("Phone Media уже запущена — её значок в трее."),
                w!("Phone Media"),
                MB_ICONINFORMATION | MB_OK,
            )
        };
        return Ok(());
    }
    if let Err(error) = identity::register() {
        log(format!(
            "не удалось зарегистрировать имя программы для медиаплашки: {error}"
        ));
    }
    if cfg!(not(debug_assertions))
        && autostart::is_enabled()
        && let Err(error) = autostart::set(true)
    {
        log(format!("не удалось обновить автозапуск: {error}"));
    }
    update::cleanup();

    let (events, rx) = mpsc::channel();
    let pairing = Arc::new(AtomicBool::new(args.iter().any(|arg| arg == "--pair")));
    let tray = Tray::spawn(events.clone(), pairing.clone());
    let mut flyout = Flyout::new(events.clone())?;
    phone::spawn(events.clone(), pairing.clone());
    if cfg!(not(debug_assertions)) {
        update::spawn(events.clone());
    }
    let searching = |pairing: &AtomicBool| {
        if pairing.load(Ordering::Relaxed) {
            WAITING_FOR_NEW
        } else {
            SEARCHING
        }
    };
    log(searching(&pairing));
    tray.update(|status| status.phone = searching(&pairing).to_string());
    if let Some(old) = updated_from {
        log(format!("обновлено: {old} → {}", update::VERSION));
        tray.update(|status| {
            status.balloon = Some((
                format!("Phone Media обновлена до {}", update::VERSION),
                format!("Была версия {old}"),
            ))
        });
    }

    let mut link: Option<TcpStream> = None;
    let mut last_logged = String::new();
    for event in rx {
        let result = match event {
            Event::Connected {
                name,
                address,
                link: stream,
            } => {
                log(format!("подключено: {name} ({address})"));
                link = Some(stream);
                tray.update(|status| {
                    status.connected = true;
                    status.phone = format!("Телефон: {name}");
                });
                Ok(())
            }
            Event::Disconnected => {
                log("связь с телефоном потеряна, ищу снова…");
                link = None;
                last_logged.clear();
                tray.update(|status| {
                    status.connected = false;
                    status.phone = searching(&pairing).to_string();
                    status.track = None;
                });
                flyout.hide()
            }
            Event::Message(PhoneMessage::Player(player)) => {
                let (status, icon) = if player.playing {
                    ("играет", "▶")
                } else {
                    ("пауза", "⏸")
                };
                let line = format!(
                    "{}: {} — {} [{status}]",
                    player.app, player.artist, player.title
                );
                if line != last_logged {
                    log(&line);
                    last_logged = line;
                }
                tray.update(|status| {
                    status.track = Some(format!("{icon} {} — {}", player.artist, player.title))
                });
                flyout.show(&player)
            }
            Event::Message(PhoneMessage::Art { jpeg }) => {
                flyout.set_art(jpeg.and_then(|text| BASE64.decode(text).ok()))
            }
            Event::Message(PhoneMessage::Idle) => {
                log("на телефоне ничего не играет");
                last_logged.clear();
                tray.update(|status| status.track = None);
                flyout.hide()
            }
            Event::Message(PhoneMessage::Ping) => Ok(()),
            Event::Button(button) => {
                if let Some(action) = action_for(button) {
                    log(format!("кнопка: {action}"));
                    send(&mut link, json!({ "type": "command", "action": action }));
                }
                Ok(())
            }
            Event::Seek(position) => {
                let position_ms = position.as_millis() as u64;
                log(format!("перемотка на {} с", position_ms / 1000));
                send(
                    &mut link,
                    json!({ "type": "command", "action": "seek", "positionMs": position_ms }),
                );
                Ok(())
            }
            Event::PairNew => {
                log("сопряжение нового телефона: откройте на нём приложение Phone Media");
                pairing.store(true, Ordering::Relaxed);
                if let Some(stream) = &link {
                    let _ = stream.shutdown(Shutdown::Both);
                }
                tray.update(|status| status.phone = WAITING_FOR_NEW.to_string());
                Ok(())
            }
            Event::CancelPairing => {
                log("сопряжение отменено");
                pairing.store(false, Ordering::Relaxed);
                if link.is_none() {
                    tray.update(|status| status.phone = SEARCHING.to_string());
                }
                Ok(())
            }
            Event::CheckUpdates => {
                if cfg!(debug_assertions) {
                    log("в отладочной сборке обновления отключены");
                } else {
                    update::check_now(events.clone());
                }
                Ok(())
            }
            Event::Restart(exe) => {
                let _ = flyout.hide();
                tray.remove();
                if let Err(error) = Command::new(&exe)
                    .args(["--updated", update::VERSION])
                    .spawn()
                {
                    log(format!("не удалось запустить новую версию: {error}"));
                }
                return Ok(());
            }
            Event::Quit => {
                log("выход");
                let _ = flyout.hide();
                tray.remove();
                return Ok(());
            }
        };
        if let Err(error) = result {
            log(format!("медиаплашка: {error}"));
        }
    }
    Ok(())
}

fn acquire_instance(wait: Duration) -> bool {
    unsafe {
        let Ok(mutex) = CreateMutexW(None, false, w!(r"Local\Crosbic.PhoneMedia")) else {
            return true;
        };
        let result = WaitForSingleObject(mutex, wait.as_millis() as u32);
        result == WAIT_OBJECT_0 || result == WAIT_ABANDONED
    }
}

fn action_for(button: Button) -> Option<&'static str> {
    Some(match button {
        Button::Play => "play",
        Button::Pause => "pause",
        Button::Stop => "stop",
        Button::Next => "next",
        Button::Previous => "prev",
        _ => return None,
    })
}

fn send(link: &mut Option<TcpStream>, message: serde_json::Value) {
    let Some(stream) = link else {
        log("телефон не подключён");
        return;
    };
    if let Err(error) = stream.write_all(format!("{message}\n").as_bytes()) {
        log(format!("не удалось отправить команду: {error}"));
        *link = None;
    }
}

pub fn log_path() -> PathBuf {
    known_folder(&FOLDERID_LocalAppData)
        .join("phone-media")
        .join("phone-media.log")
}

pub fn known_folder(id: &GUID) -> PathBuf {
    unsafe {
        let Ok(path) = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) else {
            return PathBuf::new();
        };
        let folder = PathBuf::from(OsString::from_wide(path.as_wide()));
        CoTaskMemFree(Some(path.0 as _));
        folder
    }
}

pub fn log(message: impl AsRef<str>) {
    static FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    let now = unsafe { GetLocalTime() };
    let line = format!(
        "{:02}:{:02}:{:02}  {}",
        now.wHour,
        now.wMinute,
        now.wSecond,
        message.as_ref()
    );
    println!("{line}");
    let file = FILE.get_or_init(|| {
        let path = log_path();
        fs::create_dir_all(path.parent()?).ok()?;
        let updated = std::env::args().any(|arg| arg == "--updated");
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(updated)
            .truncate(!updated)
            .open(path)
            .ok()?;
        let _ = writeln!(
            file,
            "Phone Media {}, журнал за {:02}.{:02}.{}",
            env!("CARGO_PKG_VERSION"),
            now.wDay,
            now.wMonth,
            now.wYear
        );
        Some(Mutex::new(file))
    });
    if let Some(Ok(mut file)) = file.as_ref().map(Mutex::lock) {
        let _ = writeln!(file, "{line}");
    }
}
