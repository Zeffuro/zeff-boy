use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use zeff_emu_common::replay::{
    ReplayCheckpoint, ReplayGameBoyLinkAction, ReplayGameBoyLinkCoordinatorOwner,
    ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkEvent, ReplayMetadata, ReplayRecorder,
};
use zeff_firmware::{sha256_bytes, sha256_hex};
use zeff_gb_core::hardware::types::constants::{SERIAL_SB, SERIAL_SC};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::*;
use crate::cli::headless_runner::replay::paired_lease::{
    PairedGameBoyFrameLease, PairedGameBoyFrameLeaseOutcome,
};
use crate::cli::headless_runner::replay::paired_plan::{PairedPlanError, PairedTransferPlan};

static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);
const TRANSFER_ID: u64 = 0x0100_0000_0000_0001;
const RIGHT_TRANSFER_ID: u64 = 0x0200_0000_0000_0001;

fn backend(path: &str) -> EmuBackend {
    backend_from_rom(path, vec![0u8; 0x8000])
}

fn backend_from_rom(path: &str, rom: Vec<u8>) -> EmuBackend {
    let emulator =
        zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
            .unwrap();
    EmuBackend::from_gb(emulator, PathBuf::from(path))
}

fn player(
    events: Vec<ReplayEvent>,
    tick: u64,
    label: &str,
    checkpoint: Option<[u8; 32]>,
) -> ReplayPlayer {
    let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "zeff-paired-direct-{label}-{}-{id}.zrpl",
        std::process::id()
    ));
    let metadata = ReplayMetadata {
        events,
        game_boy_link_start_tick: Some(tick),
        checkpoints: checkpoint
            .map(|state_sha256| {
                vec![ReplayCheckpoint {
                    frame: 1,
                    state_sha256,
                }]
            })
            .unwrap_or_default(),
        ..ReplayMetadata::default()
    };
    let mut recorder = ReplayRecorder::new_with_metadata(path.clone(), Vec::new(), metadata);
    recorder.record_frame(0, 0);
    recorder.finish().unwrap();
    let player = ReplayPlayer::load(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    player
}

fn player_with_start_state(tick: u64, label: &str, state: ReplayGameBoyLinkState) -> ReplayPlayer {
    player_with_start_state_and_events(tick, label, state, Vec::new())
}

fn player_with_start_state_and_events(
    tick: u64,
    label: &str,
    state: ReplayGameBoyLinkState,
    events: Vec<ReplayEvent>,
) -> ReplayPlayer {
    let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "zeff-paired-direct-{label}-{}-{id}.zrpl",
        std::process::id()
    ));
    let metadata = ReplayMetadata {
        events,
        game_boy_link_start_state: Some(state),
        game_boy_link_start_tick: Some(tick),
        ..ReplayMetadata::default()
    };
    let mut recorder = ReplayRecorder::new_with_metadata(path.clone(), Vec::new(), metadata);
    recorder.record_frame(0, 0);
    recorder.finish().unwrap();
    let player = ReplayPlayer::load(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    player
}

fn player_with_master_continuation(
    events: Vec<ReplayEvent>,
    tick: u64,
    label: &str,
    state: ReplayGameBoyLinkState,
    coordinator: ReplayGameBoyLinkCoordinatorState,
) -> ReplayPlayer {
    let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "zeff-paired-direct-{label}-{}-{id}.zrpl",
        std::process::id()
    ));
    let metadata = ReplayMetadata {
        events,
        game_boy_link_start_state: Some(state),
        game_boy_link_coordinator_start_state: Some(coordinator),
        game_boy_link_start_tick: Some(tick),
        ..ReplayMetadata::default()
    };
    let mut recorder = ReplayRecorder::new_with_metadata(path.clone(), Vec::new(), metadata);
    recorder.record_frame(0, 0);
    recorder.finish().unwrap();
    let player = ReplayPlayer::load(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    player
}

fn configured_pair() -> (EmuBackend, EmuBackend) {
    let mut left = backend("left.gb");
    let mut right = backend("right.gb");
    let EmuBackend::Gb(left_gb) = &mut left else {
        unreachable!();
    };
    left_gb.emu.write_byte(SERIAL_SB, 0xAB);
    left_gb.emu.write_byte(SERIAL_SC, 0x81);
    let EmuBackend::Gb(right_gb) = &mut right else {
        unreachable!();
    };
    right_gb.emu.write_byte(SERIAL_SB, 0x34);
    right_gb.emu.write_byte(SERIAL_SC, 0x80);
    (left, right)
}

fn configured_crossed_pair() -> (EmuBackend, EmuBackend) {
    let mut left = backend("crossed-left.gb");
    let mut right = backend("crossed-right.gb");
    let EmuBackend::Gb(left_gb) = &mut left else {
        unreachable!();
    };
    left_gb.emu.write_byte(SERIAL_SB, 0xAB);
    left_gb.emu.write_byte(SERIAL_SC, 0x81);
    let EmuBackend::Gb(right_gb) = &mut right else {
        unreachable!();
    };
    right_gb.emu.write_byte(SERIAL_SB, 0x34);
    right_gb.emu.write_byte(SERIAL_SC, 0x81);
    (left, right)
}

