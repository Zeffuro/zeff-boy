use std::path::Path;

use crate::test_support::test_directory;
use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkEvent, ReplayGameBoyLinkState, ReplayJoypadFrame, ReplayMetadata,
    ReplayPlayer, ReplayRecorder,
};

use super::super::timeline::{PairedGameBoyReplayTimeline, paired_game_boy_replay_timeline};

fn replay_player_with_gb_events(
    dir: &Path,
    name: &str,
    frames: usize,
    events: Vec<ReplayEvent>,
) -> anyhow::Result<ReplayPlayer> {
    let path = dir.join(name);
    let metadata = ReplayMetadata {
        events,
        ..ReplayMetadata::default()
    };
    let mut recorder = ReplayRecorder::new_with_metadata(path.clone(), Vec::new(), metadata);
    for _ in 0..frames {
        recorder.record_joypad_frame(ReplayJoypadFrame::default());
    }
    recorder.finish()?;
    ReplayPlayer::load(&path)
}

#[test]
fn paired_game_boy_replay_timeline_aligns_common_transfer_ids() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline")?;
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        8_121,
        vec![ReplayEvent::GameBoyLink {
            frame: 1_333,
            tick: 2_206_540_680,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: 0x0100_0000_0000_0000,
                clock_period_t_cycles: 4096,
                out_byte: 0x01,
                serial_generation: 9,
            },
        }],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10_148,
        vec![ReplayEvent::GameBoyLink {
            frame: 273,
            tick: 2_147_632_556,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: 0x0100_0000_0000_0000,
                clock_period_t_cycles: 4096,
                out_byte: 0x01,
                serial_generation: 9,
                local_reply: None,
            },
        }],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 5);

    assert_eq!(
        timeline,
        PairedGameBoyReplayTimeline {
            left_start_offset: 0,
            right_start_offset: 1_060,
            link_activation_frame: 1_333,
            left_link_activation_frame: 1_333,
            right_link_activation_frame: 1_333,
            left_link_activation_tick: None,
            right_link_activation_tick: None,
            left_target_frames: 8_126,
            right_target_frames: 10_153,
            total_global_frames: 11_213,
        }
    );
    Ok(())
}

#[test]
fn paired_game_boy_replay_timeline_uses_recorded_link_state_frames() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline-link-state")?;
    let state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 0,
    };
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        8_121,
        vec![
            ReplayEvent::GameBoyLinkState { frame: 100, state },
            ReplayEvent::GameBoyLink {
                frame: 1_333,
                tick: 2_206_540_680,
                event: ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x01,
                    serial_generation: 9,
                },
            },
        ],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10_148,
        vec![
            ReplayEvent::GameBoyLinkState { frame: 20, state },
            ReplayEvent::GameBoyLink {
                frame: 273,
                tick: 2_147_632_556,
                event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: 0x0100_0000_0000_0000,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x01,
                    serial_generation: 9,
                    local_reply: None,
                },
            },
        ],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

    assert_eq!(timeline.left_link_activation_frame, 1_060);
    assert_eq!(timeline.right_link_activation_frame, 1_080);
    assert_eq!(timeline.left_link_activation_tick, None);
    assert_eq!(timeline.right_link_activation_tick, None);
    assert_eq!(timeline.link_activation_frame, 1_060);
    Ok(())
}

#[test]
fn paired_game_boy_replay_timeline_uses_recorded_link_state_ticks() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline-link-state-ticks")?;
    let state = ReplayGameBoyLinkState {
        peer_present: true,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 0,
    };
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        10,
        vec![ReplayEvent::GameBoyLinkStateAtTick {
            frame: 4,
            tick: 100,
            state,
        }],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10,
        vec![ReplayEvent::GameBoyLinkStateAtTick {
            frame: 7,
            tick: 200,
            state,
        }],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

    assert_eq!(timeline.left_link_activation_frame, 4);
    assert_eq!(timeline.right_link_activation_frame, 7);
    assert_eq!(timeline.left_link_activation_tick, Some(100));
    assert_eq!(timeline.right_link_activation_tick, Some(200));
    Ok(())
}

#[test]
fn paired_game_boy_replay_timeline_defaults_without_common_transfer_ids() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline-no-common")?;
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        12,
        vec![ReplayEvent::GameBoyLink {
            frame: 4,
            tick: 100,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: 1,
                clock_period_t_cycles: 4096,
                out_byte: 0x01,
                serial_generation: 0,
            },
        }],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10,
        vec![ReplayEvent::GameBoyLink {
            frame: 2,
            tick: 50,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: 2,
                clock_period_t_cycles: 4096,
                out_byte: 0x01,
                serial_generation: 0,
                local_reply: None,
            },
        }],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

    assert_eq!(
        timeline,
        PairedGameBoyReplayTimeline {
            left_start_offset: 0,
            right_start_offset: 0,
            link_activation_frame: 0,
            left_link_activation_frame: 0,
            right_link_activation_frame: 0,
            left_link_activation_tick: None,
            right_link_activation_tick: None,
            left_target_frames: 12,
            right_target_frames: 10,
            total_global_frames: 12,
        }
    );
    Ok(())
}

#[test]
fn paired_game_boy_replay_timeline_ignores_same_role_transfer_ids() -> anyhow::Result<()> {
    let temp = test_directory("pair-timeline-same-role")?;
    let event = ReplayGameBoyLinkEvent::LocalMasterStart {
        transfer_id: 0x0100_0000_0000_0007,
        clock_period_t_cycles: 4096,
        out_byte: 0x42,
        serial_generation: 3,
    };
    let left = replay_player_with_gb_events(
        temp.path(),
        "left.zrpl",
        12,
        vec![ReplayEvent::GameBoyLink {
            frame: 4,
            tick: 100,
            event,
        }],
    )?;
    let right = replay_player_with_gb_events(
        temp.path(),
        "right.zrpl",
        10,
        vec![ReplayEvent::GameBoyLink {
            frame: 2,
            tick: 50,
            event,
        }],
    )?;

    let timeline = paired_game_boy_replay_timeline(&left, &right, 0);

    assert_eq!(timeline.left_start_offset, 0);
    assert_eq!(timeline.right_start_offset, 0);
    assert_eq!(timeline.link_activation_frame, 0);
    Ok(())
}
