use super::*;

pub(super) fn overlay(machine: &mut Emulator, patches: &[CheatPatch]) {
    if machine.bus.cartridge.lock_on != Some(LockOn::TwoMiB) {
        machine.install_rom_patches(patches).unwrap();
        return;
    }
    assert!(!machine.supports_rom_cheats());
    if !patches.is_empty() {
        let before = machine.clone();
        assert!(machine.install_rom_patches(patches).is_err());
        assert_eq!(*machine, before);
    }
    let ordinary = Cartridge::new(machine.bus.cartridge.bytes.to_vec()).unwrap();
    assert!(ordinary.lock_on.is_none() && ordinary.lock_on_save.is_none());
    machine.bus.cartridge.rom_overlay = ordinary.prepare_rom_overlay(patches).unwrap();
    assert_eq!(machine.rom_offset_for_cpu_address(0x1000), None);
    assert!(machine.check_debug_call(0x1000).is_err());
}

pub(super) fn setup(machine: &mut Emulator, target: u32) {
    write_words(
        &mut machine.bus.ram,
        0x600,
        &[0x4eb9, (target >> 16) as u16, target as u16, 0x4e75, 0x4e71],
    );
}

pub(super) fn begin(machine: &mut Emulator, target: u32) -> crate::emulator::DebugCallContext {
    assert!(!machine.supports_rom_cheats());
    let before = machine.clone();
    assert!(machine.check_debug_call(target).is_err());
    assert_eq!(*machine, before);
    let context = machine.begin_debug_call(0xff0600).unwrap();
    machine.step_instruction().unwrap();
    assert_eq!(machine.registers().pc, target);
    context
}

pub(super) fn return_to_caller(machine: &mut Emulator) {
    assert_eq!(machine.registers().pc, 0xff0606);
    machine.step_instruction().unwrap();
}
