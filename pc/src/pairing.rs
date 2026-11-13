use std::collections::BTreeMap;
use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, KeyInit, Mac};
use p256::PublicKey;
use p256::ecdh::EphemeralSecret;
use p256::elliptic_curve::Generate;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use windows::Win32::UI::Shell::FOLDERID_RoamingAppData;

use crate::{dialog, known_folder, log};

const CODE_TIMEOUT: Duration = Duration::from_secs(110);
const READ_ATTEMPTS: u32 = 5;
const READ_RETRY: Duration = Duration::from_secs(1);
const BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Pairings {
    pc_id: String,
    #[serde(default)]
    phones: BTreeMap<String, PairedPhone>,
    #[serde(skip)]
    file: PathBuf,
    #[serde(skip)]
    writable: bool,
    #[serde(skip)]
    unsaved: bool,
}

#[derive(Serialize, Deserialize)]
struct PairedPhone {
    name: String,
    key: String,
    #[serde(
        default,
        deserialize_with = "lenient_address",
        skip_serializing_if = "Option::is_none"
    )]
    address: Option<Ipv4Addr>,
}

fn lenient_address<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Ipv4Addr>, D::Error> {
    Ok(Value::deserialize(deserializer)?
        .as_str()
        .and_then(|text| text.parse().ok()))
}

