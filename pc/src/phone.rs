use std::collections::{BTreeSet, HashSet};
use std::io::{self, BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use if_addrs::{IfAddr, Ifv4Addr};
use serde::Deserialize;

use crate::pairing::{Failure, Pairings};
use crate::{Event, log};

const DISCOVERY_PORT: u16 = 47800;
const QUERY: &str = "PHONEMEDIA? 2";
const ANSWER: &str = "PHONEMEDIA! 2";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
pub const PING_EVERY: Duration = Duration::from_secs(10);
const SILENCE_LIMIT: Duration = Duration::from_secs(PING_EVERY.as_secs() * 5 / 2);
const _: () = assert!(SILENCE_LIMIT.as_secs() > 2 * PING_EVERY.as_secs());
const UNICAST_EAGER: Duration = Duration::from_secs(120);
const UNICAST_EVERY: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PhoneMessage {
    Player(Player),
    Art { jpeg: Option<String> },
    Idle,
    Ping,
}

#[derive(Deserialize, Default, Clone)]
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
    pub volume: Option<u32>,
    pub volume_max: u32,
    pub muted: bool,
    pub volume_fixed: bool,
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
        let mut unicast = Unicast::new(Instant::now());
        loop {
            let pairing_now = pairing.load(Ordering::Relaxed);
            if pairing_now && !was_pairing {
                declined.clear();
            }
            was_pairing = pairing_now;

            let phone = discover(
                |networks| {
                    let known = if pairing.load(Ordering::Relaxed) {
                        Vec::new()
                    } else {
                        pairings.addresses()
                    };
                    unicast.select(Instant::now(), networks, known)
                },
                |id| {
                    if pairing.load(Ordering::Relaxed) {
                        !pairings.knows(id) && !declined.contains(id)
                    } else {
                        pairings.knows(id) || (!pairings.has_any() && !declined.contains(id))
                    }
                },
            );
            match connect(&phone, &mut pairings, pairing.load(Ordering::Relaxed)) {
                Ok((reader, link, paired)) => {
                    last_error.clear();
                    if let IpAddr::V4(address) = phone.address.ip() {
                        pairings.remember(&phone.id, address);
                    }
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
                    unicast = Unicast::new(Instant::now());
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
    let mut reader = BufReader::new(stream.try_clone().map_err(link)?);
    reader
        .get_ref()
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(link)?;
    let paired = pairings.authenticate(&mut reader, &mut stream, &phone.name, pair_new)?;
    reader
        .get_ref()
        .set_read_timeout(Some(SILENCE_LIMIT))
        .map_err(link)?;
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

struct Unicast {
    since: Instant,
    last: Option<Instant>,
    networks: BTreeSet<(Ipv4Addr, u8)>,
}

impl Unicast {
    fn new(now: Instant) -> Self {
        Self {
            since: now,
            last: None,
            networks: BTreeSet::new(),
        }
    }

    fn select(
        &mut self,
        now: Instant,
        networks: &[Ifv4Addr],
        known: Vec<Ipv4Addr>,
    ) -> Vec<Ipv4Addr> {
        let current: BTreeSet<_> = networks
            .iter()
            .map(|network| (network.ip, network.prefixlen))
            .collect();
        if current != self.networks {
            *self = Self {
                networks: current,
                ..Self::new(now)
            };
        }
        let reachable: Vec<Ipv4Addr> = known
            .into_iter()
            .filter(|&address| networks.iter().any(|network| inside(network, address)))
            .collect();
        if reachable.is_empty() || !self.due(now) {
            return Vec::new();
        }
        reachable
    }

    fn due(&mut self, now: Instant) -> bool {
        let due = now.duration_since(self.since) < UNICAST_EAGER
            || self
                .last
                .is_none_or(|last| now.duration_since(last) >= UNICAST_EVERY);
        if due {
            self.last = Some(now);
        }
        due
    }
}

fn discover(
    mut unicast: impl FnMut(&[Ifv4Addr]) -> Vec<Ipv4Addr>,
    accept: impl Fn(&str) -> bool,
) -> Found {
    loop {
        let networks = networks();
        let direct = unicast(&networks);
        match try_discover(&targets(&networks, &direct), &accept) {
            Ok(Some(found)) => return found,
            Ok(None) => {}
            Err(error) => {
                log(format!("поиск телефона: {error}"));
                thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

fn try_discover(
    targets: &BTreeSet<Ipv4Addr>,
    accept: &impl Fn(&str) -> bool,
) -> io::Result<Option<Found>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    for &target in targets {
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

fn networks() -> Vec<Ifv4Addr> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| match interface.addr {
            IfAddr::V4(v4) if !v4.ip.is_loopback() => Some(v4),
            _ => None,
        })
        .collect()
}

fn targets(networks: &[Ifv4Addr], direct: &[Ipv4Addr]) -> BTreeSet<Ipv4Addr> {
    let mut targets = BTreeSet::from([Ipv4Addr::BROADCAST]);
    targets.extend(networks.iter().filter_map(|network| network.broadcast));
    targets.extend(direct.iter().copied());
    targets
}

fn inside(network: &Ifv4Addr, address: Ipv4Addr) -> bool {
    let Some(mask) = u32::MAX.checked_shl(32 - u32::from(network.prefixlen.min(32))) else {
        return false;
    };
    address.to_bits() & mask == network.ip.to_bits() & mask
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(ip: [u8; 4], prefix: u8) -> Ifv4Addr {
        let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
        let ip = Ipv4Addr::from(ip);
        Ifv4Addr {
            ip,
            netmask: Ipv4Addr::from_bits(mask),
            prefixlen: prefix,
            broadcast: Some(Ipv4Addr::from_bits(ip.to_bits() | !mask)),
        }
    }

    #[test]
    fn unicast_only_inside_current_networks() {
        let start = Instant::now();
        let home = network([192, 168, 1, 220], 24);
        let tunnel = network([10, 8, 0, 2], 32);
        let phone = Ipv4Addr::new(192, 168, 1, 234);
        let known = vec![phone, Ipv4Addr::new(10, 8, 0, 5), phone];
        let networks = [home, tunnel];
        let direct = Unicast::new(start).select(start, &networks, known.clone());
        let expected = BTreeSet::from([
            Ipv4Addr::BROADCAST,
            Ipv4Addr::new(10, 8, 0, 2),
            Ipv4Addr::new(192, 168, 1, 255),
            phone,
        ]);
        assert_eq!(targets(&networks, &direct), expected);
        let unknown_mask = Ifv4Addr {
            netmask: Ipv4Addr::UNSPECIFIED,
            prefixlen: 0,
            ..network([192, 168, 1, 220], 24)
        };
        assert!(
            Unicast::new(start)
                .select(start, &[unknown_mask], known.clone())
                .is_empty()
        );
        assert!(Unicast::new(start).select(start, &[], known).is_empty());
    }

    #[test]
    fn unicast_backs_off_after_eager_period() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut unicast = Unicast::new(start);
        assert!(unicast.due(at(0)));
        assert!(unicast.due(at(118)));
        assert!(!unicast.due(at(120)));
        assert!(!unicast.due(at(147)));
        assert!(unicast.due(at(148)));
        assert!(!unicast.due(at(177)));
        assert!(unicast.due(at(178)));
        let mut late = Unicast::new(start);
        assert!(late.due(at(200)));
        assert!(!late.due(at(202)));
    }

    #[test]
    fn unicast_restarts_on_network_change_and_skips_unreachable_rounds() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let home = [network([192, 168, 1, 220], 24)];
        let phone = Ipv4Addr::new(192, 168, 1, 234);
        let mut unicast = Unicast::new(start);
        assert_eq!(unicast.select(at(0), &home, vec![phone]), vec![phone]);
        assert_eq!(unicast.select(at(130), &home, vec![phone]), vec![phone]);
        assert!(unicast.select(at(140), &home, vec![phone]).is_empty());
        assert!(unicast.select(at(150), &home, Vec::new()).is_empty());
        assert_eq!(unicast.select(at(160), &home, vec![phone]), vec![phone]);
        assert!(unicast.select(at(170), &[], vec![phone]).is_empty());
        assert_eq!(unicast.select(at(172), &home, vec![phone]), vec![phone]);
        assert_eq!(unicast.select(at(174), &home, vec![phone]), vec![phone]);
    }
}
