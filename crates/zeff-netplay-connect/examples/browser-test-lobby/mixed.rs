use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tokio::time::{Instant, timeout};
use zeff_netplay_connect::{DataConnection, LobbyConnection, PacketKind, protocol::*};
use zeff_netplay_lobby::Lobby;

#[path = "packets.rs"]
mod packets;

#[derive(Clone)]
struct Fixture {
    lobby: Lobby,
    url: String,
    origin: String,
    jobs: Arc<Mutex<BTreeMap<String, String>>>,
    slot: Arc<tokio::sync::Semaphore>,
}

pub fn router(lobby: Lobby, url: String, origin: String, run_id: String) -> Router {
    Router::new()
        .route(
            "/test/mixed/health",
            get(move || {
                let id = run_id.clone();
                async move { id }
            }),
        )
        .route("/test/mixed/start", post(start))
        .route("/test/mixed/status/{room}", get(status))
        .with_state(Fixture {
            lobby,
            url,
            origin,
            jobs: Default::default(),
            slot: Arc::new(tokio::sync::Semaphore::new(1)),
        })
}

fn response(fixture: &Fixture, status: StatusCode, body: String) -> Response {
    (
        status,
        [
            ("access-control-allow-origin", fixture.origin.as_str()),
            ("cache-control", "no-store"),
        ],
        body,
    )
        .into_response()
}

fn allowed(fixture: &Fixture, headers: &HeaderMap) -> bool {
    headers.get("origin").and_then(|value| value.to_str().ok()) == Some(&fixture.origin)
}

async fn start(State(fixture): State<Fixture>, headers: HeaderMap, room: String) -> Response {
    if !allowed(&fixture, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !room.is_empty() && !valid_hex(&room, 24) {
        return response(&fixture, StatusCode::BAD_REQUEST, "invalid room".into());
    }
    let Ok(permit) = fixture.slot.clone().try_acquire_owned() else {
        return response(
            &fixture,
            StatusCode::CONFLICT,
            "native proof already running".into(),
        );
    };
    let identity = SessionIdentity {
        core: "nes".into(),
        content_hash: "a".repeat(64),
        compatibility_hash: "b".repeat(64),
        mode: SessionMode::SharedConsole,
    };
    let auth = if room.is_empty() {
        ClientMessage::Create {
            version: VERSION,
            access_token: String::new(),
            identity,
        }
    } else {
        ClientMessage::Join {
            version: VERSION,
            access_token: String::new(),
            identity,
            room,
        }
    };
    let connection = match LobbyConnection::open(&fixture.url, &auth).await {
        Ok(connection) => connection,
        Err(error) => return response(&fixture, StatusCode::BAD_GATEWAY, error.to_string()),
    };
    let room = connection.room().to_owned();
    fixture.jobs.lock().unwrap().clear();
    fixture
        .jobs
        .lock()
        .unwrap()
        .insert(room.clone(), "running".into());
    let job = room.clone();
    let state = fixture.clone();
    tokio::spawn(async move {
        let result = timeout(Duration::from_secs(30), async {
            let mut peer = connection.establish().await?;
            let result = exchange(&mut peer, &state.lobby).await;
            let closed = peer.close().await;
            result?;
            closed
        })
        .await;
        let status = match result {
            Ok(Ok(())) => "passed:64:direct:signaling-closed:loss-recovery".into(),
            Ok(Err(error)) => format!("failed:{error:#}"),
            Err(_) => "failed:native proof timeout".into(),
        };
        state.jobs.lock().unwrap().insert(job, status);
        drop(permit);
    });
    response(&fixture, StatusCode::OK, room)
}

async fn status(
    State(fixture): State<Fixture>,
    headers: HeaderMap,
    Path(room): Path<String>,
) -> Response {
    if !allowed(&fixture, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let result = fixture.jobs.lock().unwrap().get(&room).cloned();
    response(
        &fixture,
        if result.is_some() {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        },
        result.unwrap_or_default(),
    )
}

async fn send(peer: &DataConnection, round: u8) -> Result<()> {
    let packet = packets::payload(round);
    if packets::channel(round) == 0 {
        peer.send_control(&packet).await
    } else {
        peer.try_send_input(&packet).await
    }
}

async fn exchange(peer: &mut DataConnection, lobby: &Lobby) -> Result<()> {
    ensure!(
        !peer.connection_path().await?.relay,
        "mixed proof used relay"
    );
    ensure!(
        lobby.room_count() == 0,
        "signaling room survived establishment"
    );
    let mut omitted = false;
    let mut retries = 0;
    for round in 0..packets::ROUNDS {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut next_send = Instant::now() + Duration::from_millis(100);
        if packets::native_sender(round) {
            send(peer, round).await?;
        }
        loop {
            ensure!(Instant::now() < deadline, "mixed round {round} stalled");
            match timeout(
                Duration::from_millis(50),
                peer.receive(Duration::from_secs(3)),
            )
            .await
            {
                Ok(packet) => {
                    let packet = packet?;
                    let kind = u8::from(packet.kind == PacketKind::Input);
                    let current = packets::validate(kind, &packet.bytes, round, true)
                        .map_err(anyhow::Error::msg)?;
                    if current && round == 3 && !omitted {
                        omitted = true;
                        continue;
                    }
                    if packet.bytes[0] == 0xd1 {
                        peer.send_control(&packets::ack(packet.bytes[1])).await?;
                    }
                    if current {
                        break;
                    }
                }
                Err(_)
                    if packets::native_sender(round)
                        && packets::channel(round) == 1
                        && Instant::now() >= next_send =>
                {
                    send(peer, round).await?;
                    retries += 1;
                    next_send = Instant::now() + Duration::from_millis(100);
                }
                Err(_) => {}
            }
        }
    }
    ensure!(
        omitted && retries > 0,
        "mixed loss recovery was not exercised"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let finish = peer
            .receive(deadline.saturating_duration_since(Instant::now()))
            .await
            .context("missing mixed finish")?;
        if finish.kind == PacketKind::Control && finish.bytes == b"mixed-finish" {
            break;
        }
        packets::validate(
            u8::from(finish.kind == PacketKind::Input),
            &finish.bytes,
            packets::ROUNDS - 1,
            true,
        )
        .map_err(anyhow::Error::msg)?;
    }
    peer.send_control(b"mixed-finished").await?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let close = peer
            .receive(deadline.saturating_duration_since(Instant::now()))
            .await?;
        if close.kind == PacketKind::Control && close.bytes == b"mixed-close" {
            break;
        }
        packets::validate(
            u8::from(close.kind == PacketKind::Input),
            &close.bytes,
            packets::ROUNDS - 1,
            true,
        )
        .map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
