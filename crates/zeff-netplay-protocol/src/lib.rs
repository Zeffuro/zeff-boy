#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const VERSION: u16 = 1;
pub const MAX_MESSAGE_BYTES: usize = 40 * 1024;
pub const MAX_SDP_BYTES: usize = 32 * 1024;
pub const MAX_CANDIDATE_BYTES: usize = 2048;
pub const MAX_CANDIDATES: usize = 64;
pub const MAX_PEER_PACKET_BYTES: usize = 1024;
pub const CONTROL_LABEL: &str = "zeff-control";
pub const INPUT_LABEL: &str = "zeff-input";
pub const CHANNEL_PROTOCOL: &str = "zeff-netplay-v1";
pub const CONTROL_CHANNEL_ID: u16 = 0;
pub const INPUT_CHANNEL_ID: u16 = 1;

mod ice;
pub use ice::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    SharedConsole,
    LinkedDevices,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionIdentity {
    pub core: String,
    pub content_hash: String,
    pub compatibility_hash: String,
    pub mode: SessionMode,
}

impl SessionIdentity {
    pub fn valid(&self) -> bool {
        !self.core.is_empty()
            && self.core.len() <= 64
            && self
                .core
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            && valid_hex(&self.content_hash, 64)
            && valid_hex(&self.compatibility_hash, 64)
    }
}

pub fn valid_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Host,
    Guest,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Signal {
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
    Candidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_m_line_index: Option<u16>,
    },
}

impl Signal {
    pub fn valid(&self) -> bool {
        match self {
            Self::Offer { sdp } | Self::Answer { sdp } => {
                !sdp.is_empty() && sdp.len() <= MAX_SDP_BYTES && !sdp.contains('\0')
            }
            Self::Candidate {
                candidate, sdp_mid, ..
            } => {
                candidate.len() <= MAX_CANDIDATE_BYTES
                    && sdp_mid.as_ref().is_none_or(|s| s.len() <= 32)
                    && !candidate.contains('\0')
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Create {
        version: u16,
        access_token: String,
        identity: SessionIdentity,
    },
    Join {
        version: u16,
        access_token: String,
        room: String,
        identity: SessionIdentity,
    },
    Signal {
        signal: Signal,
    },
    Finish {},
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    Version,
    Invalid,
    Full,
    Unavailable,
    Incompatible,
    RateLimited,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerMessage {
    Welcome {
        version: u16,
        room: String,
        role: Role,
        ice_servers: Vec<IceServer>,
        relay_allowed: bool,
    },
    PeerJoined,
    Signal {
        signal: Signal,
    },
    Complete,
    PeerLeft,
    Error {
        code: ErrorCode,
    },
}

impl std::fmt::Debug for IceServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IceServer")
            .field("url_count", &self.urls.len())
            .field("authenticated", &self.credential.is_some())
            .finish()
    }
}
impl std::fmt::Debug for Signal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, bytes) = match self {
            Self::Offer { sdp } => ("Offer", sdp.len()),
            Self::Answer { sdp } => ("Answer", sdp.len()),
            Self::Candidate { candidate, .. } => ("Candidate", candidate.len()),
        };
        f.debug_struct(kind).field("bytes", &bytes).finish()
    }
}
impl std::fmt::Debug for ClientMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Create {
                version, identity, ..
            } => f
                .debug_struct("Create")
                .field("version", version)
                .field("identity", identity)
                .finish_non_exhaustive(),
            Self::Join {
                version, identity, ..
            } => f
                .debug_struct("Join")
                .field("version", version)
                .field("identity", identity)
                .finish_non_exhaustive(),
            Self::Signal { signal } => signal.fmt(f),
            Self::Finish {} => f.write_str("Finish"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_core_independent_and_exact() {
        for core in ["nes", "gba", "ws", "sega8"] {
            let identity = SessionIdentity {
                core: core.into(),
                content_hash: "1".repeat(64),
                compatibility_hash: "2".repeat(64),
                mode: SessionMode::SharedConsole,
            };
            assert!(identity.valid());
            assert_eq!(
                identity,
                serde_json::from_str(&serde_json::to_string(&identity).unwrap()).unwrap()
            );
        }
    }

    #[test]
    fn arbitrary_payloads_and_unknown_fields_are_rejected() {
        assert!(
            serde_json::from_str::<ClientMessage>(r#"{"type":"input","data":"payload"}"#).is_err()
        );
        assert!(
            serde_json::from_str::<ClientMessage>(r#"{"type":"finish","data":"payload"}"#).is_err()
        );
        assert!(
            !Signal::Offer {
                sdp: "x".repeat(MAX_SDP_BYTES + 1)
            }
            .valid()
        );
        assert!(!valid_hex(&"A".repeat(24), 24));
    }

    #[test]
    fn debugging_never_exposes_auth_or_ice_secrets() {
        let auth = ClientMessage::Join {
            version: VERSION,
            access_token: "private-access-token".into(),
            room: "private-room-code".into(),
            identity: SessionIdentity {
                core: "nes".into(),
                content_hash: "0".repeat(64),
                compatibility_hash: "1".repeat(64),
                mode: SessionMode::SharedConsole,
            },
        };
        let debug = format!("{auth:?}");
        assert!(!debug.contains("private-access-token") && !debug.contains("private-room-code"));
        let ice = IceServer {
            urls: vec!["turn:private-server".into()],
            username: Some("private-user".into()),
            credential: Some("private-password".into()),
        };
        assert!(!format!("{ice:?}").contains("private-"));
        assert!(
            !format!(
                "{:?}",
                Signal::Offer {
                    sdp: "private-address".into()
                }
            )
            .contains("private-address")
        );
    }
}
