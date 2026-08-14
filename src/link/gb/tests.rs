use std::path::PathBuf;

use zeff_emu_common::replay::{ReplayEvent, ReplayGameBoyLinkEvent, ReplayGameBoyLinkReply};
use zeff_gb_core::emulator::Emulator as GameBoyEmulator;
use zeff_gb_core::hardware::bus::{GameBoyLinkAction, GameBoyLinkReply};
use zeff_gb_core::hardware::types::constants::{IE_ADDR, INTERRUPT_IF, SERIAL_SB, SERIAL_SC};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use crate::emu_backend::EmuBackend;
use crate::link::transport::LocalLinkTransport;
use crate::link::{LinkEndpointId, LinkPacketKind, LinkSession, LinkSessionError, LinkSystemType};

use super::protocol::{
    GameBoyLinkEvent, GbTransferId, decode_game_boy_link_event, encode_game_boy_link_event,
};
use super::{GameBoyRemoteLink, GameBoyReplayLink};

#[test]
fn game_boy_link_event_payload_roundtrips_master_start() {
    let event = GameBoyLinkEvent::MasterStart {
        transfer_id: GbTransferId(0x0100_0000_0000_0007),
        start_tick: 123,
        action: GameBoyLinkAction {
            out_byte: 0xAB,
            clock_period_t_cycles: 4096,
            serial_generation: 42,
        },
    };

    assert_eq!(
        decode_game_boy_link_event(&encode_game_boy_link_event(event)),
        Ok(event)
    );
}

#[test]
fn game_boy_link_event_payload_roundtrips_transfer_reply() {
    let event = GameBoyLinkEvent::TransferReply {
        transfer_id: GbTransferId(0x0200_0000_0000_0009),
        sample_tick: 456,
        reply: GameBoyLinkReply {
            out_byte: 0x34,
            passive: true,
            serial_generation: 77,
        },
    };

    assert_eq!(
        decode_game_boy_link_event(&encode_game_boy_link_event(event)),
        Ok(event)
    );
}

#[test]
fn game_boy_remote_link_binds_reply_to_exact_transfer_id() {
    let (left_transport, right_transport) = LocalLinkTransport::pair();
    let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
        left_transport,
        LinkSystemType::GameBoy,
        LinkEndpointId(1),
    ));
    let mut right_link = GameBoyRemoteLink::new(LinkSession::new(
        right_transport,
        LinkSystemType::GameBoy,
        LinkEndpointId(2),
    ));
    let mut left = gb_emulator();
    let mut right = gb_emulator();

    left.set_game_boy_link_peer_present(true);
    right.set_game_boy_link_peer_present(true);
    left.write_byte(SERIAL_SB, 0xAB);
    right.write_byte(SERIAL_SB, 0x34);
    left.write_byte(SERIAL_SC, 0x81);
    right.write_byte(SERIAL_SC, 0x80);

    left_link.poll_emulator(&mut left).unwrap();
    right_link.poll_emulator(&mut right).unwrap();
    left_link.poll_emulator(&mut left).unwrap();

    assert_eq!(right.cpu_peek8(SERIAL_SB), 0xAB);
    assert_eq!(right.cpu_peek8(SERIAL_SC) & 0x80, 0);
    assert_eq!(right.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    assert_eq!(left.cpu_peek8(SERIAL_SB), 0xAB);
    assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0x80);

    left.step_frame();
    assert_eq!(left.cpu_peek8(SERIAL_SB), 0x34);
    assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0);
    assert_eq!(left.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
}

#[test]
fn game_boy_remote_link_records_replay_events_for_endpoint() {
    let (left_transport, right_transport) = LocalLinkTransport::pair();
    let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
        left_transport,
        LinkSystemType::GameBoy,
        LinkEndpointId(1),
    ));
    let mut right_link = GameBoyRemoteLink::new(LinkSession::new(
        right_transport,
        LinkSystemType::GameBoy,
        LinkEndpointId(2),
    ));
    let mut left = gb_emulator();
    let mut right = gb_emulator();

    left.set_game_boy_link_peer_present(true);
    right.set_game_boy_link_peer_present(true);
    left.write_byte(SERIAL_SB, 0xAB);
    right.write_byte(SERIAL_SB, 0x34);
    left.write_byte(SERIAL_SC, 0x81);
    right.write_byte(SERIAL_SC, 0x80);

    left_link.poll_emulator(&mut left).unwrap();
    right_link.poll_emulator(&mut right).unwrap();
    left_link.poll_emulator(&mut left).unwrap();

    let left_events = left_link.take_replay_events();
    let right_events = right_link.take_replay_events();

    assert_eq!(
        left_events,
        vec![
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0xAB,
                    serial_generation: 4,
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0100_0000_0000_0000,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 4,
                },
            },
        ]
    );
    assert_eq!(
        right_events,
        vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: 0x0100_0000_0000_0000,
                clock_period_t_cycles: 4096,
                out_byte: 0xAB,
                serial_generation: 4,
                local_reply: Some(ReplayGameBoyLinkReply {
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 4,
                }),
            },
        }]
    );
}

