use std::path::PathBuf;

use super::*;

fn stopped_thread() -> EmuThread {
    let emulator = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .unwrap();
    let mut thread = EmuThread::spawn(
        EmuBackend::from_sega8(emulator, PathBuf::from("channel-test.sms")),
        false,
    );
    thread.shutdown();
    thread
}

#[test]
fn checked_channel_apis_distinguish_worker_disconnect() {
    let thread = stopped_thread();

    assert!(!thread.send_checked(EmuCommand::SetUncapped(false)));
    assert!(matches!(
        thread.poll_response(),
        EmuResponsePoll::Disconnected
    ));
    assert!(thread.recv_checked().is_err());
}
