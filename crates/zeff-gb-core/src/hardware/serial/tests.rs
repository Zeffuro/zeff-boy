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
    serial.mode = HardwareMode::CGBNormal;
    serial.sc = 0x83; // start + internal clock + fast

    assert!(!serial.step(127));
    assert!(serial.step(1));
    assert_eq!(serial.sb, 0xFF);
    assert_eq!(serial.sc & 0x80, 0);
}
