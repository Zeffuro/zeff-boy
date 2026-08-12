pub(crate) use self::local::LocalLinkTransport;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use self::native::TcpLinkTransport;

pub(crate) mod local;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkConnectionState {
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkTransportError {
    Disconnected,
    SendQueueClosed,
}

pub(crate) trait LinkTransport {
    fn state(&self) -> LinkConnectionState;

    fn send(&mut self, packet: &[u8]) -> Result<(), LinkTransportError>;

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, LinkTransportError>;

    fn disconnect(&mut self);
}