fn crossed_fixture() -> (EmuBackend, EmuBackend, ReplayPlayer, ReplayPlayer) {
    let (left, right) = configured_crossed_pair();

    let start_tick = left.game_boy_cpu_cycles().unwrap();
    assert_eq!(right.game_boy_cpu_cycles(), Some(start_tick));
    let preview = left.preview_game_boy_link_peer(&right).unwrap();
    let left_action = preview.local_action.unwrap();
    let right_action = preview.peer_action.unwrap();
    assert!(!preview.local_reply.passive);
    assert!(!preview.peer_reply.passive);

    let left_events = vec![
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: TRANSFER_ID,
                clock_period_t_cycles: left_action.clock_period_t_cycles,
                out_byte: left_action.out_byte,
                serial_generation: left_action.serial_generation,
            },
        },
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: RIGHT_TRANSFER_ID,
                clock_period_t_cycles: right_action.clock_period_t_cycles,
                out_byte: right_action.out_byte,
                serial_generation: right_action.serial_generation,
                local_reply: Some(ReplayGameBoyLinkReply {
                    out_byte: preview.local_reply.out_byte,
                    passive: preview.local_reply.passive,
                    serial_generation: preview.local_reply.serial_generation,
                }),
            },
        },
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: TRANSFER_ID,
                out_byte: preview.peer_reply.out_byte,
                passive: preview.peer_reply.passive,
                serial_generation: preview.peer_reply.serial_generation,
            },
        },
    ];
    let right_events = vec![
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: RIGHT_TRANSFER_ID,
                clock_period_t_cycles: right_action.clock_period_t_cycles,
                out_byte: right_action.out_byte,
                serial_generation: right_action.serial_generation,
            },
        },
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: TRANSFER_ID,
                clock_period_t_cycles: left_action.clock_period_t_cycles,
                out_byte: left_action.out_byte,
                serial_generation: left_action.serial_generation,
                local_reply: Some(ReplayGameBoyLinkReply {
                    out_byte: preview.peer_reply.out_byte,
                    passive: preview.peer_reply.passive,
                    serial_generation: preview.peer_reply.serial_generation,
                }),
            },
        },
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 0,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: RIGHT_TRANSFER_ID,
                out_byte: preview.local_reply.out_byte,
                passive: preview.local_reply.passive,
                serial_generation: preview.local_reply.serial_generation,
            },
        },
    ];
    (
        left,
        right,
        player(left_events, start_tick, "crossed-left", None),
        player(right_events, start_tick, "crossed-right", None),
    )
}

fn fixture(
    event_tick: u64,
    reply_tick: u64,
    checkpoints: (Option<[u8; 32]>, Option<[u8; 32]>),
) -> (EmuBackend, EmuBackend, ReplayPlayer, ReplayPlayer) {
    let (left, right) = configured_pair();
    let start_tick = left.game_boy_cpu_cycles().unwrap();
    assert_eq!(right.game_boy_cpu_cycles(), Some(start_tick));
    let preview = left.preview_game_boy_link_peer(&right).unwrap();
    let action = preview.local_action.unwrap();
    let reply = preview.peer_reply;
    let left_events = vec![
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: event_tick,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: TRANSFER_ID,
                clock_period_t_cycles: action.clock_period_t_cycles,
                out_byte: action.out_byte,
                serial_generation: action.serial_generation,
            },
        },
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: reply_tick,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: TRANSFER_ID,
                out_byte: reply.out_byte,
                passive: reply.passive,
                serial_generation: reply.serial_generation,
            },
        },
    ];
    let right_events = vec![ReplayEvent::GameBoyLink {
        frame: 0,
        tick: event_tick,
        event: ReplayGameBoyLinkEvent::RemoteMasterStart {
            transfer_id: TRANSFER_ID,
            clock_period_t_cycles: action.clock_period_t_cycles,
            out_byte: action.out_byte,
            serial_generation: action.serial_generation,
            local_reply: Some(ReplayGameBoyLinkReply {
                out_byte: reply.out_byte,
                passive: reply.passive,
                serial_generation: reply.serial_generation,
            }),
        },
    }];
    (
        left,
        right,
        player(left_events, start_tick, "left", checkpoints.0),
        player(right_events, start_tick, "right", checkpoints.1),
    )
}

fn run_fixture() -> DirectPairedReplayResult {
    let (left, right, left_player, right_player) = fixture(0, 0, (None, None));
    let plan = PairedTransferPlan::build(
        &left_player.metadata().events,
        left_player.metadata().game_boy_link_start_tick,
        &right_player.metadata().events,
        right_player.metadata().game_boy_link_start_tick,
    )
    .unwrap();
    run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap()
}

#[test]
fn singleton_execution_is_exact_and_deterministic() {
    let first = run_fixture();
    let second = run_fixture();
    assert_eq!(first.left_generated_link_events.len(), 2);
    assert_eq!(first.right_generated_link_events.len(), 1);
    assert_eq!(
        first.left_generated_link_events,
        second.left_generated_link_events
    );
    assert_eq!(
        first.right_generated_link_events,
        second.right_generated_link_events
    );
    assert_eq!(first.left_final_state_hash, second.left_final_state_hash);
    assert_eq!(first.right_final_state_hash, second.right_final_state_hash);
}

