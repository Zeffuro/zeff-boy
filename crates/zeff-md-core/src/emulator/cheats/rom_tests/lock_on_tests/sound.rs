use super::*;
use crate::hardware::sound::schedule::SoundSchedule;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(super) fn arm(machine: &mut Emulator, target: u32, program: &[u8]) {
    machine.sound_cpu = zeff_z80::Cpu::new();
    machine.sound_schedule = SoundSchedule::default();
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = (target >> 15) as u16;
    machine.bus.sound.ram[..program.len()].copy_from_slice(program);
}

pub(super) fn pending(machine: &mut Emulator, target: u32) {
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
    panic!("no actual bank-read segment");
}

fn restore_component(machine: &mut Emulator, bytes: &[u8]) {
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
fn lock_on_sound_stable_samples_and_dynamic_replay_keep_distinct_saved_deadline_policies() {
    for region in REGIONS {
        for (mode, target, source, dynamic) in [
            (LockOn::Empty, 0x1000, 0x1000, false),
            (LockOn::HalfMiB, 0x280000, 0x200000, false),
            (LockOn::MiB, 0x200000, 0x200000, false),
            (LockOn::Patch, 0x200000, 0x200000, false),
            (LockOn::MiB, 0x300000, 0x200000, true),
            (LockOn::Patch, 0x300000, 0x300000, true),
            (LockOn::TwoMiB, 0x1000, 0x1000, false),
            (LockOn::TwoMiB, 0x202000, 0x202000, false),
            (LockOn::TwoMiB, 0x200001, 0x200001, true),
            (LockOn::TwoMiB, 0x300000, 0x300000, true),
        ] {
            let mut machine = machine(mode, region, true, &[0x60fe, 0x4e71]);
            let original = machine.rom_bytes()[source as usize];
            consumer::overlay(&mut machine, &[patch(source, 0xa5)]);
            let local = (0x8000 | (target & 0x7fff)) as u16;
            let [low, high] = local.to_le_bytes();
            arm(
                &mut machine,
                target,
                &[0x3a, low, high, 0x3a, low, high, 0x76],
            );
            pending(&mut machine, target);
            assert_eq!(
                machine
                    .sound_schedule
                    .pending
                    .as_ref()
                    .unwrap()
                    .cpu
                    .regs()
                    .a,
                0xa5
            );
            let deadline = machine.sound_deadline().unwrap();
            let raw = machine.encode_state().unwrap();
            let frontend = machine.encode_frontend_state().unwrap();
            let mut writer = StateWriter::new();
            machine
                .sound_schedule
                .write_state(&machine.sound_cpu, &mut writer)
                .unwrap();
            let component = writer.into_bytes();
            for current in [Some(0xa5), Some(0x5a), None] {
                for front in [false, true] {
                    let mut receiver = machine.clone();
                    advance(&mut receiver, 1);
                    receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
                    consumer::overlay(
                        &mut receiver,
                        &current
                            .map(|value| vec![patch(source, value)])
                            .unwrap_or_default(),
                    );
                    assert_ne!(receiver.encode_state().unwrap(), raw);
                    if front {
                        receiver.decode_frontend_state(&frontend).unwrap();
                    } else {
                        receiver.decode_state(&raw).unwrap();
                    }
                    assert_eq!(receiver.encode_state().unwrap(), raw);
                    assert_eq!(receiver.sound_deadline(), Some(deadline));
                    restore_component(&mut receiver, &component);
                    let cpu = receiver.sound_cpu.clone();
                    let remaining = deadline.get() - receiver.bus.now.get();
                    assert!(remaining > 1);
                    advance(&mut receiver, remaining - 1);
                    assert_eq!(receiver.sound_cpu, cpu);
                    advance(&mut receiver, 1);
                    assert_eq!(receiver.sound_fault(), None);
                    let value = current.unwrap_or(original);
                    assert_eq!(
                        receiver.sound_cpu.regs().a,
                        if dynamic { value } else { 0xa5 }
                    );
                    for _ in 0..1000 {
                        if receiver.sound_cpu.state() == zeff_z80::CpuState::Halted {
                            break;
                        }
                        advance(&mut receiver, 1);
                    }
                    assert_eq!(receiver.sound_cpu.state(), zeff_z80::CpuState::Halted);
                    assert_eq!(receiver.sound_cpu.regs().a, value);
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn lock_on_sound_dynamic_opcode_guard_and_later_operand_refresh_survive_component_restore() {
    for mode in [LockOn::MiB, LockOn::Patch] {
        let source = if mode == LockOn::Patch {
            0x300000
        } else {
            0x200000
        };
        let mut machine = machine(mode, Region::NtscUs, true, &[0x60fe, 0x4e71]);
        let mut set = Vec::new();
        word(&mut set, source, 0x3e22);
        set.push(patch(source + 2, 0x76));
        consumer::overlay(&mut machine, &set);
        arm(&mut machine, 0x300000, &[0xc3, 0, 0x80]);
        pending(&mut machine, 0x300000);
        assert!(machine.sound_schedule.state.next_segment_reads_opcode());
        let mut writer = StateWriter::new();
        machine
            .sound_schedule
            .write_state(&machine.sound_cpu, &mut writer)
            .unwrap();
        let saved = writer.into_bytes();
        let mut changed = machine.clone();
        consumer::overlay(&mut changed, &[patch(source, 0x76)]);
        restore_component(&mut changed, &saved);
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
        pending(&mut machine, 0x300001);
        let mut writer = StateWriter::new();
        machine
            .sound_schedule
            .write_state(&machine.sound_cpu, &mut writer)
            .unwrap();
        let saved = writer.into_bytes();
        let mut set = Vec::new();
        word(&mut set, source, 0x3e33);
        set.push(patch(source + 2, 0x76));
        consumer::overlay(&mut machine, &set);
        restore_component(&mut machine, &saved);
        let cpu = machine.sound_cpu.clone();
        let remaining = machine.sound_deadline().unwrap().get() - machine.bus.now.get();
        assert!(remaining > 1);
        advance(&mut machine, remaining - 1);
        assert_eq!(machine.sound_cpu, cpu);
        advance(&mut machine, 1);
        assert_eq!(machine.sound_fault(), None);
        assert_eq!(machine.sound_cpu.regs().a, 0x33);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn lock_on_queued_psg_opcode_uses_unknown_mutable_aperture_origin_after_control_change() {
    use zeff_emu_common::audio_trace::{MdTraceSource, MdTraceWrite};
    for concurrent_sound in [false, true] {
        let mut bytes = crate::cartridge::registers::tests::normal_rom_fixture(LockOn::MiB);
        bytes[4..8].copy_from_slice(&0x300000_u32.to_be_bytes());
        write_words(&mut bytes, 0x200000, &[0x1281, 0x1080, 0x60fe, 0x4e71]);
        let mut cart = Cartridge::new(bytes).unwrap();
        assert!(cart.lock_on.is_none());
        cart.lock_on = Some(LockOn::MiB);
        let mut machine = Emulator::new_with_cartridge(
            cart,
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound,
            },
        )
        .unwrap();
        machine.reset_and_begin_audio_trace(8).unwrap();
        machine.cpu.registers.a[0] = 0xc00011;
        machine.cpu.registers.a[1] = 0xa130f1;
        machine.cpu.registers.d[0] = 0x9f;
        machine.cpu.registers.d[1] = 1;
        machine.step_instruction().unwrap();
        assert_eq!(machine.bus.cartridge.lock_on, Some(LockOn::Patch));
        assert_eq!(machine.queued_words().unwrap()[0], 0x1080);
        assert_ne!(machine.peek(0x300002), Some(0x10));
        machine.step_instruction().unwrap();
        let capture = machine.finish_audio_trace().unwrap();
        capture.trace.validate_complete().unwrap();
        assert_eq!(capture.trace.events.len(), 1);
        match capture.trace.events[0].write {
            MdTraceWrite::Psg { value, origin } => {
                assert_eq!(
                    (value, origin.boundary_pc, origin.source),
                    (0x9f, 0x300002, MdTraceSource::Unknown)
                );
            }
            other => panic!("unexpected trace event: {other:?}"),
        }
        let cart = &machine.bus.cartridge;
        assert_eq!(
            crate::hardware::audio::trace::sound_origin(0x8002, 0x60, cart).source,
            MdTraceSource::Unknown
        );
        assert_eq!(
            crate::hardware::audio::trace::main_origin(0x1000, cart).source,
            MdTraceSource::EffectiveRom { offset: 0x1000 }
        );
    }
}
