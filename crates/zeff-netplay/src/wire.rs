#[cfg(all(test, not(target_arch = "wasm32")))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::net::{Shutdown, TcpStream};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::AtomicBool;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

#[cfg(not(target_arch = "wasm32"))]
use crate::endpoint::ConnectionScope;
use crate::lockstep::Player;

mod admission;
mod build;
mod chat;
mod codec;
mod messages;
pub use admission::Admission;
pub use codec::PacketCodec;
#[cfg(not(target_arch = "wasm32"))]
mod connection;
#[cfg(not(target_arch = "wasm32"))]
mod io;
#[cfg(not(target_arch = "wasm32"))]
mod split;
pub use build::BuildInfo;
pub use chat::{CHAT_MAX_BYTES, validate_chat};
#[cfg(not(target_arch = "wasm32"))]
use io::{Driver, check_deadline};
#[cfg(all(test, not(target_arch = "wasm32")))]
use io::{read_deadline, write_deadline};
#[cfg(not(target_arch = "wasm32"))]
pub use split::{ConnectionTerminated, Receiver, Sender};

const MAGIC: &[u8; 4] = b"ZNPL";
const VERSION: u16 = 6;
const BUILD_INFO_OFFSET: usize = 243;
const HELLO_LEN: usize = BUILD_INFO_OFFSET + build::ENCODED_LEN;
const HEADER_LEN: usize = 48;
const TAG_LEN: usize = 32;
const MAX_PACKET: usize = 1024;
#[cfg(not(target_arch = "wasm32"))]
const IO_BUDGET: Duration = Duration::from_secs(2);
const AUTH_DOMAIN: &[u8] = b"zeff-netplay-auth-v1";
const READY_DOMAIN: &[u8] = b"zeff-netplay-ready-v1";
const PACKET_DOMAIN: &[u8] = b"zeff-netplay-packet-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "native-proof", derive(serde::Serialize))]
pub struct Identity {
    pub build: [u8; 32],
    pub build_info: BuildInfo,
    pub source: [u8; 32],
    pub effective: [u8; 32],
    pub media_len: u64,
    pub config: [u8; 32],
    pub initial: [u8; 32],
    pub persistent: [u8; 32],
    pub state_format: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Chat {
        text: String,
    },
    Input {
        player: Player,
        frame: u64,
        buttons: u8,
    },
    Checkpoint {
        frame: u64,
        logical: [u8; 32],
        video: [u8; 32],
        audio: [u8; 32],
        persistent: [u8; 32],
    },
    Close {
        frame: u64,
    },
    Pause {
        frame: u64,
        paused: bool,
    },
    Progress {
        frame: u64,
        confirmed: u64,
    },
    PauseChange {
        request: u64,
        frame: u64,
        paused: bool,
    },
    PauseAck {
        request: u64,
    },
}

#[cfg(not(target_arch = "wasm32"))]
mod tcp;
#[cfg(not(target_arch = "wasm32"))]
pub use tcp::{Connection, admit, admit_cancellable, admit_cancellable_scoped, admit_scoped};

fn hello(player: Player, identity: &Identity) -> Result<[u8; HELLO_LEN]> {
    let mut bytes = Vec::with_capacity(HELLO_LEN);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_be_bytes());
    bytes.push(role(player));
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce).map_err(|error| anyhow::anyhow!("nonce generation: {error}"))?;
    bytes.extend_from_slice(&nonce);
    for digest in [&identity.build, &identity.source, &identity.effective] {
        bytes.extend_from_slice(digest);
    }
    bytes.extend_from_slice(&identity.media_len.to_be_bytes());
    for digest in [&identity.config, &identity.initial, &identity.persistent] {
        bytes.extend_from_slice(digest);
    }
    bytes.extend_from_slice(&identity.state_format.to_be_bytes());
    bytes.extend_from_slice(&identity.build_info.encode()?);
    Ok(bytes.try_into().expect("fixed hello layout"))
}

fn role(player: Player) -> u8 {
    match player {
        Player::One => 1,
        Player::Two => 2,
    }
}

fn other(player: Player) -> Player {
    match player {
        Player::One => Player::Two,
        Player::Two => Player::One,
    }
}

fn mac(secret: &[u8; 32], parts: &[&[u8]]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts a 32-byte key");
    for part in parts {
        mac.update(part);
    }
    mac
}

fn tag(secret: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    mac(secret, parts).finalize().into_bytes().into()
}

fn verify(secret: &[u8; 32], parts: &[&[u8]], received: &[u8]) -> Result<()> {
    mac(secret, parts)
        .verify_slice(received)
        .map_err(|_| anyhow::anyhow!("authentication failed"))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
