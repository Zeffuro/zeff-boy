use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use zeff_netplay_connect::protocol::{ClientMessage, SessionIdentity, SessionMode, VERSION};
use zeff_netplay_connect::{DataConnection, LobbyConnection, PacketKind};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    ensure!(
        args.len() >= 3,
        "usage: native-proof create|join ws[s]://host/v1/ws [room]; token from ZEFF_LOBBY_TOKEN"
    );
    let token = std::env::var("ZEFF_LOBBY_TOKEN").context("ZEFF_LOBBY_TOKEN is required")?;
    let identity = SessionIdentity {
        core: "transport-proof".into(),
        content_hash: "1".repeat(64),
        compatibility_hash: "2".repeat(64),
        mode: SessionMode::SharedConsole,
    };
    let host = args[1] == "create";
    let auth = match args[1].as_str() {
        "create" => ClientMessage::Create {
            version: VERSION,
            access_token: token,
            identity,
        },
        "join" => ClientMessage::Join {
            version: VERSION,
            access_token: token,
            identity,
            room: args.get(3).context("join needs a room")?.clone(),
        },
        _ => bail!("role must be create or join"),
    };
    let lobby = LobbyConnection::open(&args[2], &auth).await?;
    println!("ROOM={}", lobby.room());
    std::io::stdout().flush()?;
    let mut peer = lobby.establish().await?;
    println!("SIGNALING_COMPLETE_AND_CLOSED");
    std::io::stdout().flush()?;
    let delay = std::env::var("ZEFF_NETPLAY_PROOF_START_DELAY_MS")
        .unwrap_or_else(|_| "0".into())
        .parse::<u64>()?;
    ensure!(delay <= 5000, "proof start delay exceeds limit");
    tokio::time::sleep(Duration::from_millis(delay)).await;
    let path = peer.connection_path().await?;
    println!(
        "PATH LOCAL={} REMOTE={} PROTOCOL={} RELAY={}",
        path.local_type, path.remote_type, path.protocol, path.relay
    );
    for (phase, count) in [(0, 240), (1, 8)] {
        let result = exchange(&mut peer, host, phase, count).await;
        if let Err(error) = result {
            let _ = peer.close().await;
            return Err(error);
        }
    }
    if host {
        peer.send_control(b"proof-close-ready").await?;
        let final_ack = peer.receive(Duration::from_secs(10)).await?;
        ensure!(
            final_ack.kind == PacketKind::Control && final_ack.bytes == b"proof-close-ack",
            "missing final acknowledgement"
        );
        peer.send_control(b"proof-close-done").await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    } else {
        let ready = peer.receive(Duration::from_secs(10)).await?;
        ensure!(
            ready.kind == PacketKind::Control && ready.bytes == b"proof-close-ready",
            "missing close barrier"
        );
        peer.send_control(b"proof-close-ack").await?;
        let done = peer.receive(Duration::from_secs(10)).await?;
        ensure!(
            done.kind == PacketKind::Control && done.bytes == b"proof-close-done",
            "missing close completion"
        );
    }
    peer.close().await?;
    println!("PROOF_OK");
    Ok(())
}

async fn exchange(peer: &mut DataConnection, host: bool, phase: u8, count: u32) -> Result<()> {
    let mut hash = 0xcbf29ce484222325u64;
    for sequence in 0..count {
        let control = packet(phase, sequence, PacketKind::Control);
        let input = packet(phase, sequence, PacketKind::Input);
        if host {
            peer.send_control(&control).await?;
            peer.try_send_input(&input).await?;
        }
        let mut saw_control = false;
        let mut saw_input = false;
        for _ in 0..2 {
            let received = peer.receive(Duration::from_secs(10)).await?;
            match received.kind {
                PacketKind::Control => {
                    ensure!(
                        !saw_control && received.bytes == control,
                        "control sequence or bytes differ"
                    );
                    saw_control = true;
                    if !host {
                        peer.send_control(&received.bytes).await?;
                    }
                }
                PacketKind::Input => {
                    ensure!(
                        !saw_input && received.bytes == input,
                        "input sequence or bytes differ"
                    );
                    saw_input = true;
                    if !host {
                        peer.try_send_input(&received.bytes).await?;
                    }
                }
            }
        }
        ensure!(saw_control && saw_input, "incomplete exchange");
        for byte in control.iter().chain(&input) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    println!(
        "PHASE={phase} AFTER_SIGNALING_CLOSE=true CONTROL={count} INPUT={count} BYTES={} FNV1A64={hash:016x}",
        u64::from(count) * 128
    );
    std::io::stdout().flush()?;
    Ok(())
}

fn packet(phase: u8, sequence: u32, kind: PacketKind) -> Vec<u8> {
    let mut bytes = vec![0; 64];
    bytes[0..4].copy_from_slice(b"ZNP1");
    bytes[4] = phase;
    bytes[5] = match kind {
        PacketKind::Control => 0,
        PacketKind::Input => 1,
    };
    bytes[6..10].copy_from_slice(&sequence.to_le_bytes());
    for (index, byte) in bytes.iter_mut().enumerate().skip(10) {
        *byte = (sequence as u8)
            .wrapping_mul(13)
            .wrapping_add(index as u8)
            .wrapping_add(phase);
    }
    bytes
}
