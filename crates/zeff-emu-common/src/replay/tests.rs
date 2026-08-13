use super::{ReplayPlayer, ReplayRecorder};
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
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&[0xAA, 0xBB, 0xCC]);

    std::fs::write(&path, &data).unwrap();
    let mut player = ReplayPlayer::load(&path).expect("should load");
    assert_eq!(player.total_frames(), 1);
    assert_eq!(player.next_frame(), Some((0xAA, 0xBB)));
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
