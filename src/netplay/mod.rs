pub(crate) mod adapters;
pub(crate) mod chat;
pub(crate) mod compatibility;
pub(crate) mod connect;
pub(crate) mod identity;
pub(crate) mod network;
pub(crate) mod proof;
pub(crate) mod session;
pub(crate) mod ui;

pub(crate) struct Start {
    pub(crate) stream: std::net::TcpStream,
    pub(crate) player: zeff_netplay::lockstep::Player,
    pub(crate) build: [u8; 32],
    pub(crate) secret: [u8; 32],
    pub(crate) scope: zeff_netplay::endpoint::ConnectionScope,
    pub(crate) allow_different_versions: bool,
    pub(crate) verify_every_frame: bool,
    pub(crate) input_delay: zeff_netplay::rollback::InputDelay,
}

pub(crate) enum Response {
    Ready,
    Chat {
        local: bool,
        text: String,
    },
    ChatError(String),
    Paused {
        frame: u64,
        local: bool,
        peer: bool,
    },
    Frame {
        checkpoint: zeff_netplay::wire::Message,
        ports: [u8; 2],
        audio: Vec<f32>,
    },
    Audio {
        frame: u64,
        audio: Vec<f32>,
    },
    Presented {
        frame: u64,
        confirmed: u64,
        changed: bool,
        step_complete: bool,
        prediction_depth: u64,
        rollback_frames: u64,
        retained_bytes: usize,
    },
    Stopped {
        reason: String,
        restored: bool,
    },
    Rejected(String),
}