#[test]
fn crossed_master_result_matches_core_exchange_oracle() {
    let (left, right, left_player, right_player) = crossed_fixture();
    let plan = PairedTransferPlan::build(
        &left_player.metadata().events,
        left_player.metadata().game_boy_link_start_tick,
        &right_player.metadata().events,
        right_player.metadata().game_boy_link_start_tick,
    )
    .unwrap();
    let result = run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap();

    let (mut oracle_left, mut oracle_right) = configured_crossed_pair();
    let (EmuBackend::Gb(left_gb), EmuBackend::Gb(right_gb)) = (&mut oracle_left, &mut oracle_right)
    else {
        unreachable!();
    };
    left_gb
        .emu
        .try_sync_game_boy_link_peer(&mut right_gb.emu)
        .unwrap();
    oracle_left.step_frame();
    oracle_right.step_frame();

    assert_eq!(
        result.left_final_state_hash,
        sha256_hex(&oracle_left.encode_replay_hash_state_bytes().unwrap())
    );
    assert_eq!(
        result.right_final_state_hash,
        sha256_hex(&oracle_right.encode_replay_hash_state_bytes().unwrap())
    );
    assert_eq!(
        result.left_final_framebuffer_hash,
        sha256_hex(oracle_left.framebuffer())
    );
    assert_eq!(
        result.right_final_framebuffer_hash,
        sha256_hex(oracle_right.framebuffer())
    );
}

#[test]
fn crossed_masters_execute_atomically_and_deterministically() {
    fn run() -> DirectPairedReplayResult {
        let (left, right, left_player, right_player) = crossed_fixture();
        let plan = PairedTransferPlan::build(
            &left_player.metadata().events,
            left_player.metadata().game_boy_link_start_tick,
            &right_player.metadata().events,
            right_player.metadata().game_boy_link_start_tick,
        )
        .unwrap();
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(plan.batches[0].transfers.len(), 2);
        run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap()
    }

    let first = run();
    let second = run();
    assert_eq!(first.left_generated_link_events.len(), 3);
    assert_eq!(first.right_generated_link_events.len(), 3);
    assert_eq!(
        first.left_generated_link_events,
        second.left_generated_link_events
    );
    assert_eq!(
        first.right_generated_link_events,
        second.right_generated_link_events
    );
    assert_eq!(first.left_final_state_hash, second.left_final_state_hash);
    assert_eq!(first.right_final_state_hash, second.right_final_state_hash);
}

#[test]
fn crossed_master_batch_rejects_delayed_remote_observation() {
    let (left, right, left_player, right_player) = crossed_fixture();
    let mut left_events = left_player.metadata().events.clone();
    for event in &mut left_events[1..=2] {
        let ReplayEvent::GameBoyLink { frame, tick, .. } = event else {
            unreachable!();
        };
        *frame = 1;
        *tick = 4;
    }
    let start_tick = left_player.metadata().game_boy_link_start_tick.unwrap();
    let left_player = player(left_events, start_tick, "crossed-delayed-left", None);
    let plan = PairedTransferPlan::build(
        &left_player.metadata().events,
        left_player.metadata().game_boy_link_start_tick,
        &right_player.metadata().events,
        right_player.metadata().game_boy_link_start_tick,
    )
    .unwrap();

    assert!(matches!(
        run_direct_paired_replay(left, right, left_player, right_player, plan, 0),
        Err(DirectCoordinatorError::DelayedReply {
            side: Side::Left,
            id: TRANSFER_ID,
            ..
        })
    ));
}

#[test]
fn delayed_reply_is_rejected_before_execution() {
    let (left, right, left_player, right_player) = fixture(0, 4, (None, None));
    let plan = PairedTransferPlan::build(
        &left_player.metadata().events,
        left_player.metadata().game_boy_link_start_tick,
        &right_player.metadata().events,
        right_player.metadata().game_boy_link_start_tick,
    )
    .unwrap();
    assert!(matches!(
        run_direct_paired_replay(left, right, left_player, right_player, plan, 0),
        Err(DirectCoordinatorError::DelayedReply { .. })
    ));
}

