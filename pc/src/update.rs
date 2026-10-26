use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde::Deserialize;
use windows::Foundation::Uri;
use windows::Storage::Streams::DataReader;
use windows::Web::Http::Filters::{
    HttpBaseProtocolFilter, HttpCacheReadBehavior, HttpCacheWriteBehavior,
};
use windows::Web::Http::HttpClient;
use windows::core::{HSTRING, h};

use crate::{Event, log};

const LATEST: &str = "https://api.github.com/repos/Crosbic/phone-media/releases/latest";
const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const MAGIC: &[u8; 8] = b"PMSIG1\0\0";
const PUBLIC_KEY: &str = "04f0804af1a53425153cd3f380c1db92c5a5c0ae04503c43741585166951e321356e7d518ce7fa6b0eeaab138f58fe749a17e1506d5d90aac32bce2113f33757f1";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn spawn(events: Sender<Event>) {
    thread::spawn(move || {
        thread::sleep(FIRST_CHECK);
        loop {
            if let Err(error) = run(&events) {
                log(format!("проверка обновлений: {error}"));
            }
            thread::sleep(CHECK_EVERY);
        }
    });
}

pub fn check_now(events: Sender<Event>) {
    thread::spawn(move || match run(&events) {
        Ok(false) => log(format!("обновлений нет, версия {VERSION} последняя")),
        Ok(true) => {}
        Err(error) => log(format!("проверка обновлений: {error}")),
    });
}

pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(exe.with_extension("exe.old"));
        let _ = fs::remove_file(exe.with_extension("exe.new"));
    }
}

fn run(events: &Sender<Event>) -> Result<bool, String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Ok(true);
    }
    let result = update(events);
    BUSY.store(false, Ordering::SeqCst);
    result
}

fn update(events: &Sender<Event>) -> Result<bool, String> {
    let url = std::env::var("PHONE_MEDIA_UPDATE_URL").unwrap_or_else(|_| LATEST.to_string());
    let release: Release = serde_json::from_slice(&download(&url)?)
        .map_err(|error| format!("непонятный ответ о релизах: {error}"))?;
    let version = release.tag_name.trim_start_matches('v').to_string();
    if parse(&version) <= parse(VERSION) {
        return Ok(false);
    }
    let name = format!("PhoneMedia-{version}.exe");
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| format!("в релизе {version} нет файла {name}"))?;
    log(format!("доступна версия {version}, скачиваю"));
    let program = download(&asset.browser_download_url)?;
    verify(&program).map_err(|error| format!("версия {version} отклонена: {error}"))?;
    let exe =
        install(&program).map_err(|error| format!("не удалось заменить программу: {error}"))?;
    log(format!("версия {version} установлена, перезапускаюсь"));
    let _ = events.send(Event::Restart(exe));
    Ok(true)
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let fail = |error: windows::core::Error| format!("{url}: {}", error.message());
    let filter = HttpBaseProtocolFilter::new().map_err(fail)?;
    let cache = filter.CacheControl().map_err(fail)?;
    cache
        .SetReadBehavior(HttpCacheReadBehavior::NoCache)
        .map_err(fail)?;
    cache
        .SetWriteBehavior(HttpCacheWriteBehavior::NoCache)
        .map_err(fail)?;
    let client = HttpClient::Create(&filter).map_err(fail)?;
    client
        .DefaultRequestHeaders()
        .map_err(fail)?
        .TryAppendWithoutValidation(
            h!("User-Agent"),
            &HSTRING::from(format!("PhoneMedia/{VERSION}")),
        )
        .map_err(fail)?;
    let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(fail)?;
    let response = client.GetAsync(&uri).map_err(fail)?.join().map_err(fail)?;
    if !response.IsSuccessStatusCode().map_err(fail)? {
        return Err(format!(
            "{url}: HTTP {}",
            response.StatusCode().map_err(fail)?.0
        ));
    }
    let buffer = response
        .Content()
        .map_err(fail)?
        .ReadAsBufferAsync()
        .map_err(fail)?
        .join()
        .map_err(fail)?;
    let mut bytes = vec![0; buffer.Length().map_err(fail)? as usize];
    DataReader::FromBuffer(&buffer)
        .map_err(fail)?
        .ReadBytes(&mut bytes)
        .map_err(fail)?;
    Ok(bytes)
}

fn verify(program: &[u8]) -> Result<(), String> {
    let signed = program
        .strip_suffix(MAGIC.as_slice())
        .ok_or("файл не подписан")?;
    let split = signed.len().checked_sub(64).ok_or("файл повреждён")?;
    let (data, signature) = signed.split_at(split);
    let key = VerifyingKey::from_sec1_bytes(&unhex(PUBLIC_KEY))
        .map_err(|_| "встроенный ключ повреждён")?;
    let signature = Signature::from_slice(signature).map_err(|_| "подпись повреждена")?;
    key.verify(data, &signature)
        .map_err(|_| "подпись не совпадает".to_string())
}

fn install(program: &[u8]) -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let fresh = exe.with_extension("exe.new");
    let old = exe.with_extension("exe.old");
    fs::write(&fresh, program)?;
    let _ = fs::remove_file(&old);
    fs::rename(&exe, &old)?;
    if let Err(error) = fs::rename(&fresh, &exe) {
        let _ = fs::rename(&old, &exe);
        return Err(error);
    }
    Ok(exe)
}

fn parse(version: &str) -> (u32, u32, u32) {
    let mut parts = version.split('.').map(|part| part.parse().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .filter_map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(parse("1.2") > parse("1.1.0"));
        assert!(parse("1.10") > parse("1.9.9"));
        assert!(parse("1.1") == parse("1.1.0"));
        assert!(parse("2.0") > parse("1.99"));
    }

    #[test]
    fn rejects_unsigned_and_tampered() {
        assert!(verify(b"MZ unsigned program").is_err());
        let mut forged = b"MZ program".to_vec();
        forged.extend_from_slice(&[7; 64]);
        forged.extend_from_slice(MAGIC);
        assert!(verify(&forged).is_err());
    }
}
