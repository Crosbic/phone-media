use std::collections::{BTreeSet, HashSet};
use std::io::{self, BufRead, BufReader};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use crate::pairing::{Failure, Pairings};
use crate::{Event, log};

const DISCOVERY_PORT: u16 = 47800;
const QUERY: &str = "PHONEMEDIA? 2";
const ANSWER: &str = "PHONEMEDIA! 2";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const SILENCE_LIMIT: Duration = Duration::from_secs(25);

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PhoneMessage {
    Player(Player),
    Art { jpeg: Option<String> },
    Idle,
    Ping,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Player {
    pub app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub actions: Vec<String>,
}

impl Player {
    pub fn can(&self, action: &str) -> bool {
        self.actions.iter().any(|a| a == action)
    }
}

struct Found {
    id: String,
    name: String,
    address: SocketAddr,
}

pub fn spawn(events: Sender<Event>, pairing: Arc<AtomicBool>) {
    thread::spawn(move || {
        let mut pairings = Pairings::load();
        let mut declined = HashSet::new();
        let mut was_pairing = false;
        let mut last_error = String::new();
        loop {
            let pairing_now = pairing.load(Ordering::Relaxed);
            if pairing_now && !was_pairing {
                declined.clear();
            }
            was_pairing = pairing_now;

            let phone = discover(|id| {
                if pairing.load(Ordering::Relaxed) {
                    !pairings.knows(id) && !declined.contains(id)
                } else {
                    pairings.knows(id) || (!pairings.has_any() && !declined.contains(id))
                }
            });
            match connect(&phone, &mut pairings, pairing.load(Ordering::Relaxed)) {
                Ok((reader, link, paired)) => {
                    last_error.clear();
                    if paired {
                        pairing.store(false, Ordering::Relaxed);
                    }
                    let connected = Event::Connected {
                        name: phone.name,
                        address: phone.address,
                        link,
                    };
                    if events.send(connected).is_err() {
                        return;
                    }
                    read_messages(reader, &events);
                    if events.send(Event::Disconnected).is_err() {
                        return;
                    }
                }
                Err(Failure::Declined) => {
                    log(format!("сопряжение с телефоном «{}» отменено", phone.name));
                    declined.insert(phone.id);
                    last_error.clear();
                    if events.send(Event::CancelPairing).is_err() {
                        return;
                    }
                }
                Err(Failure::Error(error)) if error == last_error => {}
                Err(Failure::Error(error)) => {
                    log(&error);
                    last_error = error;
                }
            }
            thread::sleep(Duration::from_secs(3));
        }
    });
}

fn connect(
    phone: &Found,
    pairings: &mut Pairings,
    pair_new: bool,
) -> Result<(BufReader<TcpStream>, TcpStream, bool), Failure> {
    let link = |error: io::Error| Failure::Error(format!("связь с {}: {error}", phone.address));
    let mut stream =
        TcpStream::connect_timeout(&phone.address, Duration::from_secs(3)).map_err(link)?;
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(link)?;
    let mut reader = BufReader::new(stream.try_clone().map_err(link)?);
    let paired = pairings.authenticate(&mut reader, &mut stream, &phone.name, pair_new)?;
    stream.set_read_timeout(Some(SILENCE_LIMIT)).map_err(link)?;
    Ok((reader, stream, paired))
}

fn read_messages(reader: BufReader<TcpStream>, events: &Sender<Event>) {
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                log(format!(
                    "телефон молчит дольше {} с",
                    SILENCE_LIMIT.as_secs()
                ));
                return;
            }
            Err(error) => {
                log(format!("связь: {error}"));
                return;
            }
        };
        match serde_json::from_str(&line) {
            Ok(message) => {
                if events.send(Event::Message(message)).is_err() {
                    return;
                }
            }
            Err(error) => log(format!("непонятное сообщение от телефона: {error}")),
        }
    }
}

fn discover(accept: impl Fn(&str) -> bool) -> Found {
    loop {
        match try_discover(&accept) {
            Ok(Some(found)) => return found,
            Ok(None) => {}
            Err(error) => {
                log(format!("поиск телефона: {error}"));
                thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

fn try_discover(accept: &impl Fn(&str) -> bool) -> io::Result<Option<Found>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    for target in broadcast_addresses() {
        let _ = socket.send_to(QUERY.as_bytes(), (target, DISCOVERY_PORT));
    }
    let mut buffer = [0; 512];
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((size, from)) => {
                if let Some(found) = parse_answer(&buffer[..size], from)
                    && accept(&found.id)
                {
                    return Ok(Some(found));
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Ok(None);
            }
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
            Err(error) => return Err(error),
        }
    }
}

fn parse_answer(data: &[u8], from: SocketAddr) -> Option<Found> {
    let mut parts = std::str::from_utf8(data)
        .ok()?
        .strip_prefix(ANSWER)?
        .strip_prefix(' ')?
        .splitn(3, ' ');
    let port = parts.next()?.parse().ok()?;
    let id = parts.next()?.to_string();
    let name = parts.next().unwrap_or("телефон").to_string();
    Some(Found {
        id,
        name,
        address: SocketAddr::new(from.ip(), port),
    })
}

fn broadcast_addresses() -> BTreeSet<Ipv4Addr> {
    let mut targets = BTreeSet::from([Ipv4Addr::BROADCAST]);
    for interface in if_addrs::get_if_addrs().unwrap_or_default() {
        if let if_addrs::IfAddr::V4(v4) = interface.addr
            && !v4.ip.is_loopback()
            && let Some(broadcast) = v4.broadcast
        {
            targets.insert(broadcast);
        }
    }
    targets
}
