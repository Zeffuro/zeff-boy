use super::*;

#[test]
fn transfer_period_matches_mode_and_fast_bit() {
    let mut serial = Serial::new();

    serial.mode = HardwareMode::DMG;
    serial.sc = 0x00;
    assert_eq!(serial.transfer_period(), 4096);

    serial.mode = HardwareMode::CGBNormal;
    serial.sc = 0x00;
    assert_eq!(serial.transfer_period(), 4096);
    serial.sc = 0x02;
    assert_eq!(serial.transfer_period(), 128);

    serial.mode = HardwareMode::CGBDouble;
    serial.sc = 0x00;
    assert_eq!(serial.transfer_period(), 2048);
    serial.sc = 0x02;
    assert_eq!(serial.transfer_period(), 64);
}

#[test]
fn step_completes_transfer_only_after_selected_period() {
    let mut serial = Serial::new();
    let mut printer = crate::hardware::printer::GameboyPrinter::new();
    serial.mode = HardwareMode::CGBNormal;
    serial.sc = 0x83;

    assert!(!serial.step(127, &mut printer));
    assert!(serial.step(1, &mut printer));

    assert_eq!(serial.sb, 0x00);
    assert_eq!(serial.sc & 0x80, 0);
}

#[test]
fn transfer_completion_uses_free_running_clock_phase() {
    let mut serial = Serial::new();
    let mut printer = crate::hardware::printer::GameboyPrinter::new();
    serial.mode = HardwareMode::DMG;
    serial.set_clock_phase(4044);

    assert!(!serial.step(104, &mut printer));
    serial.sc = 0x81;

    assert!(!serial.step(4043, &mut printer));
    assert!(serial.step(1, &mut printer));
    assert_eq!(serial.sc & 0x80, 0);
}

#[test]
fn disconnected_device_returns_ff() {
    let mut dev = DisconnectedDevice;
    assert_eq!(dev.exchange_byte(0x42), 0xFF);
}

#[test]
fn step_with_disconnected_device() {
    let mut serial = Serial::new();
    let mut dev = DisconnectedDevice;
    serial.mode = HardwareMode::DMG;
    serial.sb = 0xAB;
    serial.sc = 0x81;

    assert!(!serial.step(4095, &mut dev));
    assert!(serial.step(1, &mut dev));
    assert_eq!(serial.sb, 0xFF);
}

#[test]
fn linked_internal_clock_transfer_waits_for_peer_sync() {
    let mut serial = Serial::new();
    let mut dev = DisconnectedDevice;
    serial.mode = HardwareMode::DMG;
    serial.sb = 0xAB;
    serial.sc = 0x81;
    serial.set_link_peer_present(true);

    assert!(!serial.step(4096, &mut dev));
    assert_eq!(serial.pending_link_byte(), Some(0xAB));
    assert_eq!(serial.sb, 0xAB);
    assert_eq!(serial.sc & 0x80, 0x80);

    assert!(serial.complete_link_transfer(0x34));
    assert_eq!(serial.pending_link_byte(), None);
    assert_eq!(serial.sb, 0x34);
    assert_eq!(serial.sc & 0x80, 0);
}

#[test]
fn external_clock_transfer_completes_only_from_link_sync() {
    let mut serial = Serial::new();
    let mut dev = DisconnectedDevice;
    serial.mode = HardwareMode::DMG;
    serial.sb = 0x34;
    serial.sc = 0x80;
    serial.set_link_peer_present(true);

    assert!(!serial.step(4096, &mut dev));
    assert_eq!(serial.pending_link_byte(), None);
    assert!(serial.external_clock_transfer_active());

    assert!(serial.complete_link_transfer(0xAB));
    assert_eq!(serial.sb, 0xAB);
    assert_eq!(serial.sc & 0x80, 0);
}

#[test]
fn link_state_exports_pending_master_and_external_clock_bytes() {
    let mut serial = Serial::new();
    let mut dev = DisconnectedDevice;
    serial.mode = HardwareMode::DMG;
    serial.sb = 0xAB;
    serial.sc = 0x81;
    serial.set_link_peer_present(true);
    serial.step(4096, &mut dev);

    assert_eq!(
        serial.link_state(),
        GameBoyLinkState {
            pending_master_byte: Some(0xAB),
            external_clock_byte: None,
            output_byte: 0xAB,
        }
    );

    let mut external = Serial::new();
    external.sb = 0x34;
    external.sc = 0x80;
    external.set_link_peer_present(true);

    assert_eq!(
        external.link_state(),
        GameBoyLinkState {
            pending_master_byte: None,
            external_clock_byte: Some(0x34),
            output_byte: 0x34,
        }
    );
}

#[test]
fn remote_link_state_resolves_local_pending_master() {
    let mut serial = Serial::new();
    let mut dev = DisconnectedDevice;
    serial.mode = HardwareMode::DMG;
    serial.sb = 0xAB;
    serial.sc = 0x81;
    serial.set_link_peer_present(true);
    serial.step(4096, &mut dev);

    assert!(serial.apply_link_peer_state(GameBoyLinkState {
        pending_master_byte: None,
        external_clock_byte: Some(0x34),
        output_byte: 0x34,
    }));
    assert_eq!(serial.sb, 0x34);
    assert_eq!(serial.sc & 0x80, 0);
}

#[test]
fn remote_idle_state_can_complete_local_pending_master_with_explicit_fallback() {
    let mut serial = Serial::new();
    let mut dev = DisconnectedDevice;
    serial.mode = HardwareMode::DMG;
    serial.sb = 0xAB;
    serial.sc = 0x81;
    serial.set_link_peer_present(true);
    serial.step(4096, &mut dev);

    assert!(serial.apply_remote_link_peer_state(GameBoyLinkState::default(), Some(0xFF)));
    assert_eq!(serial.sb, 0xFF);
    assert_eq!(serial.sc & 0x80, 0);
    assert_eq!(serial.pending_link_byte(), None);
}

#[test]
fn remote_connected_idle_state_uses_peer_output_byte_for_local_pending_master() {
    let mut serial = Serial::new();
    let mut dev = DisconnectedDevice;
    serial.mode = HardwareMode::DMG;
    serial.sb = 0xAB;
    serial.sc = 0x81;
    serial.set_link_peer_present(true);
    serial.step(4096, &mut dev);

    assert!(serial.apply_remote_link_peer_state(
        GameBoyLinkState {
            pending_master_byte: None,
            external_clock_byte: None,
            output_byte: 0x34,
        },
        None,
    ));
    assert_eq!(serial.sb, 0x34);
    assert_eq!(serial.sc & 0x80, 0);
    assert_eq!(serial.pending_link_byte(), None);
}