#[test]
fn frame_advanced_reply_requires_a_frame_complete_start_without_mutation() {
    let (left, right, left_player, right_player) = fixture(0, 0, (None, None));
    let mut left_events = left_player.metadata().events.clone();
    let ReplayEvent::GameBoyLink { frame, .. } = &mut left_events[1] else {
        unreachable!();
    };
    *frame = 1;
    let start_tick = left_player.metadata().game_boy_link_start_tick.unwrap();
    let left_player = player(left_events, start_tick, "boundary-required-left", None);
    let plan = PairedTransferPlan::build(
        &left_player.metadata().events,
        left_player.metadata().game_boy_link_start_tick,
        &right_player.metadata().events,
        right_player.metadata().game_boy_link_start_tick,
    )
    .unwrap();
    let transfer = plan.batches[0].transfers[0].clone();
    let expected_action = transfer.start(transfer.master).action();
    let mut left = DirectSide::new(Side::Left, left, left_player, 0).unwrap();
    let mut right = DirectSide::new(Side::Right, right, right_player, 0).unwrap();
    left.reach(&transfer.left_start, Some(expected_action))
        .unwrap();
    right.reach(&transfer.right_start, None).unwrap();
    let left_before = left.backend.encode_replay_hash_state_bytes().unwrap();
    let right_before = right.backend.encode_replay_hash_state_bytes().unwrap();

    assert!(matches!(
        left.validate_reply_observation_shape(&transfer.master_reply),
        Err(DirectCoordinatorError::ReplyObservationRequiresStep {
            side: Side::Left,
            frame: 1
        })
    ));
    assert_eq!(
        left.backend.encode_replay_hash_state_bytes().unwrap(),
        left_before
    );
    assert_eq!(
        right.backend.encode_replay_hash_state_bytes().unwrap(),
        right_before
    );
}

#[test]
fn timed_idle_state_is_applied_at_its_exact_point() {
    let backend = backend("timed-state.gb");
    let start_tick = backend.game_boy_cpu_cycles().unwrap();
    let state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 7,
    };
    let player = player(
        vec![ReplayEvent::GameBoyLinkStateAtTick {
            frame: 0,
            tick: 4,
            state,
        }],
        start_tick,
        "timed-state",
        None,
    );
    let mut side = DirectSide::new(Side::Left, backend, player, 0).unwrap();

    side.apply_state_events_before(1).unwrap();

    assert_eq!(side.source_cursor, 1);
    assert_eq!(side.backend.game_boy_cpu_cycles(), Some(start_tick + 4));
    assert_eq!(side.backend.game_boy_link_replay_state(), Some(state));
    assert!(!side.lease.needs_frame_setup());
}

#[test]
fn passive_in_flight_start_matches_independent_core_oracle() {
    let left = backend("passive-start-left.gb");
    let mut right = backend("passive-start-right.gb");
    let EmuBackend::Gb(right_gb) = &mut right else {
        unreachable!();
    };
    right_gb.emu.write_byte(SERIAL_SB, 0x34);
    right_gb.emu.write_byte(SERIAL_SC, 0x80);
    let start_tick = left.game_boy_cpu_cycles().unwrap();
    assert_eq!(right.game_boy_cpu_cycles(), Some(start_tick));
    let state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: Some(zeff_emu_common::replay::ReplayGameBoyPassiveCompletion {
            peer_byte: 0xAB,
            remaining_t_cycles: 8,
        }),
        serial_generation: 0,
    };

    let mut expected_left = backend("passive-start-left-oracle.gb");
    let mut expected_right = backend("passive-start-right-oracle.gb");
    let EmuBackend::Gb(expected_right_gb) = &mut expected_right else {
        unreachable!();
    };
    expected_right_gb.emu.write_byte(SERIAL_SB, 0x34);
    expected_right_gb.emu.write_byte(SERIAL_SC, 0x80);
    assert!(expected_right.restore_game_boy_link_replay_state(state));
    expected_left.step_frame();
    expected_right.step_frame();
    let expected_left_hash = sha256_hex(&expected_left.encode_replay_hash_state_bytes().unwrap());
    let expected_right_hash = sha256_hex(&expected_right.encode_replay_hash_state_bytes().unwrap());

    let left_player = player(Vec::new(), start_tick, "passive-start-left", None);
    let right_player = player_with_start_state(start_tick, "passive-start-right", state);
    let plan = PairedTransferPlan::start_only(
        left_player.metadata().game_boy_link_start_tick,
        right_player.metadata().game_boy_link_start_tick,
    )
    .unwrap();
    let result = run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap();

    assert_eq!(result.left_final_state_hash, expected_left_hash);
    assert_eq!(result.right_final_state_hash, expected_right_hash);
}