impl Pairings {
    pub fn load() -> Self {
        let file = Self::path();
        let mut data = fs::read(&file);
        for _ in 1..READ_ATTEMPTS {
            match &data {
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    thread::sleep(READ_RETRY);
                    data = fs::read(&file);
                }
                _ => break,
            }
        }
        Self::from_read(file, data)
    }

    fn from_read(file: PathBuf, data: io::Result<Vec<u8>>) -> Self {
        let mut pairings = match data {
            Ok(data) => {
                match serde_json::from_slice::<Self>(data.strip_prefix(BOM).unwrap_or(&data)) {
                    Ok(pairings) => Self {
                        writable: true,
                        ..pairings
                    },
                    Err(error) => Self {
                        writable: set_aside(&file, error),
                        ..Self::default()
                    },
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self {
                writable: true,
                ..Self::default()
            },
            Err(error) => {
                log(format!(
                    "не удалось прочитать {}: {error}; до перезапуска программы сопряжения не сохраняются, а файл не перезаписывается",
                    file.display()
                ));
                Self::default()
            }
        };
        pairings.file = file;
        if pairings.pc_id.is_empty() {
            pairings.pc_id = hex(&random::<16>());
            if pairings.writable
                && let Err(error) = pairings.save()
            {
                log(format!(
                    "не удалось сохранить {}: {error}",
                    pairings.file.display()
                ));
            }
        }
        pairings
    }

    pub fn has_any(&self) -> bool {
        !self.phones.is_empty()
    }

    pub fn knows(&self, phone_id: &str) -> bool {
        self.phones.contains_key(phone_id)
    }

    pub fn authenticate(
        &mut self,
        reader: &mut BufReader<TcpStream>,
        writer: &mut TcpStream,
        phone_name: &str,
        pair_new: bool,
    ) -> Result<bool, Failure> {
        let pc_nonce = random::<16>();
        send(
            writer,
            json!({ "type": "hello", "pcId": self.pc_id, "name": pc_name(), "nonce": BASE64.encode(pc_nonce) }),
        )?;
        let reply = receive(reader)?;
        let phone_id = text(&reply, "phoneId")?.to_string();
        match reply["type"].as_str() {
            Some("auth") => {
                if let Some(key) = self.key_for(&phone_id) {
                    let phone_nonce = bytes(&reply, "nonce")?;
                    let proof = bytes(&reply, "proof")?;
                    if verify(&key, &[b"phone", &pc_nonce, &phone_nonce], &proof) {
                        let proof = hmac(&key, &[b"pc", &pc_nonce, &phone_nonce]);
                        send(
                            writer,
                            json!({ "type": "auth", "proof": BASE64.encode(proof) }),
                        )?;
                        return Ok(false);
                    }
                    if !pair_new {
                        return Err(Failure::Error(format!(
                            "телефон «{phone_name}» не подтвердил ключ сопряжения; \
                             если приложение переустанавливали, выберите в трее «Сопрячь новый телефон»"
                        )));
                    }
                }
                self.pair(reader, writer, phone_id, phone_name)
            }
            Some("unpaired") if reply["canPair"] == true => {
                self.pair(reader, writer, phone_id, phone_name)
            }
            Some("unpaired") => Err(closed_window(phone_name).into()),
            _ => Err(format!("неожиданный ответ телефона: {reply}").into()),
        }
    }

    fn pair(
        &mut self,
        reader: &mut BufReader<TcpStream>,
        writer: &mut TcpStream,
        phone_id: String,
        phone_name: &str,
    ) -> Result<bool, Failure> {
        let secret = EphemeralSecret::generate();
        let pc_key = PublicKey::from(&secret).to_sec1_bytes();
        send(
            writer,
            json!({ "type": "pair", "commit": BASE64.encode(Sha256::digest(&pc_key)) }),
        )?;
        let reply = receive(reader)?;
        if reply["type"] == "pairClosed" {
            return Err(closed_window(phone_name).into());
        }
        expect(&reply, "pairKey")?;
        let phone_key = bytes(&reply, "key")?;
        let phone_public = PublicKey::from_sec1_bytes(&phone_key)
            .map_err(|_| Failure::Error("телефон прислал неверный ключ".into()))?;
        send(
            writer,
            json!({ "type": "pairReveal", "key": BASE64.encode(&pc_key) }),
        )?;

        let shared = secret.diffie_hellman(&phone_public);
        let shared = shared.raw_secret_bytes().as_slice();
        let key = hmac(shared, &[b"phone-media key", &pc_key, &phone_key]);
        let code = code(shared, &pc_key, &phone_key);

        log(format!(
            "сопряжение с телефоном «{phone_name}»: жду код в окне"
        ));
        let Some(typed) = dialog::ask_code(phone_name, CODE_TIMEOUT) else {
            let _ = send(writer, json!({ "type": "pairFailed" }));
            return Err(Failure::Declined);
        };
        if typed != code {
            let _ = send(writer, json!({ "type": "pairFailed" }));
            return Err("код не совпал — сопряжение отменено, сейчас попробуем снова".into());
        }
        let proof = hmac(&key, &[b"confirm"]);
        send(
            writer,
            json!({ "type": "pairConfirm", "proof": BASE64.encode(proof) }),
        )?;
        expect(&receive(reader)?, "paired")?;

        self.phones.insert(
            phone_id,
            PairedPhone {
                name: phone_name.to_string(),
                key: BASE64.encode(key),
                address: None,
            },
        );
        self.save()
            .map_err(|error| format!("не удалось сохранить ключ сопряжения: {error}"))?;
        log(format!("телефон «{phone_name}» сопряжён"));
        Ok(true)
    }

    pub fn addresses(&self) -> Vec<Ipv4Addr> {
        self.phones
            .values()
            .filter_map(|phone| phone.address)
            .collect()
    }

    pub fn remember(&mut self, phone_id: &str, address: Ipv4Addr) {
        let changed = self.assign(phone_id, address);
        if !self.writable || !(changed || self.unsaved) {
            return;
        }
        if let Err(error) = self.save() {
            log(format!("не удалось запомнить адрес телефона: {error}"));
        }
    }

    fn assign(&mut self, phone_id: &str, address: Ipv4Addr) -> bool {
        if !self.phones.contains_key(phone_id) {
            return false;
        }
        let mut changed = false;
        for (id, phone) in &mut self.phones {
            let own = id == phone_id;
            if own && phone.address != Some(address) {
                phone.address = Some(address);
                changed = true;
            } else if !own && phone.address == Some(address) {
                phone.address = None;
                changed = true;
            }
        }
        changed
    }

    fn key_for(&self, phone_id: &str) -> Option<Vec<u8>> {
        BASE64.decode(&self.phones.get(phone_id)?.key).ok()
    }

    fn path() -> PathBuf {
        known_folder(&FOLDERID_RoamingAppData)
            .join("phone-media")
            .join("pairings.json")
    }

    fn save(&mut self) -> io::Result<()> {
        if !self.writable {
            return Err(io::Error::other(
                "при запуске файл не прочитался, поэтому программа его не перезаписывает",
            ));
        }
        let result = self.write();
        self.unsaved = result.is_err();
        result
    }

    fn write(&self) -> io::Result<()> {
        if let Some(folder) = self.file.parent() {
            fs::create_dir_all(folder)?;
        }
        let temporary = self.file.with_extension("json.tmp");
        let result = write_synced(&temporary, &serde_json::to_vec_pretty(self)?)
            .and_then(|()| fs::rename(&temporary, &self.file));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn write_synced(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(data)?;
    file.sync_all()
}

fn set_aside(path: &Path, error: impl Display) -> bool {
    let aside = free_aside_name(path);
    match fs::rename(path, &aside) {
        Ok(()) => {
            log(format!(
                "не удалось прочитать {}: {error}; файл переименован в {}, телефоны придётся сопрячь заново",
                path.display(),
                aside.display()
            ));
            true
        }
        Err(rename) => {
            log(format!(
                "не удалось прочитать {}: {error}; переименовать его тоже не удалось ({rename}), поэтому до перезапуска программы сопряжения не сохраняются, а файл не перезаписывается",
                path.display()
            ));
            false
        }
    }
}

fn free_aside_name(path: &Path) -> PathBuf {
    let mut number = 1;
    loop {
        let name = match number {
            1 => "pairings.bad.json".to_string(),
            _ => format!("pairings.bad.{number}.json"),
        };
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
        number += 1;
    }
}

fn closed_window(phone_name: &str) -> String {
    format!(
        "чтобы сопрячь телефон «{phone_name}» с этим ПК, откройте на нём приложение Phone Media"
    )
}

pub enum Failure {
    Declined,
    Error(String),
}

impl From<String> for Failure {
    fn from(error: String) -> Self {
        Self::Error(error)
    }
}

impl From<&str> for Failure {
    fn from(error: &str) -> Self {
        Self::Error(error.to_string())
    }
}

fn code(shared: &[u8], pc_key: &[u8], phone_key: &[u8]) -> String {
    let digest = hmac(shared, &[b"phone-media code", pc_key, phone_key]);
    let number = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    format!("{:06}", number % 1_000_000)
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC принимает ключ любой длины");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().to_vec()
}

fn verify(key: &[u8], parts: &[&[u8]], proof: &[u8]) -> bool {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC принимает ключ любой длины");
    for part in parts {
        mac.update(part);
    }
    mac.verify_slice(proof).is_ok()
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("системный генератор случайных чисел недоступен");
    bytes
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn pc_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "ПК".to_string())
}

fn send(writer: &mut TcpStream, message: Value) -> Result<(), String> {
    writer
        .write_all(format!("{message}\n").as_bytes())
        .map_err(|error| format!("связь: {error}"))
}

fn receive(reader: &mut BufReader<TcpStream>) -> Result<Value, String> {
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => Err("телефон закрыл соединение".into()),
        Ok(_) => serde_json::from_str(&line)
            .map_err(|error| format!("непонятный ответ телефона: {error}")),
        Err(error) => Err(format!("связь: {error}")),
    }
}

fn expect(message: &Value, kind: &str) -> Result<(), String> {
    if message["type"] == kind {
        Ok(())
    } else {
        Err(format!("ждали {kind}, а телефон прислал {message}"))
    }
}

fn text<'a>(message: &'a Value, field: &str) -> Result<&'a str, String> {
    message[field]
        .as_str()
        .ok_or_else(|| format!("в ответе телефона нет поля {field}"))
}

