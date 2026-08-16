use std::net::SocketAddr;
use std::sync::mpsc::Receiver;
use std::time::Duration;

mod parse;
mod server;
mod types;
mod wire;

#[cfg(test)]
mod tests;

pub(crate) use types::{
    LiveCommand, LiveMemorySpace, LiveReply, LiveRequest, PendingButtonRelease,
};

const ENV_VAR: &str = "ZEFF_REMOTE_CONTROL";
pub(crate) const AUTOMATION_ENV_VAR: &str = "ZEFF_REMOTE_AUTOMATION";
const DEFAULT_ADDR: &str = "127.0.0.1:17684";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct LiveControl {
    rx: Option<Receiver<LiveRequest>>,
    addr: Option<SocketAddr>,
}

impl LiveControl {
    pub(crate) fn from_env() -> Self {
        let Some((rx, addr)) = server::start_from_env() else {
            return Self {
                rx: None,
                addr: None,
            };
        };

        Self { rx: Some(rx), addr }
    }

    pub(crate) fn try_recv(&self) -> Option<LiveRequest> {
        self.rx.as_ref()?.try_recv().ok()
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.rx.is_some()
    }

    pub(crate) fn addr(&self) -> Option<SocketAddr> {
        self.addr
    }
}

pub(crate) fn automation_mode_enabled() -> bool {
    std::env::var(AUTOMATION_ENV_VAR)
        .ok()
        .is_some_and(|value| env_flag_enabled(value.trim()))
}

fn env_flag_enabled(value: &str) -> bool {
    !value.is_empty() && !matches!(value, "0" | "false" | "False" | "off" | "Off" | "no")
}