#[test]
fn local_master_continuations_match_independent_core_oracle() {
    for owner in [
        ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply,
        ReplayGameBoyLinkCoordinatorOwner::CoreHasReply,
    ] {
        let (mut left, mut right) = configured_pair();
        left.set_link_peer_present(true);
        right.set_link_peer_present(true);
        let action = left
            .game_boy_link_replay_state()
            .unwrap()
            .queued_master_action
            .unwrap();
        let EmuBackend::Gb(right_gb) = &mut right else {
            unreachable!();
        };
        let reply = right_gb.emu.game_boy_link_reply_to_master_start();
        assert!(reply.passive);
        right_gb.emu.write_byte(SERIAL_SB, 0x99);
        let replay_reply = ReplayGameBoyLinkReply {
            out_byte: reply.out_byte,
            passive: reply.passive,
            serial_generation: reply.serial_generation,
        };
        let left_state = ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: Some(action.out_byte),
            pending_master_response: (owner == ReplayGameBoyLinkCoordinatorOwner::CoreHasReply)
                .then_some(reply.out_byte),
            pending_master_completion_ready: false,
            queued_master_action: None,
            pending_passive_completion: None,
            serial_generation: action.serial_generation,
        };
        let right_generation = right
            .game_boy_link_replay_state()
            .unwrap()
            .serial_generation;
        let right_state = ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: None,
            pending_master_response: None,
            pending_master_completion_ready: false,
            queued_master_action: None,
            pending_passive_completion: Some(
                zeff_emu_common::replay::ReplayGameBoyPassiveCompletion {
                    peer_byte: action.out_byte,
                    remaining_t_cycles: action.clock_period_t_cycles,
                },
            ),
            serial_generation: right_generation,
        };
        assert!(left.restore_game_boy_link_replay_state(left_state));
        assert!(right.restore_game_boy_link_replay_state(right_state));
        let coordinator = ReplayGameBoyLinkCoordinatorState {
            transfer_id: TRANSFER_ID,
            action: ReplayGameBoyLinkAction {
                out_byte: action.out_byte,
                clock_period_t_cycles: action.clock_period_t_cycles,
                serial_generation: action.serial_generation,
            },
            owner,
            reply: (owner == ReplayGameBoyLinkCoordinatorOwner::CoreHasReply)
                .then_some(replay_reply),
        };
        let events = if owner == ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply {
            vec![ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 8,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: TRANSFER_ID,
                    out_byte: replay_reply.out_byte,
                    passive: replay_reply.passive,
                    serial_generation: replay_reply.serial_generation,
                },
            }]
        } else {
            Vec::new()
        };
        let start_tick = left.game_boy_cpu_cycles().unwrap();
        let left_player = player_with_master_continuation(
            events,
            start_tick,
            "master-continuation-left",
            left_state,
            coordinator,
        );
        let right_player =
            player_with_start_state(start_tick, "master-continuation-right", right_state);

        let (mut expected_left, mut expected_right) = configured_pair();
        let EmuBackend::Gb(expected_right_gb) = &mut expected_right else {
            unreachable!();
        };
        expected_right_gb.emu.write_byte(SERIAL_SB, 0x99);
        assert!(expected_left.restore_game_boy_link_replay_state(left_state));
        assert!(expected_right.restore_game_boy_link_replay_state(right_state));
        if owner == ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply {
            assert!(expected_left.apply_game_boy_link_reply(replay_reply));
        }
        expected_left.step_frame();
        expected_right.step_frame();
        let expected_left_hash =
            sha256_hex(&expected_left.encode_replay_hash_state_bytes().unwrap());
        let expected_right_hash =
            sha256_hex(&expected_right.encode_replay_hash_state_bytes().unwrap());

        let plan = crate::cli::headless_runner::replay::paired_plan::validate_paired_transfer_plan(
            &left_player,
            &right_player,
        )
        .unwrap();
        let result =
            run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap();
        assert_eq!(result.left_final_state_hash, expected_left_hash);
        assert_eq!(result.right_final_state_hash, expected_right_hash);
    }
}

#[test]
fn queued_pre_start_state_matches_independent_core_oracle() {
    let (mut left, right, left_player, right_player) = fixture(0, 0, (None, None));
    left.set_link_peer_present(true);
    let queued_state = left.game_boy_link_replay_state().unwrap();
    assert!(queued_state.queued_master_action.is_some());
    let start_tick = left_player.metadata().game_boy_link_start_tick.unwrap();
    let left_player = player_with_start_state_and_events(
        start_tick,
        "queued-pre-start-left",
        queued_state,
        left_player.metadata().events.clone(),
    );
    let plan = crate::cli::headless_runner::replay::paired_plan::validate_paired_transfer_plan(
        &left_player,
        &right_player,
    )
    .unwrap();

    let (mut expected_left, mut expected_right) = configured_pair();
    expected_left.set_link_peer_present(true);
    assert!(expected_left.sync_link_peer(&mut expected_right));
    expected_left.step_frame();
    expected_right.step_frame();
    let expected_left_hash = sha256_hex(&expected_left.encode_replay_hash_state_bytes().unwrap());
    let expected_right_hash = sha256_hex(&expected_right.encode_replay_hash_state_bytes().unwrap());

    let result = run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap();
    assert_eq!(result.left_final_state_hash, expected_left_hash);
    assert_eq!(result.right_final_state_hash, expected_right_hash);
}

