use super::*;
use crate::hardware::bus::scheduled::ScheduledBus;
use crate::hardware::sound::{CandidateBus, SoundFault};
use crate::hardware::vdp::VdpDmaSource;
use zeff_m68k::bus::Direction;
use zeff_z80::Z80Bus;

fn advance(m: &mut Emulator, ticks: u64) {
    ScheduledBus::new(&mut m.bus, &mut m.sound_cpu, &mut m.sound_schedule)
        .advance_machine(ticks)
        .unwrap();
}

fn pending(m: &mut Emulator) {
    for _ in 0..1000 {
        advance(m, 1);
        assert_eq!(m.sound_fault(), None);
        if m.sound_schedule.pending.as_ref().is_some_and(|pending| {
            pending
                .bank_access
                .is_some_and(|access| access.address == DATA && access.direction == Direction::Read)
        }) {
            return;
        }
    }
    panic!("banked ROM read not reached");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel_reader_fixture_sound_keeps_prepared_a_and_refuses_adapter_domains() {
    let program = [0x60fe, 0x4e71];
    for (model, size, base) in CASES {
        let mut source = machine(model, size, &program, &[patch(DATA, 0xa5)], true);
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 1;
        source.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x80, 0x3a, 0, 0x80, 0x76]);
        pending(&mut source);
        let prepared = source.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(prepared.cpu.regs().a, 0xa5);
        assert!(!prepared.has_read_replay());
        assert!(!source.bus.cartridge.mode_sensitive_read(DATA));
        let deadline = source.sound_deadline().unwrap().get();
        for current in [Some(0xa5), Some(0x5a), None] {
            let mut receiver = source.clone();
            receiver.bus.cartridge.rom_overlay = overlay(
                model,
                size,
                &program,
                &current
                    .map(|value| vec![patch(DATA, value)])
                    .unwrap_or_default(),
            );
            assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&prepared));
            let before = receiver.sound_cpu.clone();
            let remaining = deadline - receiver.bus.now.get();
            assert!(remaining > 1);
            advance(&mut receiver, remaining - 1);
            assert_eq!(receiver.sound_cpu, before);
            advance(&mut receiver, 1);
            assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
            pending(&mut receiver);
            let remaining = receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
            advance(&mut receiver, remaining);
            assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x12));
            assert!(!receiver.supports_rom_cheats());
            assert!(receiver.encode_state().is_err());
        }
        let guarded = if model == Model::First {
            0x10000
        } else {
            0x100000
        };
        for target in [base, guarded] {
            for direction in [Direction::Read, Direction::Write] {
                let mut memory = source.bus.sound.clone();
                memory.bank = (target >> 15) as u16;
                let before = memory.clone();
                let local = (0x8000 | (target & 0x7fff)) as u16;
                {
                    let mut bus = CandidateBus::new(&mut memory, &source.bus.cartridge);
                    if direction == Direction::Read {
                        bus.cpu_read(local);
                    } else {
                        bus.cpu_write(local, 0x77);
                    }
                    assert_eq!(
                        bus.fault.get(),
                        Some(SoundFault::UnsupportedAccess {
                            address: local,
                            direction
                        })
                    );
                }
                assert_eq!(memory, before);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel_reader_fixture_dma_keeps_accepted_a_and_refuses_adapter_sources() {
    for (model, size, base) in CASES {
        let mut program = Vec::new();
        for value in [
            0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9640, 0x9700, 0x4400,
        ] {
            program.extend([0x33fc, value, 0xc0, 4]);
        }
        program.extend([0x60fe, 0x4e71]);
        let mut source = machine(
            model,
            size,
            &program,
            &patches("008000:A55A+008002:B66B"),
            true,
        );
        for _ in 0..8 {
            source.step_instruction().unwrap();
        }
        source
            .bus
            .vdp
            .write_control_from(
                0x0080,
                Region::NtscUs,
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
            advance(&mut source, 1);
        }
        let first = source.vdp().fifo().front().expect("sampled DMA word");
        assert_eq!(first.data, 0xa55a);
        for (text, next) in [
            (Some("008000:A55A+008002:B66B"), [0xb6, 0x6b]),
            (Some("008000:C77C+008002:D88D"), [0xd8, 0x8d]),
            (None, [0x56, 0x78]),
        ] {
            let mut receiver = source.clone();
            receiver.bus.cartridge.rom_overlay = overlay(
                model,
                size,
                &program,
                &text.map(patches).unwrap_or_default(),
            );
            assert_eq!(receiver.vdp().fifo().front(), Some(first));
            advance(&mut receiver, 2000);
            assert_eq!(&receiver.vdp().memory().vram()[0x400..0x402], &[0xa5, 0x5a]);
            assert_eq!(&receiver.vdp().memory().vram()[0x402..0x404], &next);
            let counters =
                crate::VdpDmaRegisters::from_registers(receiver.vdp().registers()).unwrap();
            assert_eq!(
                (counters.length(), counters.source(), counters.source_high()),
                (0, 0x4002, 0)
            );
            assert!(receiver.vdp().fifo().is_empty());
            assert!(!receiver.supports_rom_cheats());
        }
        let guarded = if model == Model::First {
            0x10000
        } else {
            0x100000
        };
        for target in [base, guarded] {
            let mut program = Vec::new();
            for value in [
                0x8114,
                0x8f02,
                0x9301,
                0x9400,
                0x9500,
                0x9600 | ((target >> 9) as u16 & 0xff),
                0x9700 | ((target >> 17) as u16 & 0x7f),
                0x4400,
            ] {
                program.extend([0x33fc, value, 0xc0, 4]);
            }
            program.extend([0x60fe, 0x4e71]);
            let mut failed = machine(model, size, &program, &[patch(DATA, 0xa5)], true);
            for _ in 0..8 {
                failed.step_instruction().unwrap();
            }
            let before = failed.bus.vdp.clone();
            let ram = failed.bus.cartridge.parallel_adapter.clone();
            assert_eq!(
                failed.bus.vdp.write_control_from(
                    0x0080,
                    Region::NtscUs,
                    VdpDmaSource::from_cartridge(&failed.bus.cartridge, &failed.bus.ram)
                ),
                Err(crate::VdpPortError::Schedule(crate::VdpScheduleError::Dma(
                    crate::VdpDmaError::UnsupportedSource(target)
                )))
            );
            assert_eq!(failed.bus.vdp, before);
            assert_eq!(failed.bus.cartridge.parallel_adapter, ram);
        }
    }
}
