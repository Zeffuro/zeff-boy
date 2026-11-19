use super::*;
use crate::hardware::sound::schedule::SoundSchedule;
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn arm(machine: &mut Emulator, program: &[u8]) {
    machine.sound_cpu = zeff_z80::Cpu::new();
    machine.sound_schedule = SoundSchedule::default();
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0;
    machine.bus.sound.ram[..program.len()].copy_from_slice(program);
}

fn pending(machine: &mut Emulator, target: u32) {
    for _ in 0..1000 {
        advance(machine, 1);
        assert_eq!(machine.sound_fault(), None);
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending.bank_access.is_some_and(|access| {
                    access.address == target && access.direction == Direction::Read
                })
            })
        {
            return;
        }
    }
    panic!("Realtec SOUND read not reached");
}

fn restore_schedule(machine: &mut Emulator, bytes: &[u8]) {
    let mut cpu = zeff_z80::Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut reader = StateReader::new(bytes);
    schedule.read_state(&mut cpu, &mut reader).unwrap();
    assert!(reader.is_exhausted());
    schedule.validate_cartridge(&machine.bus.cartridge).unwrap();
    assert_eq!(
        (&cpu, &schedule),
        (&machine.sound_cpu, &machine.sound_schedule)
    );
    machine.sound_cpu = cpu;
    machine.sound_schedule = schedule;
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn realtec_overlay_pending_sound_data_refreshes_current_set_at_saved_deadline() {
    for committed in [false, true] {
        let mut machine = machine(&[0x60fe, 0x4e71], Region::NtscUs, true);
        if committed {
            select(&mut machine, 2, 2);
        }
        let source = machine.bus.cartridge.rom_source_offset(0x1000).unwrap() as u32;
        let original = machine.bus.cartridge.bytes[source as usize];
        machine.install_rom_patches(&[patch(source, 0xa5)]).unwrap();
        arm(&mut machine, &[0x3a, 0, 0x90, 0x76]);
        pending(&mut machine, 0x1000);
        let prepared = machine.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(prepared.instruction.cycles, 3);
        assert_eq!(
            machine
                .sound_cpu
                .next_physical_cycle(&machine.sound_schedule.state)
                .unwrap()
                .kind(),
            zeff_z80::CycleKind::MemoryRead { address: 0x9000 }
        );
        machine
            .sound_schedule
            .validate_cartridge(&machine.bus.cartridge)
            .unwrap();
        assert_eq!(machine.sound_cpu.regs().a, 0);
        let mut writer = StateWriter::new();
        machine
            .sound_schedule
            .write_state(&machine.sound_cpu, &mut writer)
            .unwrap();
        let saved = writer.into_bytes();
        for set in [Some(0xa5), Some(0x5a), None] {
            let mut receiver = machine.clone();
            receiver.clear_rom_patches();
            if let Some(value) = set {
                receiver
                    .install_rom_patches(&[patch(source, value)])
                    .unwrap();
            }
            assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&prepared));
            let overlay = receiver.bus.cartridge.rom_overlay.clone();
            restore_schedule(&mut receiver, &saved);
            assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
            assert_eq!(
                receiver.sound_schedule.pending.as_ref().unwrap().deadline,
                prepared.deadline
            );
            let cpu = receiver.sound_cpu.clone();
            let remaining = receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
            assert!(remaining > 1);
            advance(&mut receiver, remaining - 1);
            assert_eq!(receiver.sound_cpu, cpu);
            advance(&mut receiver, 1);
            assert_eq!(receiver.sound_fault(), None);
            assert_eq!(receiver.sound_cpu.regs().a, set.unwrap_or(original));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn realtec_overlay_pending_sound_operand_refresh_keeps_consumed_opcode_guard() {
    let mut machine = machine(&[0x60fe, 0x4e71], Region::NtscUs, true);
    machine
        .install_rom_patches(&patches("07F000:3E22+07F002:76"))
        .unwrap();
    arm(&mut machine, &[0xc3, 0, 0x90]);
    pending(&mut machine, 0x1000);
    assert!(machine.sound_schedule.state.next_segment_reads_opcode());
    let mut writer = StateWriter::new();
    machine
        .sound_schedule
        .write_state(&machine.sound_cpu, &mut writer)
        .unwrap();
    let saved = writer.into_bytes();
    let mut changed = machine.clone();
    changed
        .install_rom_patches(&patches("07F000:7622"))
        .unwrap();
    restore_schedule(&mut changed, &saved);
    let cpu = changed.sound_cpu.clone();
    let remaining = changed.sound_deadline().unwrap().get() - changed.bus.now.get();
    advance(&mut changed, remaining);
    assert!(matches!(
        changed.sound_fault(),
        Some(SoundFault::UnstableCartridgeRead { .. })
    ));
    assert_eq!(changed.sound_cpu, cpu);

    let remaining = machine.sound_deadline().unwrap().get() - machine.bus.now.get();
    advance(&mut machine, remaining);
    assert_eq!(machine.sound_fault(), None);
    pending(&mut machine, 0x1001);
    let prepared = machine.sound_schedule.pending.as_ref().unwrap().clone();
    machine
        .install_rom_patches(&patches("07F000:3E33+07F002:76"))
        .unwrap();
    assert_eq!(machine.sound_schedule.pending.as_ref(), Some(&prepared));
    let cpu = machine.sound_cpu.clone();
    let remaining = machine.sound_deadline().unwrap().get() - machine.bus.now.get();
    advance(&mut machine, remaining - 1);
    assert_eq!(machine.sound_cpu, cpu);
    advance(&mut machine, 1);
    assert_eq!(machine.sound_fault(), None);
    assert_eq!(machine.sound_cpu.regs().a, 0x33);
}
