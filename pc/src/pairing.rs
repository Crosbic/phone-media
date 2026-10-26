use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, KeyInit, Mac};
use p256::PublicKey;
use p256::ecdh::EphemeralSecret;
use p256::elliptic_curve::Generate;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use windows::Win32::UI::Shell::FOLDERID_RoamingAppData;

use crate::{dialog, known_folder, log};

const CODE_TIMEOUT: Duration = Duration::from_secs(110);

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Pairings {
    pc_id: String,
    phones: BTreeMap<String, PairedPhone>,
}

#[derive(Serialize, Deserialize)]
struct PairedPhone {
    name: String,
    key: String,
}

impl Pairings {
    pub fn load() -> Self {
        let mut pairings: Self = fs::read(Self::path())
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        if pairings.pc_id.is_empty() {
            pairings.pc_id = hex(&random::<16>());
            if let Err(error) = pairings.save() {
                log(format!(
                    "не удалось сохранить {}: {error}",
                    Self::path().display()
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
            },
        );
        self.save()
            .map_err(|error| format!("не удалось сохранить ключ сопряжения: {error}"))?;
        log(format!("телефон «{phone_name}» сопряжён"));
        Ok(true)
    }

    fn key_for(&self, phone_id: &str) -> Option<Vec<u8>> {
        BASE64.decode(&self.phones.get(phone_id)?.key).ok()
    }

    fn path() -> PathBuf {
        known_folder(&FOLDERID_RoamingAppData)
            .join("phone-media")
            .join("pairings.json")
    }

    fn save(&self) -> io::Result<()> {
        let path = Self::path();
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        fs::write(path, serde_json::to_vec_pretty(self)?)
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