fn bytes(message: &Value, field: &str) -> Result<Vec<u8>, String> {
    BASE64
        .decode(text(message, field)?)
        .map_err(|_| format!("поле {field} не в base64"))
}

#[cfg(test)]
mod tests {
    use std::os::windows::fs::OpenOptionsExt;

    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("pairings-tests")
            .join(name);
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn load(file: &Path) -> Pairings {
        Pairings::from_read(file.to_path_buf(), fs::read(file))
    }

    fn json(file: &Path) -> Value {
        serde_json::from_slice(&fs::read(file).unwrap()).unwrap()
    }

    fn phone(address: Option<Ipv4Addr>) -> PairedPhone {
        PairedPhone {
            name: "телефон".to_string(),
            key: String::new(),
            address,
        }
    }

    #[test]
    fn address_belongs_to_one_phone() {
        let address = Ipv4Addr::new(192, 168, 1, 234);
        let mut pairings = Pairings {
            pc_id: "pc".to_string(),
            phones: BTreeMap::from([
                ("old".to_string(), phone(Some(address))),
                ("new".to_string(), phone(None)),
            ]),
            ..Pairings::default()
        };
        assert!(!pairings.assign("unknown", address));
        assert_eq!(pairings.phones["old"].address, Some(address));
        assert!(pairings.assign("new", address));
        assert_eq!(pairings.phones["old"].address, None);
        assert_eq!(pairings.phones["new"].address, Some(address));
        assert!(!pairings.assign("new", address));
        assert_eq!(pairings.addresses(), vec![address]);
    }