#[test]
fn paired_planner_rejects_invalid_initial_master_pairings() {
    let start_tick = backend("initial-invalid-tick.gb")
        .game_boy_cpu_cycles()
        .unwrap();
    let action = ReplayGameBoyLinkAction {
        out_byte: 0xAB,
        clock_period_t_cycles: 64,
        serial_generation: 7,
    };
    let reply = ReplayGameBoyLinkReply {
        out_byte: 0x34,
        passive: true,
        serial_generation: 9,
    };
    let master_state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: Some(action.out_byte),
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: action.serial_generation,
    };
    let coordinator = ReplayGameBoyLinkCoordinatorState {
        transfer_id: TRANSFER_ID,
        action,
        owner: ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply,
        reply: None,
    };
    let master_player = |label: &str, passive: bool| {
        player_with_master_continuation(
            vec![ReplayEvent::GameBoyLink {
                frame: 0,
                tick: 8,
                event: ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: TRANSFER_ID,
                    out_byte: reply.out_byte,
                    passive,
                    serial_generation: reply.serial_generation,
                },
            }],
            start_tick,
            label,
            master_state,
            coordinator,
        )
    };
    let passive_player = |label: &str, peer_byte: u8, remaining_t_cycles: u64| {
        player_with_start_state(
            start_tick,
            label,
            ReplayGameBoyLinkState {
                peer_present: true,
                pending_master_byte: None,
                pending_master_response: None,
                pending_master_completion_ready: false,
                queued_master_action: None,
                pending_passive_completion: Some(
                    zeff_emu_common::replay::ReplayGameBoyPassiveCompletion {
                        peer_byte,
                        remaining_t_cycles,
                    },
                ),
                serial_generation: 3,
            },
        )
    };

    for peer in [
        player(Vec::new(), start_tick, "initial-missing-passive", None),
        passive_player("initial-byte-mismatch", action.out_byte ^ 0xFF, 32),
        passive_player("initial-period-mismatch", action.out_byte, 65),
    ] {
        assert!(matches!(
            crate::cli::headless_runner::replay::paired_plan::validate_paired_transfer_plan(
                &master_player("initial-invalid-master", true),
                &peer,
            ),
            Err(PairedPlanError::InvalidInitialContinuation { side: Side::Left })
        ));
    }
    assert!(matches!(
        crate::cli::headless_runner::replay::paired_plan::validate_paired_transfer_plan(
            &master_player("initial-nonpassive", false),
            &passive_player("initial-nonpassive-peer", action.out_byte, 32),
        ),
        Err(PairedPlanError::InvalidInitialContinuation { side: Side::Left })
    ));

    let right_coordinator = ReplayGameBoyLinkCoordinatorState {
        transfer_id: RIGHT_TRANSFER_ID,
        action,
        ..coordinator
    };
    let right_master = player_with_master_continuation(
        vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 8,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: RIGHT_TRANSFER_ID,
                out_byte: reply.out_byte,
                passive: true,
                serial_generation: reply.serial_generation,
            },
        }],
        start_tick,
        "initial-double-master-right",
        master_state,
        right_coordinator,
    );
    assert!(matches!(
        crate::cli::headless_runner::replay::paired_plan::validate_paired_transfer_plan(
            &master_player("initial-double-master-left", true),
            &right_master,
        ),
        Err(PairedPlanError::ConflictingInitialMasters)
    ));

    let (_, _, regular_left, regular_right) = fixture(0, 0, (None, None));
    let shift_after_continuation = |events: &[ReplayEvent]| {
        events
            .iter()
            .map(|event| match event {
                ReplayEvent::GameBoyLink { frame, tick, event } => ReplayEvent::GameBoyLink {
                    frame: frame + 1,
                    tick: tick + 16,
                    event: *event,
                },
                _ => event.clone(),
            })
            .collect::<Vec<_>>()
    };
    let regular_peer = player_with_start_state_and_events(
        start_tick,
        "initial-counter-peer",
        passive_player("initial-counter-peer-state", action.out_byte, 32)
            .metadata()
            .game_boy_link_start_state
            .unwrap(),
        shift_after_continuation(&regular_right.metadata().events),
    );
    let continuation_with_regular = |label: &str, transfer_id: u64| {
        let mut events = vec![ReplayEvent::GameBoyLink {
            frame: 0,
            tick: 8,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id,
                out_byte: reply.out_byte,
                passive: true,
                serial_generation: reply.serial_generation,
            },
        }];
        events.extend(shift_after_continuation(&regular_left.metadata().events));
        player_with_master_continuation(
            events,
            start_tick,
            label,
            master_state,
            ReplayGameBoyLinkCoordinatorState {
                transfer_id,
                ..coordinator
            },
        )
    };
    assert!(matches!(
        crate::cli::headless_runner::replay::paired_plan::validate_paired_transfer_plan(
            &continuation_with_regular("initial-counter-regression", 0x0100_0000_0000_0002,),
            &regular_peer,
        ),
        Err(PairedPlanError::CounterOrder { side: Side::Left })
    ));
    assert!(matches!(
        crate::cli::headless_runner::replay::paired_plan::validate_paired_transfer_plan(
            &continuation_with_regular("initial-endpoint-change", RIGHT_TRANSFER_ID),
            &regular_peer,
        ),
        Err(PairedPlanError::EndpointChanged { side: Side::Left })
    ));
}

#[test]
fn start_only_pair_rejects_two_passive_states() {
    let start_tick = backend("double-passive-tick.gb")
        .game_boy_cpu_cycles()
        .unwrap();
    let left = backend("double-passive-left.gb");
    let right = backend("double-passive-right.gb");
    let passive_state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: Some(zeff_emu_common::replay::ReplayGameBoyPassiveCompletion {
            peer_byte: 0xAB,
            remaining_t_cycles: 8,
        }),
        serial_generation: 0,
    };
    let left_player = player_with_start_state(start_tick, "double-passive-left", passive_state);
    let right_player = player_with_start_state(start_tick, "double-passive-right", passive_state);
    let plan = PairedTransferPlan::start_only(Some(start_tick), Some(start_tick)).unwrap();
    assert!(matches!(
        run_direct_paired_replay(left, right, left_player, right_player, plan, 0),
        Err(DirectCoordinatorError::ConflictingPassiveStartStates)
    ));
}

