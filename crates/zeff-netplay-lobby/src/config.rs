use std::{
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use zeff_netplay_protocol::{IceServer, MAX_ICE_URLS, valid_ice_url, validate_ice_servers};

pub struct Config {
    pub bind: SocketAddr,
    pub max_rooms: usize,
    pub max_connections: usize,
    pub room_ttl: Duration,
    pub origins: Vec<String>,
    pub ice_servers: Vec<IceServer>,
    pub turn: Option<Turn>,
    token_hash: Option<[u8; 32]>,
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
        Ok(Self::defaults(Some(
            Sha256::digest(token.as_bytes()).into(),
        )))
    }

    pub fn public() -> Self {
        Self::defaults(None)
    }

    fn defaults(token_hash: Option<[u8; 32]>) -> Self {
        Self {
            bind: ([127, 0, 0, 1], 8080).into(),
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
            token_hash,
        }
    }

    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(error) => Err(error.into()),
        })
    }

    fn from_lookup(lookup: impl Fn(&str) -> Result<Option<String>>) -> Result<Self> {
        let env = |name: &str, fallback: &str| -> Result<String> {
            Ok(lookup(name)?.unwrap_or_else(|| fallback.into()))
        };
        let public = boolean("ZEFF_LOBBY_PUBLIC", &env)?;
        let token = env("ZEFF_LOBBY_ACCESS_TOKEN", "")?;
        let mut c = if public {
            ensure!(
                token.is_empty() || (32..=256).contains(&token.len()),
                "access token must be empty or 32..256 bytes in public mode"
            );
            Self::public()
        } else {
            Self::local(&token)?
        };
        c.bind = env("ZEFF_LOBBY_BIND", "127.0.0.1:8080")?.parse()?;
        c.max_rooms = number("ZEFF_LOBBY_MAX_ROOMS", 4, 1, 128, &env)?;
        c.max_connections = number("ZEFF_LOBBY_MAX_CONNECTIONS", 16, 2, 256, &env)?;
        c.room_ttl = Duration::from_secs(number("ZEFF_LOBBY_ROOM_TTL", 120, 10, 600, &env)? as u64);
        c.origins = list(&env("ZEFF_LOBBY_ORIGINS", "")?);
        ensure!(
            c.origins.len() <= 16
                && c.origins.iter().all(|s| s.len() <= 256
                    && (s.starts_with("https://") || s.starts_with("http://localhost:"))),
            "invalid browser origin allowlist"
        );
        let urls = list(&env(
            "ZEFF_LOBBY_STUN_URLS",
            "stun:stun.cloudflare.com:3478",
        )?);
        ensure!(
            urls.len() <= MAX_ICE_URLS && urls.iter().all(|s| ice_url(s, false)),
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
        match boolean("ZEFF_LOBBY_ALLOW_TURN", &env)? {
            false => {
                ensure!(
                    lookup("ZEFF_LOBBY_TURN_URLS")?.is_none()
                        && lookup("ZEFF_LOBBY_TURN_SECRET")?.is_none(),
                    "TURN configuration requires explicit ZEFF_LOBBY_ALLOW_TURN=true"
                );
            }
            true => {
                ensure!(!public, "public lobbies cannot issue TURN credentials");
                let urls = list(&env("ZEFF_LOBBY_TURN_URLS", "")?);
                let secret = env("ZEFF_LOBBY_TURN_SECRET", "")?;
                ensure!(
                    !urls.is_empty()
                        && urls.len() <= MAX_ICE_URLS
                        && urls.iter().all(|s| ice_url(s, true))
                        && (32..=256).contains(&secret.len()),
                    "invalid external TURN configuration"
                );
                c.turn = Some(Turn { urls, secret });
            }
        }
        validate_ice_servers(&c.ice_servers, c.turn.is_some())?;
        Ok(c)
    }

    pub fn authenticated(&self, token: &str) -> bool {
        let Some(token_hash) = self.token_hash else {
            return token.len() <= 256;
        };
        if !(32..=256).contains(&token.len()) {
            return false;
        }
        let digest = Sha256::digest(token.as_bytes());
        digest
            .iter()
            .zip(token_hash)
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
        validate_ice_servers(&servers, self.turn.is_some())?;
        Ok(servers)
    }
}

