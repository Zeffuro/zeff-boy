use super::unique_path;
use crate::replay::{
    ReplayColecoControllerFrame, ReplayJoypadFrame, ReplayPlayer, ReplayRecorder, VERSION,
};

#[test]
fn replay_v3_roundtrips_bounded_coleco_controller_input() {
    let path = unique_path("coleco-v3");
    let frame = ReplayJoypadFrame {
        coleco: [
            ReplayColecoControllerFrame {
                up: true,
                left_button: true,
                keypad: 11,
                ..ReplayColecoControllerFrame::default()
            },
            ReplayColecoControllerFrame {
                right: true,
                right_button: true,
                keypad: 12,
                ..ReplayColecoControllerFrame::default()
            },
        ],
        ..ReplayJoypadFrame::default()
    };
    let mut recorder = ReplayRecorder::new(path.clone(), vec![0xA5; 16]);
    recorder.record_joypad_frame(frame.clone());
    recorder.finish().unwrap();

    let mut player = ReplayPlayer::load(&path).unwrap();
    assert_eq!(player.version(), VERSION);
    assert_eq!(player.next_joypad_frame(), Some(frame));

    let mut malformed = std::fs::read(&path).unwrap();
    let metadata_len = u32::from_le_bytes(malformed[8..12].try_into().unwrap()) as usize;
    let state_len_offset = 12 + metadata_len;
    let state_len = u32::from_le_bytes(
        malformed[state_len_offset..state_len_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let input_offset = state_len_offset + 4 + state_len + 4;
    malformed[input_offset + 24..input_offset + 26].copy_from_slice(&0x0340u16.to_le_bytes());
    std::fs::write(&path, malformed).unwrap();
    assert!(ReplayPlayer::load(&path).is_err());

    let _ = std::fs::remove_file(path);
}
