use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use super::{LinkConnectionState, LinkTransport, LinkTransportError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalEndpoint {
    First,
    Second,
}

#[derive(Debug, Default)]
struct LocalLinkShared {
    connected: bool,
    first_queue: VecDeque<Vec<u8>>,
    second_queue: VecDeque<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct LocalLinkTransport {
    shared: Rc<RefCell<LocalLinkShared>>,
    endpoint: LocalEndpoint,
}

impl LocalLinkTransport {
    pub(crate) fn pair() -> (Self, Self) {
        let shared = Rc::new(RefCell::new(LocalLinkShared {
            connected: true,
            first_queue: VecDeque::new(),
            second_queue: VecDeque::new(),
        }));

        (
            Self {
                shared: Rc::clone(&shared),
                endpoint: LocalEndpoint::First,
            },
            Self {
                shared,
                endpoint: LocalEndpoint::Second,
            },
        )
    }

    fn incoming_queue_mut(
        shared: &mut LocalLinkShared,
        endpoint: LocalEndpoint,
    ) -> &mut VecDeque<Vec<u8>> {
        match endpoint {
            LocalEndpoint::First => &mut shared.first_queue,
            LocalEndpoint::Second => &mut shared.second_queue,
        }
    }

    fn peer_queue_mut(
        shared: &mut LocalLinkShared,
        endpoint: LocalEndpoint,
    ) -> &mut VecDeque<Vec<u8>> {
        match endpoint {
            LocalEndpoint::First => &mut shared.second_queue,
            LocalEndpoint::Second => &mut shared.first_queue,
        }
    }
}

impl LinkTransport for LocalLinkTransport {
    fn state(&self) -> LinkConnectionState {
        if self.shared.borrow().connected {
            LinkConnectionState::Connected
        } else {
            LinkConnectionState::Disconnected
        }
    }

    fn send(&mut self, packet: &[u8]) -> Result<(), LinkTransportError> {
        let mut shared = self.shared.borrow_mut();
        if !shared.connected {
            return Err(LinkTransportError::Disconnected);
        }

        Self::peer_queue_mut(&mut shared, self.endpoint).push_back(packet.to_vec());
        Ok(())
    }

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, LinkTransportError> {
        let mut shared = self.shared.borrow_mut();
        if !shared.connected {
            return Err(LinkTransportError::Disconnected);
        }

        Ok(Self::incoming_queue_mut(&mut shared, self.endpoint).pop_front())
    }

    fn disconnect(&mut self) {
        let mut shared = self.shared.borrow_mut();
        shared.connected = false;
        shared.first_queue.clear();
        shared.second_queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_link_transport_moves_packets_between_endpoints() {
        let (mut first, mut second) = LocalLinkTransport::pair();

        assert_eq!(first.state(), LinkConnectionState::Connected);
        assert_eq!(second.state(), LinkConnectionState::Connected);

        first.send(&[0x01, 0x02, 0x03]).unwrap();
        second.send(&[0xA0]).unwrap();

        assert_eq!(second.try_receive().unwrap(), Some(vec![0x01, 0x02, 0x03]));
        assert_eq!(first.try_receive().unwrap(), Some(vec![0xA0]));
        assert_eq!(first.try_receive().unwrap(), None);
        assert_eq!(second.try_receive().unwrap(), None);
    }

    #[test]
    fn local_link_transport_disconnect_affects_both_endpoints() {
        let (mut first, mut second) = LocalLinkTransport::pair();

        first.disconnect();

        assert_eq!(first.state(), LinkConnectionState::Disconnected);
        assert_eq!(second.state(), LinkConnectionState::Disconnected);
        assert_eq!(first.send(&[0x01]), Err(LinkTransportError::Disconnected));
        assert_eq!(second.try_receive(), Err(LinkTransportError::Disconnected));
    }
}
