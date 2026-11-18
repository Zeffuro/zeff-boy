use super::*;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};

fn patch(address: u32, value: u8) -> CheatPatch {
    CheatPatch::WideRomWrite {
        address,
        value: CheatValue::Constant(value),
    }
}

fn fixture(program: &[u8]) -> Cartridge {
    let mut bytes = vec![0; 0x20000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0x10, 0, 0, 0, 2, 0]);
    bytes[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    bytes[0x1000..0x1000 + program.len()].copy_from_slice(program);
    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0x400;
    let machine = crate::Emulator::new_functional_svp(
        bytes.clone(),
        crate::LoadConfig {
            region: crate::Region::NtscUs,
            concurrent_sound: true,
        },
        irom,
        crate::svp::FunctionalProfile::new(0x507, 1, 3, [1; 6]).unwrap(),
    )
    .unwrap();
    let mut cartridge = Cartridge::new(bytes).unwrap();
    cartridge.svp = Some(Box::new(machine.synthetic_svp().unwrap().clone()));
    cartridge
}

fn memory() -> SoundMemory {
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory
}

fn restore(pending: &PendingSound, source: &Cpu, cart: &Cartridge) -> SoundSchedule {
    let schedule = SoundSchedule {
        pending: Some(pending.clone()),
        ..Default::default()
    };
    let mut w = StateWriter::new();
    schedule.write_state(source, &mut w).unwrap();
    let bytes = w.into_bytes();
    let history = schedule.write_svp_history(source).unwrap();
    let mut restored = SoundSchedule::default();
    let mut restored_cpu = source.clone();
    restored
        .read_state(&mut restored_cpu, &mut StateReader::new(&bytes))
        .unwrap();
    restored.read_svp_history(source, &history).unwrap();
    restored.validate_state(source, 0).unwrap();
    restored.validate_cartridge(cart).unwrap();
    restored
        .validate_svp_history(
            source,
            cart,
            pending.svp_history.as_ref().unwrap().preparation.now,
        )
        .unwrap();
    assert_eq!(restored_cpu, *source);
    assert_eq!(restored, schedule);
    restored
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sound_whole_banked_immediate_captures_both_reads_and_receiver_mask_survives() {
    let mut cart = fixture(&[0x3e, 0x11, 0x3e, 0x22]);
    cart.rom_overlay = cart.prepare_rom_overlay(&[patch(0x1001, 0xa5)]).unwrap();
    let cpu = Cpu::new_with_reset(zeff_z80::ResetState::new(0x9000, 0x1ffe));
    let memory = memory();
    let mut pending = PendingSound::prepare(&cpu, &memory, &cart, 300).unwrap();
    let history = pending.svp_history.as_ref().unwrap();
    assert_eq!(history.reads.len(), 2);
    assert_eq!((history.reads[0].ordinal, history.reads[1].ordinal), (1, 2));
    assert_eq!(
        (history.reads[0].offset, history.reads[1].offset),
        (0x1000, 0x1001)
    );
    assert_eq!(pending.cpu.regs().a, 0xa5);
    pending
        .reserve_after(&[Reservation {
            requester: Requester::VDP,
            ticks: 300..600,
        }])
        .unwrap();
    assert!(pending.start >= 600 && pending.deadline > pending.start);
    for current in [Some(0xb6), None] {
        cart.rom_overlay = cart
            .prepare_rom_overlay(
                &current
                    .into_iter()
                    .flat_map(|value| [patch(0x1001, value), patch(0x1003, value)])
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let overlay = cart.rom_overlay.clone();
        let restored = restore(&pending, &cpu, &cart);
        assert_eq!(cart.rom_overlay, overlay);
        let mut cpu = cpu.clone();
        let mut memory = memory.clone();
        let mut audio = Audio::default();
        restored.pending.as_ref().unwrap().publish(
            &mut cpu,
            &mut memory,
            &mut [],
            &mut audio,
            &mut cart,
        );
        assert_eq!(cpu.regs().a, 0xa5);
        let future = PendingSound::prepare(&cpu, &memory, &cart, pending.deadline).unwrap();
        assert_eq!(future.cpu.regs().a, current.unwrap_or(0x22));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sound_local_ram_write_keeps_real_preparation_preimage_and_strict_poststate() {
    let mut cart = fixture(&[0]);
    cart.rom_overlay = cart.prepare_rom_overlay(&[patch(0x1000, 0x77)]).unwrap();
    let mut memory = memory();
    memory.ram[..8].copy_from_slice(&[0x21, 0, 2, 0x3e, 0x5a, 0xc3, 0, 0x90]);
    memory.ram[0x200] = 0x33;
    let mut cpu = Cpu::new();
    for _ in 0..3 {
        cpu.step_bounded(
            &mut CandidateBus::new(&mut memory, &cart),
            &mut PrefixState::default(),
        )
        .unwrap();
    }
    assert_eq!(cpu.regs().pc, 0x9000);
    let pending = PendingSound::prepare(&cpu, &memory, &cart, 600).unwrap();
    assert_eq!(pending.memory.ram[0x200], 0x5a);
    assert_eq!(
        pending.svp_history.as_ref().unwrap().source_memory.ram[0x200],
        0x33
    );
    cart.rom_overlay = cart.prepare_rom_overlay(&[]).unwrap();
    restore(&pending, &cpu, &cart);
    let mut bad = pending.clone();
    bad.memory.ram[0x200] ^= 1;
    assert!(
        bad.validate_history(&cpu, BusState::default(), &cart)
            .is_err()
    );
    let mut bad = pending.clone();
    bad.svp_history.as_mut().unwrap().source_memory.ram[0x100] ^= 1;
    assert!(
        bad.validate_history(&cpu, BusState::default(), &cart)
            .is_err()
    );
    let clone = pending.clone();
    let mut changed = pending.clone();
    changed.svp_history.as_mut().unwrap().source_memory.ram[0x200] = 0;
    assert_eq!(
        clone.svp_history.as_ref().unwrap().source_memory.ram[0x200],
        0x33
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sound_six_read_admitted_path_and_overflow_or_unused_history_reject() {
    let mut cart = fixture(&[0xed, 0x4b, 0, 0xa0]);
    cart.rom_overlay = cart
        .prepare_rom_overlay(&[patch(0x2000, 0xa5), patch(0x2001, 0x5a)])
        .unwrap();
    let cpu = Cpu::new_with_reset(zeff_z80::ResetState::new(0x9000, 0x1ffe));
    let memory = memory();
    let pending = PendingSound::prepare(&cpu, &memory, &cart, 600).unwrap();
    assert_eq!(
        pending.svp_history.as_ref().unwrap().reads.len(),
        MAX_ROM_READS
    );
    assert_eq!(pending.cpu.regs().bc(), 0x5aa5);
    restore(&pending, &cpu, &cart);
    let mut extra = pending.clone();
    let read = extra.svp_history.as_ref().unwrap().reads[0];
    extra.svp_history.as_mut().unwrap().reads.push(read);
    assert!(
        extra
            .validate_history(&cpu, BusState::default(), &cart)
            .is_err()
    );
    let mut candidate_memory = memory.clone();
    let bus = CandidateBus::new(&mut candidate_memory, &cart);
    for _ in 0..MAX_ROM_READS {
        assert_eq!(bus.cpu_read(0xa000), 0xa5);
    }
    bus.cpu_read(0xa000);
    assert_eq!(
        bus.fault.get(),
        Some(SoundFault::UnsupportedAccess {
            address: 0xa000,
            direction: Direction::Read
        })
    );
    for change in 0..14 {
        let mut bad = pending.clone();
        let history = bad.svp_history.as_mut().unwrap();
        match change {
            0 => {
                history.reads.remove(1);
            }
            1 => history.reads.swap(0, 1),
            2 => history.reads[1].ordinal = history.reads[0].ordinal,
            3 => history.reads[0].local ^= 1,
            4 => history.reads[0].offset ^= 1,
            5 => history.reads[0].original ^= 1,
            6 => history.reads[0].effective ^= 1,
            7 => history.irq_asserted = !history.irq_asserted,
            8 => history.preparation.now += 15,
            9 => bad.deadline += 15,
            10 => bad.cpu = cpu.clone(),
            11 => history.reservation_waits += 1,
            12 => bad.start += SOUND_CPU_DIVISOR,
            _ => history.preparation.bounded = true,
        }
        assert!(
            bad.validate_history(&cpu, BusState::default(), &cart)
                .is_err(),
            "change {change}"
        );
    }
}

mod original_replay;
