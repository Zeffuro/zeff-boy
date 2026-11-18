use super::*;
use crate::hardware::bus::scheduled::ScheduledBus;
use zeff_m68k::bus::Direction;

pub(super) fn advance_sound(machine: &mut Emulator, ticks: u64) {
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .advance_machine(ticks)
    .unwrap();
}

pub(super) fn pending_rom_read(machine: &mut Emulator) {
    for _ in 0..1000 {
        advance_sound(machine, 1);
        assert_eq!(machine.sound_fault(), None);
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending.bank_access.is_some_and(|access| {
                    access.address == DATA && access.direction == Direction::Read
                })
            })
        {
            return;
        }
    }
    panic!("banked ROM read not reached");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_banked_sound_keeps_prepared_rom_byte_across_install_and_restore() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = fixture(&[0x60fe, 0x4e71], TimingModel::Standard, region);
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 3;
        source.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x80, 0x3a, 0, 0x80, 0x76]);
        source.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
        pending_rom_read(&mut source);
        let pending = source.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(pending.cpu.regs().a, 0xa5);
        assert_ne!(source.sound_cpu.regs().a, 0xa5);
        let deadline = source.sound_deadline().unwrap().get();
        assert!(source.bus.now.get() < deadline);
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        for current in [Some(0xa5), Some(0x5a), None] {
            for front in [false, true] {
                let mut receiver = source.clone();
                if let Some(value) = current {
                    receiver.install_rom_patches(&[patch(DATA, value)]).unwrap();
                } else {
                    receiver.clear_rom_patches();
                }
                assert_eq!(receiver.sound_schedule.pending.as_ref().unwrap(), &pending);
                if front {
                    receiver.decode_frontend_state(&frontend).unwrap();
                    assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
                } else {
                    receiver.decode_state(&raw).unwrap();
                    assert_eq!(receiver.encode_state().unwrap(), raw);
                }
                assert_eq!(receiver.peek(DATA), Some(current.unwrap_or(0x12)));
                assert_eq!(receiver.sound_schedule.pending.as_ref().unwrap(), &pending);
                let remaining = deadline - receiver.bus.now.get();
                advance_sound(&mut receiver, remaining - 1);
                assert_ne!(receiver.sound_cpu.regs().a, 0xa5);
                advance_sound(&mut receiver, 1);
                assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
                pending_rom_read(&mut receiver);
                let deadline = receiver.sound_deadline().unwrap().get();
                let remaining = deadline - receiver.bus.now.get();
                advance_sound(&mut receiver, remaining);
                assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x12));
            }
        }
    }
}

