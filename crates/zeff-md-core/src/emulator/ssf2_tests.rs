use super::*;
use crate::cartridge::ssf2::tests::{access, cartridge, page_program};
use zeff_m68k::timing::ClockedBus;

fn machine(program: &[u16]) -> Emulator {
    let cart = cartridge(program);
    let mut machine = Emulator::new(
        cart.bytes[..0x80000].to_vec(),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    machine.bus.cartridge = cart;
    machine
}

fn pending_read(machine: &mut Emulator) {
    machine.sound_cpu = zeff_z80::Cpu::new();
    machine.sound_schedule = Default::default();
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x10;
    machine.bus.sound.ram[..4].copy_from_slice(&[0x3a, 0, 0x80, 0x76]);
    for _ in 0..500 {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .advance_machine(1)
        .unwrap();
        assert_eq!(machine.sound_fault(), None);
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .and_then(|pending| pending.bank_access)
            .is_some_and(|access| access.address == 0x80000)
        {
            return;
        }
    }
    panic!("mapped sound read not reached");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_rom_main_queued_words_survive_store_and_refill_uses_new_page() {
    let boot = [0x13fc, 8, 0x00a1, 0x30f3, 0x4ef9, 0x0008, 0];
    let mut machine = machine(&boot);
    let old = [0x13fc_u16, 9, 0x00a1, 0x30f3, 0x7008, 0x4ef9, 0x0008, 8];
    let mut new = old;
    new[4] = 0x7009;
    let bytes = |words: &[u16]| {
        words
            .iter()
            .flat_map(|word| word.to_be_bytes())
            .collect::<Vec<_>>()
    };
    page_program(&mut machine.bus.cartridge, 8, &bytes(&old));
    page_program(&mut machine.bus.cartridge, 9, &bytes(&new));
    for _ in 0..3 {
        machine.step_instruction().unwrap();
    }
    assert_eq!(
        machine.bus.cartridge.rom_source_offset(0x80008),
        Some(0x480008)
    );
    assert_eq!(machine.cpu.queued_words().unwrap()[0], 0x7008);
    machine.step_instruction().unwrap();
    assert_eq!(machine.registers().d[0], 8);
    machine.step_instruction().unwrap();
    machine.step_instruction().unwrap();
    assert_eq!(machine.registers().d[0], 9);
    assert_eq!(
        crate::hardware::audio::trace::main_origin(0x80008, &machine.bus.cartridge).source,
        zeff_emu_common::audio_trace::MdTraceSource::Unknown
    );
    for bank in [8_u16, 9] {
        let mut program = vec![0x13fc, bank, 0x00a1, 0x30f3];
        for word in [
            0x8114, 0x8f02, 0x9304, 0x9400, 0x9500, 0x9600, 0x9704, 0x4400, 0x0080,
        ] {
            program.extend([0x33fc, word, 0x00c0, 0x0004]);
        }
        program.push(0x60fe);
        let mut dma = self::machine(&program);
        assert_eq!(&dma.vdp().memory().vram()[0x400..0x408], &[0; 8]);
        for _ in 0..10 {
            dma.step_instruction().unwrap();
        }
        assert_eq!(dma.sound_fault(), None);
        assert_eq!(dma.bus.cartridge.read(0x80000), Some(bank as u8));
        ScheduledBus::new(&mut dma.bus, &mut dma.sound_cpu, &mut dma.sound_schedule)
            .advance_machine(2000)
            .unwrap();
        assert_eq!(&dma.vdp().memory().vram()[0x400..0x408], &[bank as u8; 8]);
        assert_eq!(dma.vdp().registers().registers()[19], Some(0));
        assert_eq!(dma.vdp().registers().registers()[20], Some(0));
        assert_eq!(dma.vdp().registers().registers()[21], Some(4));
        assert_eq!(dma.vdp().registers().registers()[22], Some(0));
        assert_eq!(dma.vdp().registers().registers()[23], Some(4));
        assert_eq!(dma.vdp().command().address, 0x408);
        assert!(dma.vdp().fifo().is_empty());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_rom_raw26_frontend_future_rejects_bad_slots_and_preserves_restart() {
    let program = [
        0x13fc, 8, 0x00a1, 0x30f3, 0x1039, 0x0008, 0, 0x13fc, 9, 0x00a1, 0x30f3, 0x1239, 0x0008, 0,
        0x60fe,
    ];
    let mut source = machine(&program);
    source.step_instruction().unwrap();
    source.step_instruction().unwrap();
    assert_eq!(source.registers().d[0] & 0xff, 8);
    pending_read(&mut source);
    assert_eq!(source.bus.cartridge.read(0x80000), Some(8));
    assert_eq!(source.sound_cpu.regs().a, 0);
    let pending = source.sound_schedule.pending.as_ref().unwrap();
    let reservation = pending.reservation().unwrap();
    assert_eq!(
        reservation.requester,
        crate::hardware::bus::ownership::Requester::SOUND
    );
    assert!(source.bus.now.get() < pending.deadline);
    let read_deadline = pending.deadline;
    let mut contention = source.clone();
    let clock = contention.bus.cpu_clock;
    let store = ScheduledBus::new(
        &mut contention.bus,
        &mut contention.sound_cpu,
        &mut contention.sound_schedule,
    )
    .write_cycle(access(0xa130f3), 9, clock);
    assert_eq!(store.result, Ok(()));
    assert!(store.wait_clocks > 0);
    assert!(contention.bus.now.get() >= read_deadline);
    assert_eq!(contention.sound_cpu.regs().a, 8);
    assert_eq!(contention.sound_fault(), None);
    assert_eq!(contention.bus.cartridge.read(0x80000), Some(9));
    let raw = source.encode_state().unwrap();
    assert_eq!(&raw[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
    let front = source.encode_frontend_state().unwrap();
    let mut restored = machine(&program);
    restored.decode_frontend_state(&front).unwrap();
    let mut raw_restored = machine(&program);
    raw_restored.decode_state(&raw).unwrap();
    let mut raw_control = source.clone();
    raw_control.decode_state(&raw).unwrap();
    assert_eq!(restored.encode_state().unwrap(), raw);
    assert_eq!((&raw_restored.encode_state().unwrap()), (&raw));
    for step in 0..8 {
        let result = source.step_instruction();
        assert!(result.is_ok());
        assert_eq!(restored.step_instruction(), result);
        assert_eq!(raw_control.step_instruction(), result);
        assert_eq!(raw_restored.step_instruction(), result);
        if step == 0 {
            assert!(source.bus.now.get() >= read_deadline);
            assert_eq!(source.sound_cpu.regs().a, 8);
            assert_eq!(source.bus.cartridge.read(0x80000), Some(9));
        }
        if step == 1 {
            assert_eq!(source.registers().d[1] & 0xff, 9);
        }
        assert_eq!(
            source.encode_state().unwrap(),
            restored.encode_state().unwrap()
        );
        assert_eq!(
            (&source.encode_state().unwrap()),
            (&raw_restored.encode_state().unwrap())
        );
        assert_eq!(source.framebuffer(), restored.framebuffer());
        assert_eq!(source.framebuffer(), raw_restored.framebuffer());
        let samples = source.drain_audio_samples();
        assert_eq!(samples, restored.drain_audio_samples());
        assert_eq!(
            raw_control.drain_audio_samples(),
            raw_restored.drain_audio_samples()
        );
    }
    let before = restored.encode_frontend_state().unwrap();
    let mut bad_slot = raw.clone();
    let capabilities = state::tests::layout::capabilities_offset(&raw);
    assert_eq!(&raw[capabilities..capabilities + 3], &[0, 0, 1]);
    let slot_at = capabilities + 9;
    assert!(bad_slot[slot_at] < 10);
    bad_slot[slot_at] = 10;
    let mut legacy = raw.clone();
    legacy[8..12].copy_from_slice(&(crate::save_state::STATE_VERSION + 1).to_le_bytes());
    let mut trailing = raw.clone();
    trailing.push(0);
    for invalid in [bad_slot, legacy, trailing, raw[..raw.len() - 1].to_vec()] {
        assert!(restored.decode_state(&invalid).is_err());
        assert_eq!(restored.encode_frontend_state().unwrap(), before);
    }
    {
        let mut source = machine(&program);
        source.sound_cpu = zeff_z80::Cpu::new();
        source.sound_schedule = Default::default();
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 0x10;
        source.bus.sound.ram[..9]
            .copy_from_slice(&[0xdd, 0x21, 0x30, 0x80, 0xdd, 0x36, 0x3a, 0xff, 0x76]);
        let before = source.bus.cartridge.clone();
        for _ in 0..600 {
            ScheduledBus::new(
                &mut source.bus,
                &mut source.sound_cpu,
                &mut source.sound_schedule,
            )
            .advance_machine(1)
            .unwrap();
            assert_eq!(source.sound_fault(), None);
            if source
                .sound_schedule
                .pending
                .as_ref()
                .is_some_and(|pending| pending.any_bank_access && pending.bank_access.is_none())
            {
                break;
            }
        }
        assert!(
            source
                .sound_schedule
                .pending
                .as_ref()
                .is_some_and(|pending| pending.any_bank_access && pending.bank_access.is_none())
        );
        let raw = source.encode_state().unwrap();
        assert_eq!(&raw[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
        let mut clone = machine(&program);
        clone.decode_state(&raw).unwrap();
        assert_eq!((&clone.encode_state().unwrap()), (&raw));
        for _ in 0..64 {
            for machine in [&mut source, &mut clone] {
                ScheduledBus::new(
                    &mut machine.bus,
                    &mut machine.sound_cpu,
                    &mut machine.sound_schedule,
                )
                .advance_machine(1)
                .unwrap();
                assert_eq!(machine.sound_fault(), None);
            }
            assert_eq!(
                (&source.encode_state().unwrap()),
                (&clone.encode_state().unwrap())
            );
            assert_eq!(source.drain_audio_samples(), clone.drain_audio_samples());
        }
        assert_eq!(source.sound_cpu.regs().ix, 0x8030);
        assert_eq!(source.bus.cartridge, before);
    }
    let mapping = restored.bus.cartridge.ssf2.clone();
    restored.reset_cpu().unwrap();
    assert_eq!(restored.bus.cartridge.ssf2, mapping);
    assert!(restored.check_warm_reset().is_err());
    pending_read(&mut restored);
    restored.bus.cartridge.initial.pc = 0x400000;
    let before = restored.encode_frontend_state().unwrap();
    assert!(restored.reset_cpu().is_err());
    assert_eq!(restored.encode_frontend_state().unwrap(), before);
    assert_eq!(restored.bus.cartridge.ssf2, mapping);
    restored.bus.cartridge.initial.pc = 0x200;
    restored.reset();
    assert_eq!(restored.bus.cartridge.read(0x80000), Some(1));
    assert_eq!(restored.bus.cartridge.rom_source_offset(0x200), Some(0x200));
}
