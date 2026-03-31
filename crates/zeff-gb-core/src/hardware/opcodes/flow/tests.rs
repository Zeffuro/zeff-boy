use super::*;

fn make_test_bus(mode: HardwareMode) -> Bus {
    let rom = vec![0u8; 0x8000];
    let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
    Bus::new(rom, &header, mode).expect("test bus should initialize")
}

#[test]
fn stop_switches_to_cgb_double_speed_when_key1_prepare_set() {
    let mut cpu = Cpu::new();
    let mut bus = make_test_bus(HardwareMode::CGBNormal);
    bus.write_byte(0xFF4D, 0x01);

    stop(&mut cpu, &mut bus);

    assert_eq!(bus.hardware_mode, HardwareMode::CGBDouble);
    assert_eq!(bus.key1, 0xFE);
    assert!(matches!(cpu.running, CpuState::Running));
}

#[test]
fn stop_enters_stopped_when_cgb_prepare_not_set() {
    let mut cpu = Cpu::new();
    let mut bus = make_test_bus(HardwareMode::CGBNormal);

    stop(&mut cpu, &mut bus);

    assert_eq!(bus.hardware_mode, HardwareMode::CGBNormal);
    assert!(matches!(cpu.running, CpuState::Stopped));
}

#[test]
fn stop_in_dmg_mode_does_not_switch_speed() {
    let mut cpu = Cpu::new();
    let mut bus = make_test_bus(HardwareMode::DMG);
    bus.write_byte(0xFF4D, 0x01);

    stop(&mut cpu, &mut bus);

    assert_eq!(bus.hardware_mode, HardwareMode::DMG);
    assert!(matches!(cpu.running, CpuState::Stopped));
}

#[test]
fn halt_with_pending_irq_and_ime_pending_enable_triggers_halt_bug() {
    let mut cpu = Cpu::new();
    let mut bus = make_test_bus(HardwareMode::DMG);
    cpu.ime = ImeState::PendingEnable;
    cpu.pc = 0xC000;
    bus.ie = 0x01;
    bus.if_reg = 0x01;
    bus.write_byte(0xC000, 0x00);

    halt(&mut cpu, &mut bus);

    assert!(matches!(cpu.running, CpuState::Running));
    assert!(cpu.halt_bug_active);
    let first = cpu.fetch8_timed(&mut bus);
    assert_eq!(first, 0x00);
    assert_eq!(cpu.pc, 0xC000);
}

#[test]
fn halt_bug_causes_next_byte_to_be_read_twice() {
    let mut cpu = Cpu::new();
    let mut bus = make_test_bus(HardwareMode::DMG);
    cpu.ime = ImeState::Disabled;
    cpu.pc = 0xC000;
    bus.ie = 0x01;
    bus.if_reg = 0x01;
    bus.write_byte(0xC000, 0x3C);
    bus.write_byte(0xC001, 0x00);

    halt(&mut cpu, &mut bus);
    assert!(cpu.halt_bug_active);

    let first = cpu.fetch8_timed(&mut bus);
    assert_eq!(first, 0x3C);
    assert_eq!(cpu.pc, 0xC000);
    assert!(!cpu.halt_bug_active);

    let second = cpu.fetch8_timed(&mut bus);
    assert_eq!(second, 0x3C);
    assert_eq!(cpu.pc, 0xC001);
}
