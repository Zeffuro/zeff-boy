#![forbid(unsafe_code)]

pub use zeff_netplay_protocol as protocol;

pub use protocol::{
    CHANNEL_PROTOCOL, CONTROL_CHANNEL_ID, CONTROL_LABEL, INPUT_CHANNEL_ID, INPUT_LABEL,
};

#[cfg(target_arch = "wasm32")]
pub mod browser;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
mod native;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub use native::{ConnectionPath, DataConnection, LobbyConnection, Packet, PacketKind};
