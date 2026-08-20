use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use zeff_emu_common::replay::{ReplayJoypadFrame, ReplayPlayer, ReplayRecorder};
use zeff_gb_core::hardware::types::constants::{SERIAL_SB, SERIAL_SC};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::*;
use crate::link::{LinkEndpointId, LinkSession, LinkSystemType};

fn test_backend() -> EmuBackend {
    let emulator = zeff_gb_core::emulator::Emulator::from_rom_data(
        &vec![0; 0x8000],
        HardwareModePreference::Auto,
    )
    .expect("GB emulator should initialize");
    EmuBackend::from_gb(emulator, PathBuf::from("test.gb"))
}

fn two_frame_player() -> ReplayPlayer {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "zeff_paired_lease_{}_{}.zrpl",
        std::process::id(),
        suffix
    ));
    let mut recorder = ReplayRecorder::new(path.clone(), Vec::new());
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 0x01,
        ..ReplayJoypadFrame::default()
    });
    recorder.record_joypad_frame(ReplayJoypadFrame {
        buttons: 0x02,
        ..ReplayJoypadFrame::default()
    });
    recorder.finish().expect("replay should finish");
    let player = ReplayPlayer::load(&path).expect("replay should load");
    std::fs::remove_file(path).expect("replay should be removable after loading");
    player
}

#[test]
fn boundary_keeps_the_current_replay_frame_owned_by_its_lease() {
    let mut left = test_backend();
    let mut right = test_backend();
    let (left_transport, right_transport) = LocalLinkTransport::pair();
    let mut left_link = RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::new(
        LinkSession::new(left_transport, LinkSystemType::GameBoy, LinkEndpointId(1)),
    ));
    let mut right_link = crate::link::gb::GameBoyRemoteLink::new(LinkSession::new(
        right_transport,
        LinkSystemType::GameBoy,
        LinkEndpointId(2),
    ));
    let EmuBackend::Gb(left_backend) = &mut left else {
        unreachable!();
    };
    left_backend.emu.write_byte(SERIAL_SB, 0xAB);
    left_backend.emu.write_byte(SERIAL_SC, 0x81);
    let EmuBackend::Gb(right_backend) = &mut right else {
        unreachable!();
    };
    right_backend.emu.write_byte(SERIAL_SB, 0x34);
    right_backend.emu.write_byte(SERIAL_SC, 0x80);

    let mut player = two_frame_player();
    let mut lease = PairedGameBoyFrameLease::default();
    assert_eq!(
        step_live_link_replay_side(
            &mut left,
            Some(&mut left_link),
            &mut player,
            &mut lease,
            true,
            None,
        )
        .unwrap(),
        0
    );
    assert_eq!(player.remaining(), 2);

    right_link.poll_backend(&mut right).unwrap();
    assert_eq!(
        step_live_link_replay_side(
            &mut left,
            Some(&mut left_link),
            &mut player,
            &mut lease,
            true,
            None,
        )
        .unwrap(),
        1
    );
    assert_eq!(player.remaining(), 1);
}
