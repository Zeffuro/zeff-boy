use crate::config::Config;
use std::{collections::HashMap, time::Instant};
use tokio::sync::mpsc;
use zeff_netplay_protocol::{
    ClientMessage, ErrorCode, MAX_CANDIDATES, Role, ServerMessage, SessionIdentity, Signal,
    VERSION, valid_hex,
};

pub type Events = mpsc::Sender<ServerMessage>;
pub struct Membership {
    pub code: String,
    pub guest: bool,
}

struct Member {
    id: u64,
    tx: Events,
    candidates: usize,
    description: bool,
    finished: bool,
}
struct Room {
    identity: SessionIdentity,
    created: Instant,
    host: Member,
    guest: Option<Member>,
}

#[derive(Default)]
pub struct Rooms {
    rooms: HashMap<String, Room>,
}

impl Rooms {
    pub fn enter(
        &mut self,
        config: &Config,
        id: u64,
        tx: Events,
        message: ClientMessage,
    ) -> Result<(Membership, ServerMessage), ErrorCode> {
        self.expire(config);
        let (version, token, identity, requested) = match message {
            ClientMessage::Create {
                version,
                access_token,
                identity,
            } => (version, access_token, identity, None),
            ClientMessage::Join {
                version,
                access_token,
                identity,
                room,
            } => (version, access_token, identity, Some(room)),
            _ => return Err(ErrorCode::Invalid),
        };
        if !config.authenticated(&token) {
            return Err(ErrorCode::Unauthorized);
        }
        if version != VERSION {
            return Err(ErrorCode::Version);
        }
        if !identity.valid() {
            return Err(ErrorCode::Invalid);
        }
        let member = Member {
            id,
            tx,
            candidates: 0,
            description: false,
            finished: false,
        };
        let (code, guest) = if let Some(code) = requested {
            if !valid_hex(&code, 24) {
                return Err(ErrorCode::Invalid);
            }
            let room = self.rooms.get_mut(&code).ok_or(ErrorCode::Unavailable)?;
            if room.guest.is_some() {
                return Err(ErrorCode::Full);
            }
            if room.identity != identity {
                return Err(ErrorCode::Incompatible);
            }
            if room.host.tx.try_send(ServerMessage::PeerJoined).is_err() {
                self.rooms.remove(&code);
                return Err(ErrorCode::Unavailable);
            }
            room.guest = Some(member);
            (code, true)
        } else {
            if self.rooms.len() >= config.max_rooms {
                return Err(ErrorCode::Full);
            }
            let mut code = String::new();
            for _ in 0..4 {
                let mut entropy = [0; 12];
                getrandom::fill(&mut entropy).map_err(|_| ErrorCode::Unavailable)?;
                code = entropy.iter().map(|b| format!("{b:02x}")).collect();
                if !self.rooms.contains_key(&code) {
                    break;
                }
            }
            if self.rooms.contains_key(&code) {
                return Err(ErrorCode::Unavailable);
            }
            self.rooms.insert(
                code.clone(),
                Room {
                    identity,
                    created: Instant::now(),
                    host: member,
                    guest: None,
                },
            );
            (code, false)
        };
        let ice_servers = match config.ice(&code, guest) {
            Ok(servers) => servers,
            Err(_) => {
                self.remove(&code, id);
                return Err(ErrorCode::Unavailable);
            }
        };
        let welcome = ServerMessage::Welcome {
            version: VERSION,
            room: code.clone(),
            role: if guest { Role::Guest } else { Role::Host },
            ice_servers,
            relay_allowed: config.turn.is_some(),
        };
        Ok((Membership { code, guest }, welcome))
    }

    pub fn signal(
        &mut self,
        member: &Membership,
        id: u64,
        message: ClientMessage,
    ) -> Result<(), ErrorCode> {
        let room = self.rooms.get_mut(&member.code).ok_or(ErrorCode::Expired)?;
        let Room { host, guest, .. } = room;
        let guest = guest.as_mut().ok_or(ErrorCode::Unavailable)?;
        let (sender, other) = if member.guest {
            (guest, host)
        } else {
            (host, guest)
        };
        if sender.id != id || sender.finished {
            return Err(ErrorCode::Invalid);
        }
        match message {
            ClientMessage::Signal { signal } => {
                if !signal.valid() {
                    return Err(ErrorCode::Invalid);
                }
                match &signal {
                    Signal::Offer { .. } if !member.guest && !sender.description => {
                        sender.description = true
                    }
                    Signal::Answer { .. }
                        if member.guest && !sender.description && other.description =>
                    {
                        sender.description = true
                    }
                    Signal::Candidate { .. } if sender.candidates < MAX_CANDIDATES => {
                        sender.candidates += 1
                    }
                    _ => return Err(ErrorCode::Invalid),
                }
                other
                    .tx
                    .try_send(ServerMessage::Signal { signal })
                    .map_err(|_| ErrorCode::RateLimited)?;
            }
            ClientMessage::Finish {} if sender.description && other.description => {
                sender.finished = true;
                if other.finished {
                    let _ = other.tx.try_send(ServerMessage::Complete);
                    let _ = sender.tx.try_send(ServerMessage::Complete);
                    self.rooms.remove(&member.code);
                }
            }
            _ => return Err(ErrorCode::Invalid),
        }
        Ok(())
    }

