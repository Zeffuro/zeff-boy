use crate::link::transport::LocalLinkTransport;
use crate::link::{LinkEndpointId, LinkSession, LinkSystemType, RemoteLink};
use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent, ReplayPlayer};

pub(super) fn local_game_boy_link_pair(
    left_schedule: Vec<(u64, u64)>,
    right_schedule: Vec<(u64, u64)>,
    left_state_schedule: Vec<(u64, u64, zeff_emu_common::replay::ReplayGameBoyLinkState)>,
    right_state_schedule: Vec<(u64, u64, zeff_emu_common::replay::ReplayGameBoyLinkState)>,
) -> (
    RemoteLink<LocalLinkTransport>,
    RemoteLink<LocalLinkTransport>,
) {
    let (host, client) = LocalLinkTransport::pair();
    (
        RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::with_replay_schedules(
            LinkSession::new(host, LinkSystemType::GameBoy, LinkEndpointId(1)),
            left_schedule,
            left_state_schedule,
        )),
        RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::with_replay_schedules(
            LinkSession::new(client, LinkSystemType::GameBoy, LinkEndpointId(2)),
            right_schedule,
            right_state_schedule,
        )),
    )
}

pub(super) fn game_boy_replay_inbound_schedule(
    player: &ReplayPlayer,
    frame_base: u64,
    tick_base: u64,
) -> Vec<(u64, u64)> {
    player
        .metadata()
        .events
        .iter()
        .filter_map(|event| match event {
            ReplayEvent::GameBoyLink {
                frame,
                tick,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart { .. },
            }
            | ReplayEvent::GameBoyLink {
                frame,
                tick,
                event: ReplayGameBoyLinkEvent::RemoteReply { .. },
            } => Some((frame_base + frame, tick_base + tick)),
            _ => None,
        })
        .collect()
}

pub(super) fn game_boy_replay_state_schedule(
    player: &ReplayPlayer,
    frame_base: u64,
    tick_base: u64,
) -> Vec<(u64, u64, zeff_emu_common::replay::ReplayGameBoyLinkState)> {
    player
        .metadata()
        .events
        .iter()
        .filter_map(|event| match event {
            ReplayEvent::GameBoyLinkStateAtTick { frame, tick, state } => state
                .peer_present
                .then_some((frame_base + frame, tick_base + tick, *state)),
            _ => None,
        })
        .collect()
}