fn boolean(name: &str, env: &impl Fn(&str, &str) -> Result<String>) -> Result<bool> {
    match env(name, "false")?.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => bail!("{name} must be true or false"),
    }
}
fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
fn number(
    name: &str,
    fallback: usize,
    min: usize,
    max: usize,
    env: &impl Fn(&str, &str) -> Result<String>,
) -> Result<usize> {
    let value = env(name, &fallback.to_string())?.parse::<usize>()?;
    ensure!((min..=max).contains(&value), "{name} out of range");
    Ok(value)
}
fn ice_url(url: &str, turn: bool) -> bool {
    valid_ice_url(url, turn) && (!turn || url.starts_with("turn:") || url.starts_with("turns:"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn configured(values: &[(&str, &str)]) -> Result<Config> {
        Config::from_lookup(|name| {
            Ok(values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).into()))
        })
    }

    #[test]
    fn generated_ice_respects_client_limits_and_keeps_turn_opt_in() {
        let mut config = Config::public();
        assert!(validate_ice_servers(&config.ice("room", false).unwrap(), false).is_ok());
        config.ice_servers[0].urls.push("turn:example.com".into());
        assert!(config.ice("room", false).is_err());
        config.ice_servers[0].urls = vec!["stun:example.com".into(); MAX_ICE_URLS + 1];
        assert!(config.ice("room", false).is_err());
        let token = "a".repeat(32);
        let private = configured(&[
            ("ZEFF_LOBBY_ACCESS_TOKEN", &token),
            ("ZEFF_LOBBY_ALLOW_TURN", "true"),
            (
                "ZEFF_LOBBY_TURN_URLS",
                "turn:example.com:3478?transport=udp",
            ),
            ("ZEFF_LOBBY_TURN_SECRET", &token),
        ])
        .unwrap();
        let servers = private.ice(&"0".repeat(24), false).unwrap();
        assert!(validate_ice_servers(&servers, true).is_ok());
        assert!(validate_ice_servers(&servers, false).is_err());
        assert!(private.ice(&"x".repeat(128), false).is_err());
    }
    #[test]
    fn public_access_requires_explicit_valid_configuration() {
        assert!(configured(&[]).is_err());
        assert!(configured(&[("ZEFF_LOBBY_ACCESS_TOKEN", "")]).is_err());
        let token = "a".repeat(32);
        let private = configured(&[("ZEFF_LOBBY_ACCESS_TOKEN", &token)]).unwrap();
        assert!(!private.authenticated(""));
        assert!(private.authenticated(&token));
        for invalid in ["", "1", "TRUE", " true", "yes"] {
            assert!(
                configured(&[
                    ("ZEFF_LOBBY_PUBLIC", invalid),
                    ("ZEFF_LOBBY_ACCESS_TOKEN", &token)
                ])
                .is_err()
            );
        }
        for supplied in [None, Some(""), Some(token.as_str())] {
            let mut values = vec![("ZEFF_LOBBY_PUBLIC", "true")];
            if let Some(token) = supplied {
                values.push(("ZEFF_LOBBY_ACCESS_TOKEN", token));
            }
            let public = configured(&values).unwrap();
            assert!(public.authenticated(""));
            assert!(public.authenticated("unused"));
            assert!(!public.authenticated(&"a".repeat(257)));
            assert!(public.turn.is_none());
        }
        assert!(
            configured(&[
                ("ZEFF_LOBBY_PUBLIC", "true"),
                ("ZEFF_LOBBY_ACCESS_TOKEN", "short")
            ])
            .is_err()
        );
        assert!(
            configured(&[
                ("ZEFF_LOBBY_PUBLIC", "true"),
                ("ZEFF_LOBBY_ALLOW_TURN", "true")
            ])
            .is_err()
        );
    }
    #[test]
    fn public_configuration_preserves_resource_and_origin_bounds() {
        for (name, values) in [
            ("ZEFF_LOBBY_MAX_ROOMS", vec!["0", "129", "bad"]),
            ("ZEFF_LOBBY_MAX_CONNECTIONS", vec!["1", "257"]),
            ("ZEFF_LOBBY_ROOM_TTL", vec!["9", "601"]),
            ("ZEFF_LOBBY_ORIGINS", vec!["*", "http://example.org"]),
            ("ZEFF_LOBBY_STUN_URLS", vec!["turn:example.org"]),
            ("ZEFF_LOBBY_ALLOW_TURN", vec!["1", "TRUE"]),
            ("ZEFF_LOBBY_TURN_SECRET", vec!["secret"]),
        ] {
            for value in values {
                assert!(
                    configured(&[("ZEFF_LOBBY_PUBLIC", "true"), (name, value)]).is_err(),
                    "accepted {name}={value}"
                );
            }
        }
        let c = configured(&[
            ("ZEFF_LOBBY_PUBLIC", "true"),
            ("ZEFF_LOBBY_MAX_ROOMS", "128"),
            ("ZEFF_LOBBY_MAX_CONNECTIONS", "256"),
            ("ZEFF_LOBBY_ROOM_TTL", "600"),
        ])
        .unwrap();
        assert_eq!(
            (c.max_rooms, c.max_connections, c.room_ttl.as_secs()),
            (128, 256, 600)
        );
        assert!(Config::from_lookup(|_| bail!("invalid Unicode environment")).is_err());
    }
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
