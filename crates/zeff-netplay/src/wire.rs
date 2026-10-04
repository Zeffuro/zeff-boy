#[cfg(test)]
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use crate::endpoint::ConnectionScope;
use crate::lockstep::Player;

mod build;
mod chat;
mod connection;
mod io;
mod split;
pub use build::BuildInfo;
pub use chat::{CHAT_MAX_BYTES, validate_chat};
use io::{Driver, check_deadline};
#[cfg(test)]
use io::{read_deadline, write_deadline};
pub use split::{Receiver, Sender};

const MAGIC: &[u8; 4] = b"ZNPL";
const VERSION: u16 = 6;
const BUILD_INFO_OFFSET: usize = 243;
const HELLO_LEN: usize = BUILD_INFO_OFFSET + build::ENCODED_LEN;
const HEADER_LEN: usize = 48;
const TAG_LEN: usize = 32;
const MAX_PACKET: usize = 1024;
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

pub struct Connection {
    io: Driver,
    stream: TcpStream,
    player: Player,
    secret: [u8; 32],
    transcript: [u8; 32],
    send_sequence: u64,
    receive_sequence: u64,
    sent_close: bool,
    received_close: bool,
    terminal: bool,
    cancellation: Option<Arc<AtomicBool>>,
}

pub fn admit(
    stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
) -> Result<Connection> {
    admit_inner(
        stream,
        player,
        identity,
        secret,
        None,
        ConnectionScope::Loopback,
    )
}

pub fn admit_cancellable(
    stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    cancellation: Arc<AtomicBool>,
) -> Result<Connection> {
    admit_inner(
        stream,
        player,
        identity,
        secret,
        Some(cancellation),
        ConnectionScope::Loopback,
    )
}

pub fn admit_scoped(
    stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    scope: ConnectionScope,
) -> Result<Connection> {
    admit_inner(stream, player, identity, secret, None, scope)
}

pub fn admit_cancellable_scoped(
    stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    scope: ConnectionScope,
    cancellation: Arc<AtomicBool>,
) -> Result<Connection> {
    admit_inner(stream, player, identity, secret, Some(cancellation), scope)
}

fn admit_inner(
    mut stream: TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    cancellation: Option<Arc<AtomicBool>>,
    scope: ConnectionScope,
) -> Result<Connection> {
    let result = handshake(
        &mut stream,
        player,
        identity,
        secret,
        cancellation.as_deref(),
        scope,
    );
    match result {
        Ok((io, transcript)) => Ok(Connection {
            io,
            stream,
            player,
            secret: *secret,
            transcript,
            send_sequence: 0,
            receive_sequence: 0,
            sent_close: false,
            received_close: false,
            terminal: false,
            cancellation,
        }),
        Err(error) => {
            let _ = stream.shutdown(Shutdown::Both);
            Err(error)
        }
    }
}

fn handshake(
    stream: &mut TcpStream,
    player: Player,
    identity: &Identity,
    secret: &[u8; 32],
    cancellation: Option<&AtomicBool>,
    scope: ConnectionScope,
) -> Result<(Driver, [u8; 32])> {
    let deadline = Instant::now() + IO_BUDGET;
    check_deadline(deadline, cancellation)?;
    stream.set_nonblocking(false)?;
    scope.validate_connection(stream)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(IO_BUDGET))?;
    stream.set_write_timeout(Some(IO_BUDGET))?;
    let mut driver = Driver::new(stream)?;
    let local = hello(player, identity)?;
    driver.write(stream, &local, deadline, cancellation)?;
    let mut remote = [0; HELLO_LEN];
    driver
        .read(stream, &mut remote, deadline, cancellation)
        .context("reading admission hello")?;
    ensure!(&remote[..4] == MAGIC, "invalid admission magic");
    ensure!(
        remote[4..6] == VERSION.to_be_bytes(),
        "unsupported wire version"
    );
    ensure!(remote[6] == role(other(player)), "incompatible player role");
    ensure!(
        remote[71..BUILD_INFO_OFFSET] == local[71..BUILD_INFO_OFFSET],
        "session identity mismatch"
    );
    let remote_info = BuildInfo::decode(&remote[BUILD_INFO_OFFSET..])?;
    build::admit_builds(
        &identity.build,
        &identity.build_info,
        &remote[39..71].try_into()?,
        &remote_info,
    )?;
    let (host, client) = if player == Player::One {
        (&local, &remote)
    } else {
        (&remote, &local)
    };
    let transcript: [u8; 32] = Sha256::new()
        .chain_update(AUTH_DOMAIN)
        .chain_update(host)
        .chain_update(client)
        .finalize()
        .into();
    let local_tag = tag(secret, &[AUTH_DOMAIN, host, client, &[role(player)]]);
    driver.write(stream, &local_tag, deadline, cancellation)?;
    let mut remote_tag = [0; TAG_LEN];
    driver
        .read(stream, &mut remote_tag, deadline, cancellation)
        .context("reading authentication")?;
    verify(
        secret,
        &[AUTH_DOMAIN, host, client, &[role(other(player))]],
        &remote_tag,
    )?;
    let ready = tag(secret, &[READY_DOMAIN, &transcript, &[role(player)]]);
    driver.write(stream, &ready, deadline, cancellation)?;
    driver
        .read(stream, &mut remote_tag, deadline, cancellation)
        .context("reading Ready barrier")?;
    verify(
        secret,
        &[READY_DOMAIN, &transcript, &[role(other(player))]],
        &remote_tag,
    )?;
    check_deadline(deadline, cancellation)?;
    Ok((driver, transcript))
}

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

#[cfg(test)]
mod tests;
