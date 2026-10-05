#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod adapters;
pub(crate) mod capabilities;
pub(crate) mod chat;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod compatibility;
#[cfg(target_arch = "wasm32")]
#[path = "compatibility/browser.rs"]
pub(crate) mod compatibility;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod connect;
#[cfg(target_arch = "wasm32")]
#[path = "connect/browser.rs"]
pub(crate) mod connect;
pub(crate) mod identity;
pub(crate) mod metrics;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod network;
#[cfg(target_arch = "wasm32")]
#[path = "network/browser.rs"]
pub(crate) mod network;
#[cfg(test)]
mod portability_tests;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod proof;
pub(crate) mod session;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod test_lobby;
pub(crate) mod ui;

pub(crate) enum Transport {
    #[cfg(not(target_arch = "wasm32"))]
    Tcp(std::net::TcpStream),
    #[cfg(not(target_arch = "wasm32"))]
    Direct(Box<DirectPeer>),
    #[cfg(target_arch = "wasm32")]
    Browser(zeff_netplay_connect::browser::BrowserPeer),
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct DirectPeer {
    pub(crate) connection: zeff_netplay_connect::DataConnection,
    pub(crate) runtime: tokio::runtime::Runtime,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<std::net::TcpStream> for Transport {
    fn from(stream: std::net::TcpStream) -> Self {
        Self::Tcp(stream)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Transport {
    pub(crate) fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            Self::Tcp(stream) => stream.local_addr(),
            Self::Direct(_) => Err(std::io::ErrorKind::Unsupported.into()),
        }
    }

    pub(crate) fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            Self::Tcp(stream) => stream.peer_addr(),
            Self::Direct(_) => Err(std::io::ErrorKind::Unsupported.into()),
        }
    }
}

pub(crate) struct Start {
    pub(crate) stream: Transport,
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
    NetworkStats(metrics::Stats),
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
        ports: [u16; 2],
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
