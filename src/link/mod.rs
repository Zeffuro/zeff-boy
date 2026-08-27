#![allow(dead_code, unused_imports)]

pub(crate) use self::packet::{LinkPacket, LinkPacketDecodeError, LinkPacketKind};
pub(crate) use self::protocol::{
    LINK_PROTOCOL_VERSION, LinkEndpointId, LinkSystemType, LinkTransferId,
};
pub(crate) use self::session::{LinkSession, LinkSessionError, LinkedEmulationSession};
pub(crate) use self::transport::{LinkConnectionState, LinkTransport, LinkTransportError};
use zeff_emu_common::system::System as ActiveSystem;

pub(crate) mod gb;
pub(crate) mod packet;
pub(crate) mod protocol;
pub(crate) mod session;
pub(crate) mod transport;
pub(crate) mod ws;
pub(crate) mod ws_replay;

pub(crate) fn remote_link_system_for_active_system(
    active_system: ActiveSystem,
) -> Option<LinkSystemType> {
    match active_system {
        ActiveSystem::GameBoy => Some(LinkSystemType::GameBoy),
        ActiveSystem::WonderSwan => Some(LinkSystemType::WonderSwan),
        ActiveSystem::GameBoyAdvance
        | ActiveSystem::Nes
        | ActiveSystem::Coleco
        | ActiveSystem::Pce
        | ActiveSystem::MasterSystem
        | ActiveSystem::GameGear
        | ActiveSystem::Sg1000 => None,
    }
}

pub(crate) enum RemoteLink<T: LinkTransport> {
    GameBoy(gb::GameBoyRemoteLink<T>),
    WonderSwan(ws::WonderSwanRemoteLink<T>),
}

impl<T: LinkTransport> RemoteLink<T> {
    pub(crate) fn state(&self) -> LinkConnectionState {
        match self {
            Self::GameBoy(link) => link.state(),
            Self::WonderSwan(link) => link.state(),
        }
    }

    pub(crate) fn disconnect(&mut self) {
        match self {
            Self::GameBoy(link) => link.disconnect(),
            Self::WonderSwan(link) => link.disconnect(),
        }
    }

    pub(crate) fn take_replay_events(&mut self) -> Vec<zeff_emu_common::replay::ReplayEvent> {
        match self {
            Self::GameBoy(link) => link.take_replay_events(),
            Self::WonderSwan(link) => link.take_replay_events(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_systems_with_remote_link_support_match_implemented_adapters() {
        assert_eq!(
            remote_link_system_for_active_system(ActiveSystem::GameBoy),
            Some(LinkSystemType::GameBoy)
        );
        assert_eq!(
            remote_link_system_for_active_system(ActiveSystem::WonderSwan),
            Some(LinkSystemType::WonderSwan)
        );
        assert_eq!(
            remote_link_system_for_active_system(ActiveSystem::GameBoyAdvance),
            None
        );
        assert_eq!(
            remote_link_system_for_active_system(ActiveSystem::Nes),
            None
        );
        assert_eq!(
            remote_link_system_for_active_system(ActiveSystem::MasterSystem),
            None
        );
        assert_eq!(
            remote_link_system_for_active_system(ActiveSystem::GameGear),
            None
        );
        assert_eq!(
            remote_link_system_for_active_system(ActiveSystem::Sg1000),
            None
        );
    }
}