    pub fn remove(&mut self, code: &str, id: u64) {
        let Some(room) = self.rooms.get(code) else {
            return;
        };
        if room.host.id != id && !room.guest.as_ref().is_some_and(|g| g.id == id) {
            return;
        }
        let room = self.rooms.remove(code).unwrap();
        let _ = room.host.tx.try_send(ServerMessage::PeerLeft);
        if let Some(guest) = room.guest {
            let _ = guest.tx.try_send(ServerMessage::PeerLeft);
        }
    }

    pub fn expire(&mut self, config: &Config) {
        self.rooms.retain(|_, room| {
            if room.created.elapsed() < config.room_ttl {
                return true;
            }
            let _ = room.host.tx.try_send(ServerMessage::Error {
                code: ErrorCode::Expired,
            });
            if let Some(guest) = &room.guest {
                let _ = guest.tx.try_send(ServerMessage::Error {
                    code: ErrorCode::Expired,
                });
            }
            false
        });
    }

    pub fn len(&self) -> usize {
        self.rooms.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_netplay_protocol::SessionMode;
    const TOKEN: &str = "unit-token-with-more-than-32-characters";
    fn identity() -> SessionIdentity {
        SessionIdentity {
            core: "ws".into(),
            content_hash: "0".repeat(64),
            compatibility_hash: "1".repeat(64),
            mode: SessionMode::LinkedDevices,
        }
    }
    fn enter(
        rooms: &mut Rooms,
        config: &Config,
        id: u64,
        code: Option<&str>,
    ) -> (Membership, mpsc::Receiver<ServerMessage>) {
        let (tx, rx) = mpsc::channel(8);
        let request = if let Some(code) = code {
            ClientMessage::Join {
                version: VERSION,
                access_token: TOKEN.into(),
                identity: identity(),
                room: code.into(),
            }
        } else {
            ClientMessage::Create {
                version: VERSION,
                access_token: TOKEN.into(),
                identity: identity(),
            }
        };
        (rooms.enter(config, id, tx, request).unwrap().0, rx)
    }
    #[test]
    fn role_description_candidate_and_backpressure_limits_fail_closed() {
        let config = Config::local(TOKEN).unwrap();
        let mut rooms = Rooms::default();
        let (host, mut h) = enter(&mut rooms, &config, 1, None);
        let (guest, mut g) = enter(&mut rooms, &config, 2, Some(&host.code));
        h.try_recv().unwrap();
        let offer = || ClientMessage::Signal {
            signal: Signal::Offer {
                sdp: "offer".into(),
            },
        };
        let answer = || ClientMessage::Signal {
            signal: Signal::Answer {
                sdp: "answer".into(),
            },
        };
        assert_eq!(rooms.signal(&guest, 2, offer()), Err(ErrorCode::Invalid));
        assert_eq!(rooms.signal(&guest, 2, answer()), Err(ErrorCode::Invalid));
        rooms.signal(&host, 1, offer()).unwrap();
        g.try_recv().unwrap();
        assert_eq!(rooms.signal(&host, 1, offer()), Err(ErrorCode::Invalid));
        rooms.signal(&guest, 2, answer()).unwrap();
        h.try_recv().unwrap();
        let candidate = || ClientMessage::Signal {
            signal: Signal::Candidate {
                candidate: "candidate:1 1 udp 1 127.0.0.1 1 typ host".into(),
                sdp_mid: None,
                sdp_m_line_index: None,
            },
        };
        for _ in 0..MAX_CANDIDATES {
            rooms.signal(&host, 1, candidate()).unwrap();
            g.try_recv().unwrap();
        }
        assert_eq!(rooms.signal(&host, 1, candidate()), Err(ErrorCode::Invalid));
        for _ in 0..8 {
            rooms.signal(&guest, 2, candidate()).unwrap();
        }
        assert_eq!(
            rooms.signal(&guest, 2, candidate()),
            Err(ErrorCode::RateLimited)
        );
        rooms.remove(&host.code, 999);
        assert_eq!(rooms.len(), 1);
        rooms.remove(&host.code, 1);
        assert_eq!(rooms.len(), 0);
    }
    #[test]
    fn room_expiry_evicts_idle_entries_and_notifies_both_members() {
        let config = Config::local(TOKEN).unwrap();
        let mut rooms = Rooms::default();
        let (host, mut h) = enter(&mut rooms, &config, 1, None);
        let (_, mut g) = enter(&mut rooms, &config, 2, Some(&host.code));
        h.try_recv().unwrap();
        rooms.rooms.get_mut(&host.code).unwrap().created = Instant::now() - config.room_ttl;
        rooms.expire(&config);
        assert_eq!(rooms.len(), 0);
        for queue in [&mut h, &mut g] {
            assert_eq!(
                queue.try_recv().unwrap(),
                ServerMessage::Error {
                    code: ErrorCode::Expired
                }
            );
        }
    }
}