#[test]
fn frame_complete_state_restore_precedes_checkpoint_and_commit() {
    let state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 9,
    };
    let mut expected = backend("state-checkpoint-expected.gb");
    let start_tick = expected.game_boy_cpu_cycles().unwrap();
    let mut expected_lease = PairedGameBoyFrameLease::default();
    expected_lease.begin(&expected, None).unwrap();
    assert_eq!(
        expected_lease
            .step_direct_until(&mut expected, None, false)
            .unwrap()
            .outcome,
        PairedGameBoyFrameLeaseOutcome::FrameComplete
    );
    let event_tick = expected.game_boy_cpu_cycles().unwrap() - start_tick;
    expected.restore_game_boy_link_replay_state(state);
    let checkpoint = sha256_bytes(&expected.encode_replay_hash_state_bytes().unwrap());
    let player = player(
        vec![ReplayEvent::GameBoyLinkStateAtTick {
            frame: 0,
            tick: event_tick,
            state,
        }],
        start_tick,
        "state-checkpoint",
        Some(checkpoint),
    );
    let mut side = DirectSide::new(Side::Left, backend("state-checkpoint.gb"), player, 0).unwrap();

    side.apply_state_events_before(1).unwrap();

    assert!(side.pending_frame_complete);
    assert_eq!(side.backend.game_boy_link_replay_state(), Some(state));
    side.settle_frame().unwrap();
    assert_eq!(side.advanced_frames, 1);
    assert!(side.lease.needs_frame_setup());
}

#[test]
fn state_payload_with_owned_transfer_is_rejected_in_preflight() {
    let (left, right, left_player, right_player) = fixture(0, 0, (None, None));
    let mut left_events = left_player.metadata().events.clone();
    left_events.insert(
        0,
        ReplayEvent::GameBoyLinkStateAtTick {
            frame: 0,
            tick: 0,
            state: ReplayGameBoyLinkState {
                peer_present: true,
                pending_master_byte: Some(0xAB),
                pending_master_response: None,
                pending_master_completion_ready: false,
                queued_master_action: None,
                pending_passive_completion: None,
                serial_generation: 4,
            },
        },
    );
    let start_tick = left_player.metadata().game_boy_link_start_tick.unwrap();
    let left_player = player(left_events, start_tick, "unsafe-state", None);
    let plan = PairedTransferPlan::build(
        &left_player.metadata().events,
        left_player.metadata().game_boy_link_start_tick,
        &right_player.metadata().events,
        right_player.metadata().game_boy_link_start_tick,
    )
    .unwrap();

    assert!(matches!(
        run_direct_paired_replay(left, right, left_player, right_player, plan, 0),
        Err(DirectCoordinatorError::UnsafeStatePayload {
            side: Side::Left,
            ordinal: 0
        })
    ));
}

#[test]
fn timed_state_does_not_overwrite_active_external_serial() {
    let mut backend = backend("busy-state.gb");
    let start_tick = backend.game_boy_cpu_cycles().unwrap();
    let EmuBackend::Gb(gb) = &mut backend else {
        unreachable!();
    };
    gb.emu.write_byte(SERIAL_SB, 0x34);
    gb.emu.write_byte(SERIAL_SC, 0x80);
    let state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 1,
    };
    let player = player(
        vec![ReplayEvent::GameBoyLinkStateAtTick {
            frame: 0,
            tick: 0,
            state,
        }],
        start_tick,
        "busy-state",
        None,
    );
    let mut side = DirectSide::new(Side::Left, backend, player, 0).unwrap();

    assert!(matches!(
        side.apply_state_events_before(1),
        Err(DirectCoordinatorError::StateOverwritesTransfer {
            side: Side::Left,
            ordinal: 0,
            ..
        })
    ));
    assert_eq!(side.source_cursor, 0);
    assert_ne!(side.backend.game_boy_link_replay_state(), Some(state));
}

