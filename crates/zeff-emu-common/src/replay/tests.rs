use super::{
    ReplayEvent, ReplayFirmwareManifest, ReplayGameBoyLinkAction, ReplayGameBoyLinkEvent,
    ReplayGameBoyLinkReply, ReplayGameBoyLinkState, ReplayJoypadFrame, ReplayMetadata,
    ReplayPlayer, ReplayRecorder, ReplayWonderSwanLinkEvent, ReplayZapperFrame,
};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_path(prefix: &str) -> std::path::PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join("zeff_replay_test");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{prefix}_{pid}_{n}.zrpl"))
}

fn roundtrip(state: Vec<u8>, frames: &[(u8, u8)]) {
    let path = unique_path("roundtrip");

    let mut recorder = ReplayRecorder::new(path.clone(), state.clone());
    for &(buttons, dpad) in frames {
        recorder.record_frame(buttons, dpad);
    }
    let written_path = recorder.finish().expect("finish() should succeed");
    assert_eq!(written_path, path);

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert_eq!(player.save_state(), &state[..]);
    assert_eq!(player.total_frames(), frames.len());
    assert_eq!(player.remaining(), frames.len());
    assert!(!player.is_finished() || frames.is_empty());

    for (i, &expected) in frames.iter().enumerate() {
        let actual = player
            .next_frame()
            .unwrap_or_else(|| panic!("expected frame {i} but player was exhausted"));
        assert_eq!(actual, expected, "frame {i} mismatch");
    }

    assert!(player.is_finished());
    assert_eq!(player.remaining(), 0);
    assert_eq!(player.next_frame(), None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_roundtrip_empty() {
    roundtrip(vec![1, 2, 3], &[]);
}

#[test]
fn replay_roundtrip_with_metadata() {
    let path = unique_path("metadata");
    let metadata = ReplayMetadata {
        system: Some("nes".to_string()),
        core_family: Some("Nes".to_string()),
        rom_sha256: Some([0x11; 32]),
        firmware: vec![ReplayFirmwareManifest::External {
            firmware_id: "nintendo.fds.bios".to_string(),
            variant: Some("disksys.rom".to_string()),
            sha256: [0x22; 32],
        }],
        events: Vec::new(),
        cheat_sha256: Some([0x44; 32]),
        final_state_sha256: Some([0x33; 32]),
        game_boy_link_start_state: Some(ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: Some(0x12),
            pending_master_response: Some(0x34),
            pending_master_completion_ready: false,
            queued_master_action: Some(ReplayGameBoyLinkAction {
                out_byte: 0x12,
                clock_period_t_cycles: 4096,
                serial_generation: 7,
            }),
            serial_generation: 7,
        }),
        game_boy_link_start_tick: Some(123_456),
    };

    let mut recorder =
        ReplayRecorder::new_with_metadata(path.clone(), vec![1, 2, 3], metadata.clone());
    recorder.record_frame(0x0F, 0x03);
    recorder.finish().expect("finish() should succeed");

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert_eq!(player.save_state(), &[1, 2, 3]);
    assert_eq!(player.metadata(), &metadata);
    assert_eq!(player.next_frame(), Some((0x0F, 0x03)));
    assert_eq!(player.next_frame(), None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_roundtrip_with_fds_side_events() {
    let path = unique_path("events");
    let mut recorder =
        ReplayRecorder::new_with_metadata(path.clone(), vec![], ReplayMetadata::default());
    recorder.record_frame(0x01, 0x02);
    recorder.record_event(ReplayEvent::FdsDiskSide { frame: 1, side: 1 });
    recorder.record_frame(0x03, 0x04);
    recorder.record_event(ReplayEvent::FdsDiskSide { frame: 0, side: 0 });
    recorder.finish().expect("finish() should succeed");

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert_eq!(
        player.take_events_at_cursor(),
        vec![ReplayEvent::FdsDiskSide { frame: 0, side: 0 }]
    );
    assert_eq!(player.frames_until_next_event(10), 1);
    assert_eq!(player.next_frame(), Some((0x01, 0x02)));
    assert_eq!(
        player.take_events_at_cursor(),
        vec![ReplayEvent::FdsDiskSide { frame: 1, side: 1 }]
    );
    assert_eq!(player.frames_until_next_event(10), 10);
    assert_eq!(player.next_frame(), Some((0x03, 0x04)));
    assert!(player.take_events_at_cursor().is_empty());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_roundtrip_with_link_events_preserves_semantic_order() {
    let path = unique_path("link_events");
    let events = vec![
        ReplayEvent::WonderSwanLink {
            frame: 3,
            session_cycle: 90,
            event: ReplayWonderSwanLinkEvent::RemoteByte {
                generation: 4,
                baud_bps: 9600,
                byte: 0x33,
            },
        },
        ReplayEvent::GameBoyLink {
            frame: 1,
            tick: 105,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: 0x0200_0000_0000_0009,
                out_byte: 0x34,
                passive: true,
                serial_generation: 77,
            },
        },
        ReplayEvent::GameBoyLink {
            frame: 1,
            tick: 105,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: 0x0200_0000_0000_0009,
                clock_period_t_cycles: 4096,
                out_byte: 0x12,
                serial_generation: 76,
            },
        },
        ReplayEvent::GameBoyLink {
            frame: 1,
            tick: 100,
            event: ReplayGameBoyLinkEvent::RemoteMasterStart {
                transfer_id: 0x0100_0000_0000_0007,
                clock_period_t_cycles: 4096,
                out_byte: 0xAB,
                serial_generation: 42,
                local_reply: Some(ReplayGameBoyLinkReply {
                    out_byte: 0xCD,
                    passive: true,
                    serial_generation: 43,
                }),
            },
        },
    ];

    let mut recorder =
        ReplayRecorder::new_with_metadata(path.clone(), vec![], ReplayMetadata::default());
    for event in events {
        recorder.record_event(event);
    }
    recorder.record_frame(0, 0);
    recorder.finish().expect("finish() should succeed");

    let player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert_eq!(
        player.game_boy_link_events().collect::<Vec<_>>(),
        vec![
            (
                1,
                100,
                ReplayGameBoyLinkEvent::RemoteMasterStart {
                    transfer_id: 0x0100_0000_0000_0007,
                    clock_period_t_cycles: 4096,
                    out_byte: 0xAB,
                    serial_generation: 42,
                    local_reply: Some(ReplayGameBoyLinkReply {
                        out_byte: 0xCD,
                        passive: true,
                        serial_generation: 43,
                    }),
                },
            ),
            (
                1,
                105,
                ReplayGameBoyLinkEvent::LocalMasterStart {
                    transfer_id: 0x0200_0000_0000_0009,
                    clock_period_t_cycles: 4096,
                    out_byte: 0x12,
                    serial_generation: 76,
                },
            ),
            (
                1,
                105,
                ReplayGameBoyLinkEvent::RemoteReply {
                    transfer_id: 0x0200_0000_0000_0009,
                    out_byte: 0x34,
                    passive: true,
                    serial_generation: 77,
                },
            ),
        ]
    );
    assert_eq!(
        player.wonder_swan_link_events().collect::<Vec<_>>(),
        vec![(
            3,
            90,
            ReplayWonderSwanLinkEvent::RemoteByte {
                generation: 4,
                baud_bps: 9600,
                byte: 0x33,
            },
        )]
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_finish_pads_input_stream_to_metadata_event_frames() {
    let path = unique_path("event_padding");
    let metadata = ReplayMetadata {
        events: vec![ReplayEvent::GameBoyLink {
            frame: 4,
            tick: 123,
            event: ReplayGameBoyLinkEvent::RemoteReply {
                transfer_id: 1,
                out_byte: 0x34,
                passive: true,
                serial_generation: 2,
            },
        }],
        ..ReplayMetadata::default()
    };
    let mut recorder = ReplayRecorder::new_with_metadata(path.clone(), vec![1, 2, 3], metadata);
    recorder.record_joypad_frame(ReplayJoypadFrame::p1(0x0F, 0x03));
    recorder.finish().expect("finish() should succeed");

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");

    assert_eq!(player.total_frames(), 5);
    for _ in 0..5 {
        assert_eq!(player.next_frame(), Some((0x0F, 0x03)));
    }
    assert_eq!(player.next_frame(), None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_link_events_do_not_throttle_frame_boundary_batches() {
    let path = unique_path("link_events_do_not_throttle");
    let mut recorder =
        ReplayRecorder::new_with_metadata(path.clone(), vec![], ReplayMetadata::default());
    recorder.record_event(ReplayEvent::GameBoyLink {
        frame: 1,
        tick: 100,
        event: ReplayGameBoyLinkEvent::RemoteReply {
            transfer_id: 1,
            out_byte: 0x55,
            passive: false,
            serial_generation: 2,
        },
    });
    recorder.record_event(ReplayEvent::FdsDiskSide { frame: 2, side: 1 });
    recorder.record_frame(0, 0);
    recorder.record_frame(0, 0);
    recorder.finish().expect("finish() should succeed");

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert_eq!(player.frames_until_next_event(10), 2);
    assert!(player.take_events_at_cursor().is_empty());
    player.advance_frames(1);
    assert!(player.take_events_at_cursor().is_empty());
    player.advance_frames(1);
    assert_eq!(
        player.take_events_at_cursor(),
        vec![ReplayEvent::FdsDiskSide { frame: 2, side: 1 }]
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_roundtrip_with_p2_input() {
    let path = unique_path("p2_input");
    let frames = [
        ReplayJoypadFrame {
            buttons: 0x01,
            dpad: 0x02,
            buttons_p2: 0x04,
            dpad_p2: 0x08,
            zapper: ReplayZapperFrame::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
        },
        ReplayJoypadFrame {
            buttons: 0x10,
            dpad: 0x20,
            buttons_p2: 0x40,
            dpad_p2: 0x80,
            zapper: ReplayZapperFrame::default(),
            host_tilt: (0.25, -0.5),
            camera_frame: Some(vec![1, 2, 3, 4]),
        },
    ];

    let mut recorder =
        ReplayRecorder::new_with_metadata(path.clone(), vec![0xAA], ReplayMetadata::default());
    for frame in &frames {
        recorder.record_joypad_frame(frame.clone());
    }
    recorder.finish().expect("finish() should succeed");

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert_eq!(player.save_state(), &[0xAA]);
    assert_eq!(player.total_frames(), frames.len());
    assert_eq!(player.peek_joypad_frames(0, 10), frames);
    assert_eq!(player.next_joypad_frame(), Some(frames[0].clone()));
    assert_eq!(player.next_joypad_frame(), Some(frames[1].clone()));
    assert_eq!(player.next_joypad_frame(), None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_roundtrip_with_zapper_input() {
    let path = unique_path("zapper_input");
    let frame = ReplayJoypadFrame {
        buttons: 0x01,
        dpad: 0x02,
        buttons_p2: 0x04,
        dpad_p2: 0x08,
        zapper: ReplayZapperFrame {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some((128, 96)),
        },
        host_tilt: (0.5, -0.25),
        camera_frame: Some(vec![0x10, 0x20, 0x30]),
    };

    let mut recorder =
        ReplayRecorder::new_with_metadata(path.clone(), vec![0xBB], ReplayMetadata::default());
    recorder.record_joypad_frame(frame.clone());
    recorder.finish().expect("finish() should succeed");

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert!(player.uses_zapper_input());
    assert_eq!(player.next_joypad_frame(), Some(frame));
    assert_eq!(player.next_joypad_frame(), None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_player_reports_host_device_usage() {
    let path = unique_path("host_device_usage");
    let mut recorder =
        ReplayRecorder::new_with_metadata(path.clone(), vec![0xBB], ReplayMetadata::default());
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 0,
        dpad: 0,
        buttons_p2: 0,
        dpad_p2: 0,
        zapper: ReplayZapperFrame::default(),
        host_tilt: (0.0, 0.0),
        camera_frame: None,
    });
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 0,
        dpad: 0,
        buttons_p2: 0,
        dpad_p2: 0,
        zapper: ReplayZapperFrame::default(),
        host_tilt: (0.25, 0.0),
        camera_frame: None,
    });
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 0,
        dpad: 0,
        buttons_p2: 0,
        dpad_p2: 0,
        zapper: ReplayZapperFrame::default(),
        host_tilt: (0.0, 0.0),
        camera_frame: Some(vec![0x10, 0x20]),
    });
    recorder.finish().expect("finish() should succeed");

    let player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert!(player.uses_host_tilt_input());
    assert!(player.uses_host_camera_input());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_roundtrip_with_repeated_camera_input() {
    let path = unique_path("camera_input");
    let camera_frame = vec![0x12, 0x34, 0x56, 0x78];
    let frames = [
        ReplayJoypadFrame {
            buttons: 0x01,
            dpad: 0x02,
            buttons_p2: 0x03,
            dpad_p2: 0x04,
            zapper: ReplayZapperFrame::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: Some(camera_frame.clone()),
        },
        ReplayJoypadFrame {
            buttons: 0x05,
            dpad: 0x06,
            buttons_p2: 0x07,
            dpad_p2: 0x08,
            zapper: ReplayZapperFrame::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: Some(camera_frame),
        },
        ReplayJoypadFrame {
            buttons: 0x09,
            dpad: 0x0A,
            buttons_p2: 0x0B,
            dpad_p2: 0x0C,
            zapper: ReplayZapperFrame::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
        },
    ];

    let mut recorder =
        ReplayRecorder::new_with_metadata(path.clone(), vec![0xCC], ReplayMetadata::default());
    for frame in &frames {
        recorder.record_joypad_frame(frame.clone());
    }
    recorder.finish().expect("finish() should succeed");

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert_eq!(player.total_frames(), frames.len());
    for expected in frames {
        assert_eq!(player.next_joypad_frame(), Some(expected));
    }
    assert_eq!(player.next_joypad_frame(), None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_load_rejects_truncated_input() {
    let path = unique_path("truncated_input");

    let mut data = Vec::new();
    data.extend_from_slice(b"ZRPL");
    data.extend_from_slice(&1u32.to_le_bytes());
    let metadata = ReplayMetadata::default().encode();
    data.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    data.extend_from_slice(&metadata);
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x00]);
    std::fs::write(&path, &data).unwrap();

    let err = match ReplayPlayer::load(&path) {
        Err(e) => e,
        Ok(_) => panic!("should reject truncated input"),
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("truncated replay input")
            || msg.contains("invalid replay frame reserved byte"),
        "got: {msg}"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_roundtrip_single_frame() {
    roundtrip(vec![0xAA, 0xBB], &[(0x0F, 0x03)]);
}

#[test]
fn replay_roundtrip_many_frames() {
    let state = vec![0u8; 256];
    let frames: Vec<(u8, u8)> = (0..100).map(|i| (i as u8, (i * 3) as u8)).collect();
    roundtrip(state, &frames);
}

#[test]
fn replay_roundtrip_large_save_state() {
    let state = vec![0xCD; 65536];
    let frames = vec![(0x01, 0x02), (0xFF, 0xFE)];
    roundtrip(state, &frames);
}

#[test]
fn replay_roundtrip_empty_save_state() {
    roundtrip(vec![], &[(0x10, 0x20)]);
}

#[test]
fn replay_load_rejects_bad_magic() {
    let path = unique_path("bad_magic");
    std::fs::write(&path, b"BAAD\x01\x00\x00\x00\x00\x00\x00\x00").unwrap();
    let err = match ReplayPlayer::load(&path) {
        Err(e) => e,
        Ok(_) => panic!("should reject bad magic"),
    };
    let msg = format!("{err}");
    assert!(msg.contains("not a valid replay file"), "got: {msg}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_load_rejects_bad_version() {
    let path = unique_path("bad_version");

    let mut data = Vec::new();
    data.extend_from_slice(b"ZRPL");
    data.extend_from_slice(&99u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    std::fs::write(&path, &data).unwrap();

    let err = match ReplayPlayer::load(&path) {
        Err(e) => e,
        Ok(_) => panic!("should reject bad version"),
    };
    let msg = format!("{err}");
    assert!(msg.contains("unsupported replay version"), "got: {msg}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_load_handles_odd_trailing_byte() {
    let path = unique_path("odd_trailing");

    let mut data = Vec::new();
    data.extend_from_slice(b"ZRPL");
    data.extend_from_slice(&1u32.to_le_bytes());
    let metadata = ReplayMetadata::default().encode();
    data.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    data.extend_from_slice(&metadata);
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&[
        0xAA, 0xBB, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xCC,
    ]);
    data.extend_from_slice(&0u32.to_le_bytes());

    std::fs::write(&path, &data).unwrap();
    let err = match ReplayPlayer::load(&path) {
        Err(e) => e,
        Ok(_) => panic!("should reject trailing input byte"),
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("trailing bytes") || msg.contains("invalid replay frame reserved byte"),
        "got: {msg}"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_load_empty_metadata() {
    let path = unique_path("metadata_empty");

    let mut data = Vec::new();
    data.extend_from_slice(b"ZRPL");
    data.extend_from_slice(&1u32.to_le_bytes());
    let metadata = ReplayMetadata::default().encode();
    data.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    data.extend_from_slice(&metadata);
    data.extend_from_slice(&1u32.to_le_bytes());
    data.push(0xAB);
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&[
        0x01, 0x02, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    data.extend_from_slice(&0u32.to_le_bytes());
    std::fs::write(&path, &data).unwrap();

    let player = ReplayPlayer::load(&path).expect("replay should load");
    assert!(player.metadata().is_empty());
    assert_eq!(player.save_state(), &[0xAB]);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_loads_legacy_v1_without_metadata() {
    let path = unique_path("legacy_v1");

    let mut save_state = Vec::new();
    save_state.extend_from_slice(b"ZBSTATE\0");
    save_state.extend_from_slice(&3u32.to_le_bytes());
    save_state.extend_from_slice(&[0xAA, 0xBB, 0xCC]);

    let mut data = Vec::new();
    data.extend_from_slice(b"ZRPL");
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&(save_state.len() as u32).to_le_bytes());
    data.extend_from_slice(&save_state);
    data.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
    std::fs::write(&path, &data).unwrap();

    let mut player = ReplayPlayer::load(&path).expect("legacy replay should load");
    assert!(player.metadata().is_empty());
    assert_eq!(player.save_state(), &save_state);
    assert_eq!(player.total_frames(), 2);
    assert_eq!(player.next_frame(), Some((0x01, 0x02)));
    assert_eq!(player.next_frame(), Some((0x03, 0x04)));
    assert_eq!(player.next_frame(), None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_peek_frames_does_not_consume_cursor() {
    let path = unique_path("peek_frames");
    let frames = [(0x01, 0x02), (0x03, 0x04), (0x05, 0x06)];
    let mut recorder = ReplayRecorder::new(path.clone(), vec![]);
    for (buttons, dpad) in frames {
        recorder.record_frame(buttons, dpad);
    }
    recorder.finish().expect("finish() should succeed");

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");
    assert_eq!(player.peek_frames(0, 2), vec![(0x01, 0x02), (0x03, 0x04)]);
    assert_eq!(player.remaining(), 3);
    assert_eq!(player.next_frame(), Some((0x01, 0x02)));
    assert_eq!(player.peek_frames(1, 10), vec![(0x05, 0x06)]);
    assert_eq!(player.remaining(), 2);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_advance_frames_clamps_to_end() {
    let path = unique_path("advance_frames");
    let mut recorder = ReplayRecorder::new(path.clone(), vec![]);
    recorder.record_frame(0x01, 0x02);
    recorder.record_frame(0x03, 0x04);
    recorder.finish().expect("finish() should succeed");

    let mut player = ReplayPlayer::load(&path).expect("load() should succeed");
    player.advance_frames(1);
    assert_eq!(player.next_frame(), Some((0x03, 0x04)));
    player.advance_frames(100);
    assert!(player.is_finished());
    assert_eq!(player.next_frame(), None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_frame_count_tracks_recording() {
    let recorder = ReplayRecorder::new(std::path::PathBuf::from("/dev/null"), vec![]);
    assert_eq!(recorder.frame_count(), 0);
    let mut recorder = recorder;
    recorder.record_frame(0, 0);
    assert_eq!(recorder.frame_count(), 1);
    recorder.record_frame(1, 1);
    recorder.record_frame(2, 2);
    assert_eq!(recorder.frame_count(), 3);
}
