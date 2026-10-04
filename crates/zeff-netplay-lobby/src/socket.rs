use crate::{Lobby, rooms::Membership};
use axum::extract::ws::{Message, WebSocket};
use std::time::{Duration, Instant};
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc},
    time::{sleep_until, timeout},
};
use zeff_netplay_protocol::{ClientMessage, ErrorCode, ServerMessage};

const SEND_BUDGET: Duration = Duration::from_secs(2);

struct Lease {
    lobby: Lobby,
    id: u64,
    member: Membership,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.lobby
            .0
            .rooms
            .lock()
            .unwrap()
            .remove(&self.member.code, self.id);
    }
}

async fn send(socket: &mut WebSocket, event: &ServerMessage) -> bool {
    let Ok(json) = serde_json::to_string(event) else {
        return false;
    };
    matches!(
        timeout(SEND_BUDGET, socket.send(Message::Text(json.into()))).await,
        Ok(Ok(()))
    )
}

fn decode(message: Message) -> Result<ClientMessage, ErrorCode> {
    let Message::Text(text) = message else {
        return Err(ErrorCode::Invalid);
    };
    serde_json::from_str(&text).map_err(|_| ErrorCode::Invalid)
}

pub async fn run(
    lobby: Lobby,
    id: u64,
    mut socket: WebSocket,
    _permit: OwnedSemaphorePermit,
    gate: Option<crate::listener::ConnectionGate>,
) {
    let first = match timeout(Duration::from_secs(5), socket.recv()).await {
        Ok(Some(Ok(frame))) => decode(frame),
        _ => return,
    };
    let (tx, mut rx) = mpsc::channel(8);
    let entered = first.and_then(|request| {
        lobby
            .0
            .rooms
            .lock()
            .unwrap()
            .enter(&lobby.0.config, id, tx, request)
    });
    let (member, welcome) = match entered {
        Ok(entered) => entered,
        Err(code) => {
            let _ = send(&mut socket, &ServerMessage::Error { code }).await;
            return;
        }
    };
    let lease = Lease { lobby, id, member };
    if let Some(gate) = gate {
        gate.admit();
    }
    if !send(&mut socket, &welcome).await {
        return;
    }
    let deadline = tokio::time::Instant::now() + lease.lobby.0.config.room_ttl;
    let mut rate = (Instant::now(), 0usize);
    loop {
        tokio::select! {
            _ = sleep_until(deadline) => {
                let _ = send(&mut socket, &ServerMessage::Error { code: ErrorCode::Expired }).await;
                break;
            }
            event = rx.recv() => {
                let Some(event) = event else { break; };
                let terminal = matches!(event, ServerMessage::Complete | ServerMessage::PeerLeft | ServerMessage::Error {..});
                if !send(&mut socket, &event).await || terminal { break; }
            }
            message = socket.recv() => {
                let Some(Ok(message)) = message else { break; };
                if matches!(message, Message::Close(_)) { break; }
                if rate.0.elapsed() >= Duration::from_secs(1) { rate = (Instant::now(), 0); }
                rate.1 += 1;
                if matches!(message, Message::Ping(_) | Message::Pong(_)) && rate.1 <= 20 {
                    if let Message::Ping(payload) = message
                        && !matches!(timeout(SEND_BUDGET, socket.send(Message::Pong(payload))).await, Ok(Ok(()))) {
                        break;
                    }
                    continue;
                }
                let outcome = if rate.1 > 20 { Err(ErrorCode::RateLimited) } else {
                    decode(message).and_then(|request| lease.lobby.0.rooms.lock().unwrap().signal(&lease.member, id, request))
                };
                if let Err(code) = outcome {
                    let _ = send(&mut socket, &ServerMessage::Error { code }).await; break;
                }
            }
        }
    }
    let _ = timeout(SEND_BUDGET, socket.send(Message::Close(None))).await;
}