#[test]
fn frame_complete_link_work_precedes_checkpoint_and_commit() {
    let mut probe = backend("probe.gb");
    let start_tick = probe.game_boy_cpu_cycles().unwrap();
    let mut cursor = probe.begin_game_boy_frame_slice().unwrap();
    let completion = probe
        .step_game_boy_frame_slice_until(&mut cursor, None, false)
        .unwrap();
    assert_eq!(
        completion.outcome,
        zeff_gb_core::emulator::FrameSliceOutcome::FrameComplete
    );
    let event_tick = probe.game_boy_cpu_cycles().unwrap() - start_tick;
    let nop_count = usize::try_from((event_tick - 20) / 4).unwrap();
    let mut rom = vec![0u8; 0x8000];
    let action_offset = 0x100 + nop_count;
    rom[action_offset..action_offset + 4].copy_from_slice(&[0x3E, 0x81, 0xE0, 0x02]);

    let boundary_pair = || {
        let mut left = backend_from_rom("boundary-left.gb", rom.clone());
        let mut right = backend("boundary-right.gb");
        left.set_link_peer_present(true);
        right.set_link_peer_present(true);
        let EmuBackend::Gb(left_gb) = &mut left else {
            unreachable!();
        };
        left_gb.emu.write_byte(SERIAL_SB, 0xAB);
        let EmuBackend::Gb(right_gb) = &mut right else {
            unreachable!();
        };
        right_gb.emu.write_byte(SERIAL_SB, 0x34);
        right_gb.emu.write_byte(SERIAL_SC, 0x80);
        (left, right)
    };

    let (mut expected_left, mut expected_right) = boundary_pair();
    let target = expected_left.game_boy_cpu_cycles().unwrap() + event_tick;
    let mut left_lease = PairedGameBoyFrameLease::default();
    let mut right_lease = PairedGameBoyFrameLease::default();
    left_lease.begin(&expected_left, None).unwrap();
    right_lease.begin(&expected_right, None).unwrap();
    let left_progress = left_lease
        .step_direct_until(&mut expected_left, Some(target), true)
        .unwrap();
    assert_eq!(
        left_progress.outcome,
        PairedGameBoyFrameLeaseOutcome::FrameComplete
    );
    assert!(left_progress.boundary_reached);
    assert!(left_progress.queued_master_action.is_some());
    assert_eq!(
        right_lease
            .step_direct_until(&mut expected_right, Some(target), true)
            .unwrap()
            .outcome,
        PairedGameBoyFrameLeaseOutcome::FrameComplete
    );
    let preview = expected_left
        .preview_game_boy_link_peer(&expected_right)
        .unwrap();
    let action = preview.local_action.unwrap();
    let reply = preview.peer_reply;
    let mut prepared = expected_left
        .try_prepare_game_boy_link_peer(&mut expected_right)
        .unwrap();
    expected_left
        .try_apply_prepared_game_boy_link_reply(prepared.local_action.take().unwrap())
        .unwrap();
    let left_checkpoint = sha256_bytes(&expected_left.encode_replay_hash_state_bytes().unwrap());
    let right_checkpoint = sha256_bytes(&expected_right.encode_replay_hash_state_bytes().unwrap());
    let left_events = vec![
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: event_tick,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: TRANSFER_ID,
                clock_period_t_cycles: action.clock_period_t_cycles,
                out_byte: action.out_byte,
                serial_generation: action.serial_generation,
            },
        },
        ReplayEvent::GameBoyLink {
            frame: 0,
            tick: event_tick,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: TRANSFER_ID,
                out_byte: reply.out_byte,
                passive: reply.passive,
                serial_generation: reply.serial_generation,
            },
        },
    ];
    let right_events = vec![ReplayEvent::GameBoyLink {
        frame: 0,
        tick: event_tick,
        event: ReplayGameBoyLinkEvent::RemoteMasterStart {
            transfer_id: TRANSFER_ID,
            clock_period_t_cycles: action.clock_period_t_cycles,
            out_byte: action.out_byte,
            serial_generation: action.serial_generation,
            local_reply: Some(ReplayGameBoyLinkReply {
                out_byte: reply.out_byte,
                passive: reply.passive,
                serial_generation: reply.serial_generation,
            }),
        },
    }];
    let (left, right) = boundary_pair();
    let left_player = player(
        left_events.clone(),
        start_tick,
        "boundary-left",
        Some(left_checkpoint),
    );
    let right_player = player(
        right_events.clone(),
        start_tick,
        "boundary-right",
        Some(right_checkpoint),
    );
    let plan = PairedTransferPlan::build(
        &left_player.metadata().events,
        left_player.metadata().game_boy_link_start_tick,
        &right_player.metadata().events,
        right_player.metadata().game_boy_link_start_tick,
    )
    .unwrap();
    let result = run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap();
    assert_eq!(result.frames, 1);
    assert_eq!(result.left_generated_link_events.len(), 2);
    assert_eq!(result.right_generated_link_events.len(), 1);

    let mut delayed_left_events = left_events;
    let ReplayEvent::GameBoyLink { frame, .. } = &mut delayed_left_events[1] else {
        unreachable!();
    };
    *frame = 1;
    let (left, right) = boundary_pair();
    let left_player = player(
        delayed_left_events,
        start_tick,
        "boundary-delayed-left",
        Some(left_checkpoint),
    );
    let right_player = player(right_events, start_tick, "boundary-delayed-right", None);
    let plan = PairedTransferPlan::build(
        &left_player.metadata().events,
        left_player.metadata().game_boy_link_start_tick,
        &right_player.metadata().events,
        right_player.metadata().game_boy_link_start_tick,
    )
    .unwrap();

    let result = run_direct_paired_replay(left, right, left_player, right_player, plan, 0).unwrap();
    assert_eq!(result.frames, 2);
    assert_eq!(result.left_generated_link_events[1].frame(), 1);
}