#[test]
fn game_boy_replay_link_applies_recorded_reply_without_tcp_peer() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: 0x0100_0000_0000_0000,
                out_byte: 0x34,
                passive: true,
                serial_generation: 4,
            },
        }],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();

    replay_link.poll_emulator(&mut gb).unwrap();
    gb.write_byte(SERIAL_SB, 0xAB);
    gb.write_byte(SERIAL_SC, 0x81);
    replay_link.poll_emulator(&mut gb).unwrap();
    gb.step_frame();

    assert_eq!(gb.cpu_peek8(SERIAL_SB), 0x34);
    assert_eq!(gb.cpu_peek8(SERIAL_SC) & 0x80, 0);
    assert_eq!(gb.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
}

#[test]
fn game_boy_replay_link_validates_recorded_local_master_start() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0xAB,
                    serial_generation: 4,
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0100_0000_0000_0000,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 4,
                },
            },
        ],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();

    replay_link.poll_emulator(&mut gb).unwrap();
    gb.write_byte(SERIAL_SB, 0xAB);
    gb.write_byte(SERIAL_SC, 0x81);
    replay_link.poll_emulator(&mut gb).unwrap();

    assert!(replay_link.events.iter().all(|record| record.delivered));
    assert_eq!(replay_link.pending_master_transfer, None);
    gb.step_frame();
    assert_eq!(gb.cpu_peek8(SERIAL_SB), 0x34);
}

#[test]
fn game_boy_replay_link_does_not_arm_future_local_master_start() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![
            ReplayEvent::GameBoyLink {
                frame: 5,
                tick: 0,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0xAB,
                    serial_generation: 4,
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 5,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0100_0000_0000_0000,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 4,
                },
            },
        ],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();

    replay_link.poll_emulator(&mut gb).unwrap();
    gb.write_byte(SERIAL_SB, 0xAB);
    gb.write_byte(SERIAL_SC, 0x81);
    replay_link.poll_emulator(&mut gb).unwrap();

    assert_eq!(replay_link.pending_master_transfer, None);
    assert!(replay_link.events.iter().all(|record| !record.delivered));
}

#[test]
fn game_boy_replay_link_preserves_recorded_peer_presence_before_remote_master() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![ReplayEvent::GameBoyLink {
            frame: 5,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: 0x0100_0000_0000_0000,
                clock_period_t_cycles: 4096,
                out_byte: 0xAB,
                serial_generation: 4,
                local_reply: None,
            },
        }],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();
    gb.restore_game_boy_link_replay_state(zeff_emu_common::replay::ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        serial_generation: 0,
    });

    replay_link.poll_emulator(&mut gb).unwrap();

    assert!(gb.game_boy_link_replay_state().peer_present);
    assert_eq!(replay_link.pending_master_transfer, None);
    assert!(replay_link.events.iter().all(|record| !record.delivered));
}

#[test]
fn game_boy_replay_link_expands_relative_ticks_from_playback_start() {
    let replay_link = GameBoyReplayLink::new(
        vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 123,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: 0x0100_0000_0000_0000,
                out_byte: 0x34,
                passive: true,
                serial_generation: 4,
            },
        }],
        0,
        Some(9_000),
        5_000,
    );

    assert_eq!(replay_link.absolute_event_tick(123), 5_123);
}

#[test]
fn game_boy_replay_link_rejects_local_master_serial_generation_mismatch() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0xAB,
                    serial_generation: 999,
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0100_0000_0000_0000,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 999,
                },
            },
        ],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();

    replay_link.poll_emulator(&mut gb).unwrap();
    gb.write_byte(SERIAL_SB, 0xAB);
    gb.write_byte(SERIAL_SC, 0x81);

    assert_eq!(
        replay_link.poll_emulator(&mut gb),
        Err(LinkSessionError::MalformedPacketPayload)
    );
    assert!(replay_link.events.iter().all(|record| !record.delivered));
}

#[test]
fn game_boy_replay_link_does_not_consume_due_reply_before_local_master() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: 0x0100_0000_0000_0000,
                out_byte: 0x34,
                passive: true,
                serial_generation: 4,
            },
        }],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();

    replay_link.poll_emulator(&mut gb).unwrap();

    assert!(!replay_link.events[0].delivered);
    assert_eq!(replay_link.pending_master_transfer, None);
}

