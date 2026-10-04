#![forbid(unsafe_code)]

pub mod config;
pub mod listener;
mod rooms;
mod socket;

use axum::{
    Extension, Router,
    extract::{ConnectInfo, State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{any, get},
};
use config::Config;
use rooms::Rooms;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use zeff_netplay_protocol::MAX_MESSAGE_BYTES;

struct StateInner {
    config: Config,
    rooms: Mutex<Rooms>,
    slots: Arc<Semaphore>,
    ids: AtomicU64,
    admissions: Mutex<(Instant, usize)>,
}

#[derive(Clone)]
pub struct Lobby(Arc<StateInner>);

impl Lobby {
    pub fn new(config: Config) -> Self {
        let slots = Arc::new(Semaphore::new(config.max_connections));
        Self(Arc::new(StateInner {
            config,
            slots,
            rooms: Mutex::new(Rooms::default()),
            ids: AtomicU64::new(1),
            admissions: Mutex::new((Instant::now(), 0)),
        }))
    }

    pub fn room_count(&self) -> usize {
        let mut rooms = self.0.rooms.lock().unwrap();
        rooms.expire(&self.0.config);
        rooms.len()
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/v1/ws", any(upgrade))
            .with_state(self.clone())
    }
}

async fn upgrade(
    State(lobby): State<Lobby>,
    gate: Option<Extension<ConnectInfo<listener::ConnectionGate>>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(origin) = headers.get("origin")
        && !origin
            .to_str()
            .ok()
            .is_some_and(|o| lobby.0.config.origins.iter().any(|allowed| o == allowed))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(permit) = lobby.0.slots.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    {
        let mut admissions = lobby.0.admissions.lock().unwrap();
        if admissions.0.elapsed() >= Duration::from_secs(60) {
            *admissions = (Instant::now(), 0);
        }
        if admissions.1 >= 120 {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        admissions.1 += 1;
    }
    let id = lobby.0.ids.fetch_add(1, Ordering::Relaxed);
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .read_buffer_size(4096)
        .write_buffer_size(4096)
        .max_write_buffer_size(MAX_MESSAGE_BYTES * 2)
        .on_upgrade(move |socket| {
            socket::run(
                lobby,
                id,
                socket,
                permit,
                gate.map(|Extension(ConnectInfo(gate))| gate),
            )
        })
        .into_response()
}
