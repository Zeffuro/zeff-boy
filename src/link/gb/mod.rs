mod diagnostics;
mod live;
mod protocol;
mod replay;
#[cfg(not(target_arch = "wasm32"))]
mod trace;

#[cfg(test)]
mod tests;

pub(crate) use live::GameBoyRemoteLink;
pub(crate) use protocol::GameBoyLinkPayloadError;
pub(crate) use replay::GameBoyReplayLink;
