#![forbid(unsafe_code)]

pub use zeff_netplay_protocol as protocol;

pub const CONTROL_LABEL: &str = "zeff-control";
pub const INPUT_LABEL: &str = "zeff-input";
pub const CHANNEL_PROTOCOL: &str = "zeff-netplay-v1";
pub const CONTROL_CHANNEL_ID: u16 = 0;
pub const INPUT_CHANNEL_ID: u16 = 1;

#[cfg(target_arch = "wasm32")]
pub mod browser;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
mod native;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub use native::{ConnectionPath, DataConnection, LobbyConnection, Packet, PacketKind};