fn write_byte(program: &mut Vec<u16>, address: u32, value: u8) {
    program.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn precise_sound_fixture(region: Region) -> Emulator {
    let mut program = Vec::new();
    write_byte(&mut program, 0xa11100, 1);
    write_byte(&mut program, 0xa11200, 1);
    let mut sound = vec![0; 32];
    sound.extend([
        0x21, 0, 0, 0x2a, 0, 0x80, 0x22, 0, 7, 0x2a, 0, 0x80, 0x22, 2, 7, 0x76,
    ]);
    for (index, value) in sound.into_iter().enumerate() {
        write_byte(&mut program, 0xa00000 + index as u32, value);
    }
    for bit in 0..9 {
        write_byte(&mut program, 0xa06000, u8::from(bit < 2));
    }
    write_byte(&mut program, 0xa11100, 0);
    program.extend([0x4e72, 0x2700, 0x60fe, 0x4e71]);
    let mut machine = fixture(&program, TimingModel::Precise, region);
    machine
        .install_rom_patches(&patches("018000:A55A"))
        .unwrap();
    for _ in 0..256 {
        if machine.main_cpu_stopped() {
            break;
        }
        machine.step_instruction().unwrap();
    }
    assert!(machine.main_cpu_stopped());
    assert_eq!(machine.sound_bank(), 3);
    assert!(!machine.sound_reset_asserted() && !machine.sound_bus_requested());
    machine
}

fn low_byte_pending(machine: &Emulator) -> bool {
    let state = machine.bus.active_sound_state().unwrap();
    machine
        .sound_cpu()
        .next_physical_cycle(&state)
        .is_ok_and(|request| {
            matches!(
                request.kind(),
                zeff_z80::CycleKind::MemoryRead { address: 0x8001 }
            )
        })
}

fn sound_description(machine: &Emulator) -> String {
    let next = machine
        .bus
        .active_sound_state()
        .map(|state| format!("{:?}", machine.sound_cpu().next_physical_cycle(&state)));
    format!(
        "PC={:04x} HL={:04x} cycles={} stopped={} reset={} requested={} output={:02x?} next={next:?}",
        machine.sound_cpu().regs().pc,
        machine.sound_cpu().regs().hl(),
        machine.sound_cpu().cycles(),
        machine.main_cpu_stopped(),
        machine.sound_reset_asserted(),
        machine.sound_bus_requested(),
        &machine.bus.active_sound_memory().ram[0x700..0x704]
    )
}

fn finish_sound_reads(machine: &mut Emulator) {
    for _ in 0..2048 {
        if machine.bus.active_sound_memory().ram[0x703] != 0 {
            return;
        }
        machine.step_instruction().unwrap();
    }
    panic!(
        "physical SOUND pair reads did not retire: {}",
        sound_description(machine)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_pending_low_byte_survives_install_and_restore() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = precise_sound_fixture(region);
        for _ in 0..2048 {
            if low_byte_pending(&source) {
                break;
            }
            source.step_instruction().unwrap();
        }
        assert!(
            low_byte_pending(&source),
            "{region:?}: {}",
            sound_description(&source)
        );
        assert!(source.sound_instruction_pending());
        assert_eq!(source.sound_cpu().regs().hl(), 0);
        assert_eq!(&source.bus.active_sound_memory().ram[0x700..0x704], &[0; 4]);
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        for (code, expected) in [
            ("018000:A55A", [0xa5, 0x5a, 0xa5, 0x5a]),
            ("018000:C77C", [0xa5, 0x7c, 0xc7, 0x7c]),
            ("", [0xa5, 0x34, 0x12, 0x34]),
        ] {
            for restore in 0..3 {
                let mut receiver = source.clone();
                if code.is_empty() {
                    receiver.clear_rom_patches();
                } else {
                    receiver.install_rom_patches(&patches(code)).unwrap();
                }
                match restore {
                    0 => assert_eq!(receiver.encode_frontend_state().unwrap(), frontend),
                    1 => {
                        receiver.decode_state(&raw).unwrap();
                        assert_eq!(receiver.encode_state().unwrap(), raw);
                    }
                    2 => {
                        receiver.decode_frontend_state(&frontend).unwrap();
                        assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
                    }
                    _ => unreachable!(),
                }
                assert!(low_byte_pending(&receiver));
                assert_eq!(receiver.sound_cpu().regs().hl(), 0);
                assert_eq!(
                    &receiver.bus.active_sound_memory().ram[0x700..0x704],
                    &[0; 4]
                );
                assert_eq!(receiver.peek(DATA), Some(expected[2]));
                assert_eq!(receiver.peek(DATA + 1), Some(expected[3]));
                finish_sound_reads(&mut receiver);
                assert_eq!(
                    &receiver.bus.active_sound_memory().ram[0x700..0x704],
                    &expected,
                    "{region:?}, set={code}, restore={restore}: {}",
                    sound_description(&receiver)
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn actual_guest_dma_reads_upper_and_lower_rom_bytes_in_both_timing_models() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut program = Vec::new();
            for word in [
                0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x96c0, 0x9700, 0x4400, 0x0080,
            ] {
                program.extend([0x33fc, word, 0x00c0, 0x0004]);
            }
            program.extend([0x60fe, 0x4e71]);
            let mut machine = fixture(&program, timing, region);
            machine
                .install_rom_patches(&patches("018000:A55A + 018002:B66B"))
                .unwrap();
            for _ in 0..256 {
                machine.step_instruction().unwrap();
                if machine.vdp().memory().vram()[0x400..0x404] == [0xa5, 0x5a, 0xb6, 0x6b] {
                    break;
                }
            }
            assert_eq!(
                &machine.vdp().memory().vram()[0x400..0x404],
                &[0xa5, 0x5a, 0xb6, 0x6b]
            );
            assert_eq!(machine.sound_fault(), None);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_dma_fifo_keeps_sampled_word_and_future_fetches_use_receiver_set() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let source = fixture(&dma_fifo_program(), TimingModel::Standard, region);
        assert_dma_fifo_retains_sampled_word(source, region);
    }
}

pub(super) fn dma_fifo_program() -> Vec<u16> {
    let mut program = Vec::new();
    for word in [
        0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x96c0, 0x9700, 0x4400,
    ] {
        program.extend([0x33fc, word, 0x00c0, 0x0004]);
    }
    program.extend([0x60fe, 0x4e71]);
    program
}

pub(super) fn assert_dma_fifo_retains_sampled_word(source: Emulator, region: Region) {
    assert_dma_fifo_retains_sampled_word_with_overlay(source, region, |machine, patches| {
        machine.install_rom_patches(patches).unwrap();
    });
}

pub(super) fn assert_dma_fifo_retains_sampled_word_with_overlay(
    mut source: Emulator,
    region: Region,
    overlay: fn(&mut Emulator, &[CheatPatch]),
) {
    use crate::hardware::vdp::VdpDmaSource;
    overlay(&mut source, &patches("018000:A55A + 018002:B66B"));
    for _ in 0..8 {
        source.step_instruction().unwrap();
    }
    source
        .bus
        .vdp
        .write_control_from(
            0x0080,
            region,
            VdpDmaSource::from_cartridge(&source.bus.cartridge, &source.bus.ram),
        )
        .unwrap();
    for _ in 0..2000 {
        if source
            .vdp()
            .fifo()
            .front()
            .is_some_and(|entry| entry.data == 0xa55a)
        {
            break;
        }
        advance_sound(&mut source, 1);
    }
    let first = source.vdp().fifo().front().expect("first sampled DMA word");
    assert_eq!(first.data, 0xa55a);
    assert_ne!(&source.vdp().memory().vram()[0x400..0x402], &[0xa5, 0x5a]);
    let saved = source.encode_frontend_state().unwrap();
    for current in [
        Some(("018000:A55A + 018002:B66B", [0xb6, 0x6b])),
        Some(("018000:C77C + 018002:D88D", [0xd8, 0x8d])),
        None,
    ] {
        let mut receiver = source.clone();
        overlay(
            &mut receiver,
            &current.map(|(text, _)| patches(text)).unwrap_or_default(),
        );
        assert_eq!(receiver.vdp().fifo().front(), Some(first));
        receiver.decode_frontend_state(&saved).unwrap();
        assert_eq!(receiver.encode_frontend_state().unwrap(), saved);
        advance_sound(&mut receiver, 2000);
        assert_eq!(&receiver.vdp().memory().vram()[0x400..0x402], &[0xa5, 0x5a]);
        assert_eq!(
            &receiver.vdp().memory().vram()[0x402..0x404],
            &current.map_or([0x56, 0x78], |(_, bytes)| bytes)
        );
    }
}
