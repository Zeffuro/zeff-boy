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
