use std::fs;
use std::process::ExitCode;

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::Generate;

const MAGIC: &[u8; 8] = b"PMSIG1\0\0";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, key] if command == "generate" => generate(key),
        [key, file] => sign(key, file),
        _ => Err("использование: sign generate <файл ключа> | sign <файл ключа> <exe>".to_string()),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn generate(path: &str) -> Result<String, String> {
    if fs::metadata(path).is_ok() {
        return Err(format!("{path} уже есть — не трогаю"));
    }
    let key = SigningKey::generate();
    fs::write(path, hex(&key.to_bytes())).map_err(|error| format!("{path}: {error}"))?;
    let public = key.verifying_key().to_sec1_point(false);
    Ok(format!(
        "ключ создан: {path}\nоткрытый ключ: {}",
        hex(public.as_bytes())
    ))
}

fn sign(key_path: &str, file: &str) -> Result<String, String> {
    let text = fs::read_to_string(key_path).map_err(|error| format!("{key_path}: {error}"))?;
    let key = SigningKey::from_slice(&unhex(text.trim())?)
        .map_err(|_| format!("{key_path}: неверный ключ"))?;
    let mut data = fs::read(file).map_err(|error| format!("{file}: {error}"))?;
    if data.ends_with(MAGIC) {
        return Err(format!("{file} уже подписан"));
    }
    let signature: Signature = key.sign(&data);
    data.extend_from_slice(&signature.to_bytes());
    data.extend_from_slice(MAGIC);
    fs::write(file, data).map_err(|error| format!("{file}: {error}"))?;
    Ok(format!("подписан: {file}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2).unwrap_or(""), 16))
        .collect::<Result<_, _>>()
        .map_err(|_| "ключ должен быть в шестнадцатеричном виде".to_string())
}