#[test]
fn game_boy_replay_link_binds_matching_reply_before_recorded_frame_boundary() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![ReplayEvent::GameBoyLink {
            frame: 5,
            tick: u64::MAX,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: 0x0100_0000_0000_0000,
                out_byte: 0x34,
                passive: true,
                serial_generation: 4,
            },
        }],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();

    replay_link.poll_emulator(&mut gb).unwrap();
    gb.write_byte(SERIAL_SB, 0xAB);
    gb.write_byte(SERIAL_SC, 0x81);
    replay_link.poll_emulator(&mut gb).unwrap();

    assert_eq!(replay_link.pending_master_transfer, None);
    assert!(replay_link.events[0].delivered);
    gb.step_frame();
    assert_eq!(gb.cpu_peek8(SERIAL_SB), 0x34);
}

#[test]
fn game_boy_replay_link_does_not_synthesize_local_transfer_from_restored_sc() {
    let mut replay_link = GameBoyReplayLink::new(Vec::new(), 0, None, 0);
    let mut gb = gb_emulator();

    gb.write_byte(SERIAL_SB, 0xAB);
    gb.write_byte(SERIAL_SC, 0x81);
    let state = gb.encode_state_bytes().unwrap();
    gb.load_state_from_bytes(state).unwrap();

    replay_link.poll_emulator(&mut gb).unwrap();

    assert_eq!(gb.cpu_peek8(SERIAL_SC) & 0x80, 0x80);
    assert_eq!(replay_link.pending_master_transfer, None);
}

#[test]
fn game_boy_replay_link_handles_simultaneous_remote_master_before_reply() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: 0x0200_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x56,
                    serial_generation: 4,
                    local_reply: None,
                },
            },
            ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 0,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0100_0000_0000_0000,
                    out_byte: 0x34,
                    passive: false,
                    serial_generation: 4,
                },
            },
        ],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();

    replay_link.poll_emulator(&mut gb).unwrap();
    gb.write_byte(SERIAL_SB, 0xAB);
    gb.write_byte(SERIAL_SC, 0x81);
    replay_link.poll_emulator(&mut gb).unwrap();
    gb.step_frame();

    assert_eq!(gb.cpu_peek8(SERIAL_SB), 0x34);
    assert_eq!(gb.cpu_peek8(SERIAL_SC) & 0x80, 0);
    assert_eq!(gb.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
}

#[test]
fn game_boy_replay_link_rejects_remote_master_local_reply_mismatch() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: 0x0100_0000_0000_0000,
                clock_period_t_cycles: 4096,
                out_byte: 0x56,
                serial_generation: 4,
                local_reply: Some(ReplayGameBoyLinkReply {
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 4,
                }),
            },
        }],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();

    assert_eq!(
        replay_link.poll_emulator(&mut gb),
        Err(LinkSessionError::MalformedPacketPayload)
    );
}

#[test]
fn game_boy_replay_link_rejects_remote_master_local_reply_generation_mismatch() {
    let mut replay_link = GameBoyReplayLink::new(
        vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: 0x0100_0000_0000_0000,
                clock_period_t_cycles: 4096,
                out_byte: 0x56,
                serial_generation: 4,
                local_reply: Some(ReplayGameBoyLinkReply {
                    out_byte: 0xFF,
                    passive: false,
                    serial_generation: 99,
                }),
            },
        }],
        0,
        None,
        0,
    );
    let mut gb = gb_emulator();

    assert_eq!(
        replay_link.poll_emulator(&mut gb),
        Err(LinkSessionError::MalformedPacketPayload)
    );
}

#[test]
fn game_boy_remote_link_does_not_spin_wait_for_early_pending_reply() {
    let (left_transport, _right_transport) = LocalLinkTransport::pair();
    let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
        left_transport,
        LinkSystemType::GameBoy,
        LinkEndpointId(1),
    ));
    let mut left = gb_emulator();

    left.set_game_boy_link_peer_present(true);
    left.write_byte(SERIAL_SB, 0xAB);
    left.write_byte(SERIAL_SC, 0x81);

    left_link.poll_emulator(&mut left).unwrap();

    assert_eq!(
        left_link.pending_master_transfer_id(),
        Some(0x0100_0000_0000_0000)
    );
    assert!(!left.game_boy_link_waiting_at_completion_boundary());
    assert_eq!(left.cpu_peek8(SERIAL_SC) & 0x80, 0x80);
    assert_eq!(left.cpu_peek8(INTERRUPT_IF) & 0x08, 0);
}

