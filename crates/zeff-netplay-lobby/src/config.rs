use std::{
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use zeff_netplay_protocol::IceServer;

pub struct Config {
    pub bind: SocketAddr,
    pub max_rooms: usize,
    pub max_connections: usize,
    pub room_ttl: Duration,
    pub origins: Vec<String>,
    pub ice_servers: Vec<IceServer>,
    pub turn: Option<Turn>,
    token_hash: [u8; 32],
}

pub struct Turn {
    urls: Vec<String>,
    secret: String,
}

impl Config {
    pub fn local(token: &str) -> Result<Self> {
        ensure!(
            (32..=256).contains(&token.len()),
            "access token must be 32..256 bytes"
        );
        Ok(Self {
            bind: "127.0.0.1:8080".parse()?,
            max_rooms: 4,
            max_connections: 16,
            room_ttl: Duration::from_secs(120),
            origins: Vec::new(),
            ice_servers: vec![IceServer {
                urls: vec!["stun:stun.cloudflare.com:3478".into()],
                username: None,
                credential: None,
            }],
            turn: None,
            token_hash: Sha256::digest(token.as_bytes()).into(),
        })
    }

    pub fn from_env() -> Result<Self> {
        let mut c = Self::local(
            &std::env::var("ZEFF_LOBBY_ACCESS_TOKEN")
                .map_err(|_| anyhow::anyhow!("set ZEFF_LOBBY_ACCESS_TOKEN (32..256 bytes)"))?,
        )?;
        c.bind = env("ZEFF_LOBBY_BIND", "127.0.0.1:8080").parse()?;
        c.max_rooms = number("ZEFF_LOBBY_MAX_ROOMS", 4, 1, 128)?;
        c.max_connections = number("ZEFF_LOBBY_MAX_CONNECTIONS", 16, 2, 256)?;
        c.room_ttl = Duration::from_secs(number("ZEFF_LOBBY_ROOM_TTL", 120, 10, 600)? as u64);
        c.origins = list("ZEFF_LOBBY_ORIGINS", "");
        ensure!(
            c.origins.len() <= 16
                && c.origins.iter().all(|s| s.len() <= 256
                    && (s.starts_with("https://") || s.starts_with("http://localhost:"))),
            "invalid browser origin allowlist"
        );
        let urls = list("ZEFF_LOBBY_STUN_URLS", "stun:stun.cloudflare.com:3478");
        ensure!(
            urls.len() <= 4 && urls.iter().all(|s| ice_url(s, false)),
            "invalid STUN URLs"
        );
        c.ice_servers = if urls.is_empty() {
            vec![]
        } else {
            vec![IceServer {
                urls,
                username: None,
                credential: None,
            }]
        };
        match env("ZEFF_LOBBY_ALLOW_TURN", "false").as_str() {
            "false" => {
                ensure!(
                    std::env::var_os("ZEFF_LOBBY_TURN_URLS").is_none()
                        && std::env::var_os("ZEFF_LOBBY_TURN_SECRET").is_none(),
                    "TURN configuration requires explicit ZEFF_LOBBY_ALLOW_TURN=true"
                );
            }
            "true" => {
                let urls = list("ZEFF_LOBBY_TURN_URLS", "");
                let secret = std::env::var("ZEFF_LOBBY_TURN_SECRET")?;
                ensure!(
                    !urls.is_empty()
                        && urls.len() <= 4
                        && urls.iter().all(|s| ice_url(s, true))
                        && (32..=256).contains(&secret.len()),
                    "invalid external TURN configuration"
                );
                c.turn = Some(Turn { urls, secret });
            }
            _ => bail!("ZEFF_LOBBY_ALLOW_TURN must be true or false"),
        }
        Ok(c)
    }

    pub fn authenticated(&self, token: &str) -> bool {
        if !(32..=256).contains(&token.len()) {
            return false;
        }
        let digest = Sha256::digest(token.as_bytes());
        digest
            .iter()
            .zip(self.token_hash)
            .fold(0u8, |diff, (a, b)| diff | (*a ^ b))
            == 0
    }

    pub fn ice(&self, room: &str, guest: bool) -> Result<Vec<IceServer>> {
        let mut servers = self.ice_servers.clone();
        if let Some(turn) = &self.turn {
            let expires = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 3600;
            let username = format!("{expires}:{room}:{}", u8::from(guest));
            let mut mac = Hmac::<Sha1>::new_from_slice(turn.secret.as_bytes())?;
            mac.update(username.as_bytes());
            servers.push(IceServer {
                urls: turn.urls.clone(),
                username: Some(username),
                credential: Some(STANDARD.encode(mac.finalize().into_bytes())),
            });
        }
        Ok(servers)
    }
}

fn env(name: &str, fallback: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| fallback.into())
}
fn list(name: &str, fallback: &str) -> Vec<String> {
    env(name, fallback)
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
fn number(name: &str, fallback: usize, min: usize, max: usize) -> Result<usize> {
    let value = env(name, &fallback.to_string()).parse::<usize>()?;
    ensure!((min..=max).contains(&value), "{name} out of range");
    Ok(value)
}
fn ice_url(url: &str, turn: bool) -> bool {
    let schemes: &[&str] = if turn {
        &["turn:", "turns:"]
    } else {
        &["stun:", "stuns:"]
    };
    url.len() <= 256
        && schemes
            .iter()
            .any(|s| url.strip_prefix(s).is_some_and(|r| !r.is_empty()))
        && !url
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control() || b == b'@')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_never_returns_turn_and_auth_is_exact() {
        let token = "a".repeat(32);
        let c = Config::local(&token).unwrap();
        assert!(c.authenticated(&token));
        assert!(!c.authenticated(&"b".repeat(32)));
        assert!(!c.authenticated("a"));
        assert!(c.turn.is_none());
        assert_eq!(c.ice("room", false).unwrap().len(), 1);
        assert!(!ice_url("turn:example:3478", false));
    }
    #[test]
    fn external_turn_credentials_are_expiring_and_role_scoped() {
        let mut c = Config::local(&"a".repeat(32)).unwrap();
        c.turn = Some(Turn {
            urls: vec!["turn:example:3478".into()],
            secret: "s".repeat(32),
        });
        let host = c.ice("room", false).unwrap().pop().unwrap();
        let guest = c.ice("room", true).unwrap().pop().unwrap();
        assert_ne!(host.username, guest.username);
        assert_ne!(host.credential, guest.credential);
        assert_eq!(STANDARD.decode(host.credential.unwrap()).unwrap().len(), 20);
        let expiry: u64 = host
            .username
            .unwrap()
            .split(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!((now + 3599..=now + 3601).contains(&expiry));
    }
}