    #[test]
    fn reads_and_writes_addresses_leniently() {
        let file = folder("lenient").join("pairings.json");
        let text = r#"{"pcId":"pc","phones":{
            "a":{"name":"n","key":"k"},
            "b":{"name":"n","key":"k","address":"192.168.1.234"},
            "c":{"name":"n","key":"k","address":"192.168.1.5:47801"},
            "d":{"name":"n","key":"k","address":5}}}"#;
        fs::write(&file, [BOM, text.as_bytes()].concat()).unwrap();
        let mut pairings = load(&file);
        assert!(pairings.writable);
        assert_eq!(pairings.pc_id, "pc");
        assert_eq!(pairings.addresses(), vec![Ipv4Addr::new(192, 168, 1, 234)]);
        pairings.remember("a", Ipv4Addr::new(192, 168, 1, 50));
        let saved = json(&file);
        assert_eq!(saved["phones"]["a"]["address"], "192.168.1.50");
        assert_eq!(saved["phones"]["b"]["address"], "192.168.1.234");
        assert!(saved["phones"]["c"].is_object());
        assert!(saved["phones"]["c"].get("address").is_none());
        assert!(!file.with_extension("json.tmp").exists());
    }

    #[test]
    fn keeps_identity_without_phones() {
        let file = folder("identity").join("pairings.json");
        fs::write(&file, r#"{"pcId":"pc"}"#).unwrap();
        let pairings = load(&file);
        assert_eq!(pairings.pc_id, "pc");
        assert!(!pairings.has_any());
        assert!(!file.with_file_name("pairings.bad.json").exists());
    }

    #[test]
    fn starts_fresh_without_file() {
        let file = folder("fresh").join("pairings.json");
        let pairings = load(&file);
        assert!(pairings.writable);
        assert_eq!(json(&file)["pcId"], pairings.pc_id.as_str());
    }

    #[test]
    fn broken_file_is_set_aside_under_free_name() {
        let folder = folder("broken");
        let file = folder.join("pairings.json");
        fs::write(folder.join("pairings.bad.json"), "earlier").unwrap();
        fs::write(&file, "{broken").unwrap();
        let pairings = load(&file);
        assert!(pairings.writable);
        assert_eq!(
            fs::read_to_string(folder.join("pairings.bad.json")).unwrap(),
            "earlier"
        );
        assert_eq!(
            fs::read_to_string(folder.join("pairings.bad.2.json")).unwrap(),
            "{broken"
        );
        assert_eq!(json(&file)["pcId"], pairings.pc_id.as_str());
    }

    #[test]
    fn broken_file_that_cannot_be_moved_is_left_alone() {
        let file = folder("locked").join("pairings.json");
        fs::write(&file, "{broken").unwrap();
        let holder = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&file)
            .unwrap();
        let mut pairings = load(&file);
        assert!(!pairings.writable);
        assert!(pairings.save().is_err());
        drop(holder);
        assert_eq!(fs::read_to_string(&file).unwrap(), "{broken");
        assert!(!file.with_file_name("pairings.bad.json").exists());
    }

    #[test]
    fn unreadable_file_is_never_overwritten() {
        let file = folder("unreadable").join("pairings.json");
        fs::write(&file, "kept").unwrap();
        let mut pairings =
            Pairings::from_read(file.clone(), Err(io::ErrorKind::PermissionDenied.into()));
        assert!(!pairings.writable);
        assert!(!pairings.pc_id.is_empty());
        pairings.phones.insert("a".to_string(), phone(None));
        pairings.remember("a", Ipv4Addr::new(192, 168, 1, 234));
        assert!(pairings.save().is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "kept");
        assert!(!file.with_extension("json.tmp").exists());
    }
}