#[test]
fn game_boy_remote_link_catches_up_passive_rearm_before_queued_master_start() {
    let (left_transport, right_transport) = LocalLinkTransport::pair();
    let mut left_session =
        LinkSession::new(left_transport, LinkSystemType::GameBoy, LinkEndpointId(1));
    let mut right_link = GameBoyRemoteLink::new(LinkSession::new(
        right_transport,
        LinkSystemType::GameBoy,
        LinkEndpointId(2),
    ));
    let mut right = gb_emulator_with_serial_rearm_isr(0x56);

    right.set_game_boy_link_peer_present(true);
    right.write_byte(SERIAL_SB, 0x34);
    right.write_byte(SERIAL_SC, 0x80);
    queue_master_start(&mut left_session, 0x0100_0000_0000_0000, 0xAB);
    queue_master_start(&mut left_session, 0x0100_0000_0000_0001, 0xCD);

    right_link.poll_emulator(&mut right).unwrap();

    let first = receive_reply(&mut left_session);
    let GameBoyLinkEvent::TransferReply {
        transfer_id, reply, ..
    } = first
    else {
        panic!("expected first transfer reply, got {first:?}");
    };
    assert_eq!(transfer_id, GbTransferId(0x0100_0000_0000_0000));
    assert_eq!(reply.out_byte, 0x34);
    assert!(reply.passive);

    let second = receive_reply(&mut left_session);
    let GameBoyLinkEvent::TransferReply {
        transfer_id, reply, ..
    } = second
    else {
        panic!("expected second transfer reply, got {second:?}");
    };
    assert_eq!(transfer_id, GbTransferId(0x0100_0000_0000_0001));
    assert_eq!(reply.out_byte, 0x56);
    assert!(reply.passive);
    assert_eq!(right.cpu_peek8(SERIAL_SB), 0xCD);
    assert_eq!(right.cpu_peek8(SERIAL_SC) & 0x80, 0);
    assert_eq!(right.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
}

#[test]
fn game_boy_remote_link_rejects_unmatched_reply() {
    let (left_transport, right_transport) = LocalLinkTransport::pair();
    let mut left_link = GameBoyRemoteLink::new(LinkSession::new(
        left_transport,
        LinkSystemType::GameBoy,
        LinkEndpointId(1),
    ));
    let mut right_session =
        LinkSession::new(right_transport, LinkSystemType::GameBoy, LinkEndpointId(2));
    let mut left = gb_emulator();

    right_session
        .send(
            LinkPacketKind::LinkEvent,
            &encode_game_boy_link_event(GameBoyLinkEvent::TransferReply {
                transfer_id: GbTransferId(0x0200_0000_0000_0001),
                sample_tick: 0,
                reply: GameBoyLinkReply {
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 0,
                },
            }),
        )
        .unwrap();

    assert_eq!(
        left_link.poll_emulator(&mut left),
        Err(LinkSessionError::MalformedPacketPayload)
    );
}

fn gb_emulator() -> GameBoyEmulator {
    let rom = vec![0u8; 0x8000];
    GameBoyEmulator::from_rom_data(&rom, HardwareModePreference::Auto)
        .expect("GB emulator should initialize")
}

fn gb_emulator_with_serial_rearm_isr(next_reply: u8) -> GameBoyEmulator {
    let mut rom = vec![0u8; 0x8000];
    rom[0x0058..0x0061]
        .copy_from_slice(&[0x3E, next_reply, 0xE0, 0x01, 0x3E, 0x80, 0xE0, 0x02, 0xD9]);
    rom[0x0100..0x0105].copy_from_slice(&[0xFB, 0x00, 0x76, 0x18, 0xFD]);
    let mut emulator = GameBoyEmulator::from_rom_data(&rom, HardwareModePreference::Auto)
        .expect("GB emulator should initialize");
    emulator.write_byte(IE_ADDR, 0x08);
    emulator.step_instruction();
    emulator.step_instruction();
    emulator.step_instruction();
    emulator
}

fn queue_master_start(
    session: &mut LinkSession<LocalLinkTransport>,
    transfer_id: u64,
    out_byte: u8,
) {
    session
        .send(
            LinkPacketKind::LinkEvent,
            &encode_game_boy_link_event(GameBoyLinkEvent::MasterStart {
                transfer_id: GbTransferId(transfer_id),
                start_tick: 0,
                action: GameBoyLinkAction {
                    out_byte,
                    clock_period_t_cycles: 4096,
                    serial_generation: 4,
                },
            }),
        )
        .unwrap();
}

fn receive_reply(session: &mut LinkSession<LocalLinkTransport>) -> GameBoyLinkEvent {
    let packet = session
        .try_receive_packet()
        .unwrap()
        .expect("reply packet should be queued");
    assert_eq!(packet.kind, LinkPacketKind::LinkEvent);
    decode_game_boy_link_event(&packet.payload).expect("reply payload should decode")
}

#[allow(dead_code)]
fn gb_backend() -> EmuBackend {
    EmuBackend::from_gb(gb_emulator(), PathBuf::from("test.gb"))
}
