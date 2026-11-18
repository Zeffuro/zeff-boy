use super::*;
use crate::hardware::vdp::VdpDmaSource;

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
    panic!("banked read not reached");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn six_special_save_owners_keep_prepared_rom_a_and_split_sram_publication_distinct() {
    for profile in ALL {
        let mut source = machine(profile, &[0x60fe, 0x4e71], true);
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 3;
        source.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x80, 0x3a, 0, 0x80, 0x76]);
        overlay(&mut source, profile, &[patch(DATA, 0xa5)]);
        assert!(!source.bus.cartridge.mode_sensitive_read(DATA));
        pending(&mut source, DATA);
        let prepared = source.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(prepared.cpu.regs().a, 0xa5);
        assert!(!prepared.has_read_replay());
        let deadline = source.sound_deadline().unwrap().get();
        let saved = if profile.factory_fixture() {
            Some((
                source.encode_state().unwrap(),
                source.encode_frontend_state().unwrap(),
                source.encode_debugger_checkpoint().unwrap(),
            ))
        } else {
            None
        };
        for current in [Some(0xa5), Some(0x5a), None] {
            for format in 0..if saved.is_some() { 4 } else { 1 } {
                let mut receiver = source.clone();
                overlay(
                    &mut receiver,
                    profile,
                    &current
                        .map(|value| vec![patch(DATA, value)])
                        .unwrap_or_default(),
                );
                if let Some((raw, front, owned)) = &saved
                    && format != 0
                {
                    advance(&mut receiver, deadline - source.bus.now.get());
                    receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
                    match format {
                        1 => receiver.decode_state(raw).unwrap(),
                        2 => receiver.decode_frontend_state(front).unwrap(),
                        _ => receiver.decode_debugger_checkpoint(owned).unwrap(),
                    }
                }
                assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&prepared));
                assert_eq!(receiver.sound_deadline().unwrap().get(), deadline);
                let before = receiver.sound_cpu.clone();
                let remaining = deadline - receiver.bus.now.get();
                assert!(remaining > 1);
                advance(&mut receiver, remaining - 1);
                assert_eq!(receiver.sound_cpu, before);
                advance(&mut receiver, 1);
                assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
                pending(&mut receiver, DATA);
                let remaining = receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
                advance(&mut receiver, remaining);
                assert_eq!(
                    receiver.sound_cpu.regs().a,
                    current.unwrap_or(if profile == Profile::Expansion {
                        0
                    } else {
                        0x12
                    })
                );
            }
        }
    }
    let mut source = machine(Profile::Split, &[0x60fe, 0x4e71], true);
    source.load_save_ram(&[0x77; 32768]).unwrap();
    overlay(&mut source, Profile::Split, &[patch(0x200001, 0xee)]);
    source.bus.sound.set_reset(false);
    source.bus.sound.bank = 0x40;
    source.bus.sound.ram[..7].copy_from_slice(&[0x3a, 1, 0x80, 0x3a, 1, 0x80, 0x76]);
    pending(&mut source, 0x200001);
    let prepared = source.sound_schedule.pending.as_ref().unwrap().clone();
    assert_eq!(prepared.cpu.regs().a, 0x77);
    assert!(prepared.has_read_replay());
    source.load_save_ram(&[0x88; 32768]).unwrap();
    overlay(&mut source, Profile::Split, &[patch(0x200001, 0xa5)]);
    assert_eq!(source.sound_schedule.pending.as_ref(), Some(&prepared));
    let remaining = source.sound_deadline().unwrap().get() - source.bus.now.get();
    advance(&mut source, remaining);
    assert_eq!(source.sound_cpu.regs().a, 0x88);
    let sampled = source.bus.cartridge.read_sampled(0x200001).unwrap();
    assert_eq!(
        (sampled.value, sampled.origin.storage_offset()),
        (0x88, None)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn six_special_save_owners_keep_dma_fifo_a_and_existing_source_refusals() {
    for profile in ALL {
        let program = super::super::sound_tests::dma_fifo_program();
        let mut source = machine(profile, &program, true);
        overlay(&mut source, profile, &patches("018000:A55A+018002:B66B"));
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
        let first = source.vdp().fifo().front().expect("first sampled DMA word");
        assert_eq!(first.data, 0xa55a);
        let saved = profile
            .factory_fixture()
            .then(|| source.encode_frontend_state().unwrap());
        for (text, next) in [
            (Some("018000:A55A+018002:B66B"), [0xb6, 0x6b]),
            (Some("018000:C77C+018002:D88D"), [0xd8, 0x8d]),
            (
                None,
                if profile == Profile::Expansion {
                    [0, 0]
                } else {
                    [0x56, 0x78]
                },
            ),
        ] {
            let mut receiver = source.clone();
            overlay(
                &mut receiver,
                profile,
                &text.map(patches).unwrap_or_default(),
            );
            assert_eq!(receiver.vdp().fifo().front(), Some(first));
            if let Some(saved) = &saved {
                receiver.decode_frontend_state(saved).unwrap();
                assert_eq!(receiver.encode_frontend_state().unwrap(), *saved);
            }
            advance(&mut receiver, 2000);
            assert_eq!(&receiver.vdp().memory().vram()[0x400..0x402], &[0xa5, 0x5a]);
            assert_eq!(&receiver.vdp().memory().vram()[0x402..0x404], &next);
            let counters =
                crate::VdpDmaRegisters::from_registers(receiver.vdp().registers()).unwrap();
            assert_eq!(
                (counters.length(), counters.source(), counters.source_high()),
                (0, 0xc002, 0)
            );
            assert!(receiver.vdp().fifo().is_empty());
        }
    }
    for (profile, target, middle, high) in [
        (Profile::Expansion, 0x400000, 0x9600, 0x9720),
        (Profile::Bytes, 0x200000, 0x9600, 0x9710),
        (Profile::Odd, 0x200000, 0x9600, 0x9710),
        (Profile::Switched, 0x200000, 0x9600, 0x9710),
        (Profile::Reserved, 0x200000, 0x9600, 0x9710),
        (Profile::Split, 0x210000, 0x9680, 0x9710),
    ] {
        let mut program = Vec::new();
        for value in [0x8114, 0x8f02, 0x9301, 0x9400, 0x9500, middle, high, 0x4400] {
            program.extend([0x33fc, value, 0xc0, 4]);
        }
        program.extend([0x60fe, 0x4e71]);
        let mut source = machine(profile, &program, true);
        overlay(&mut source, profile, &[patch(DATA, 0xa5)]);
        for _ in 0..8 {
            source.step_instruction().unwrap();
        }
        let before = source.bus.vdp.clone();
        let battery = source.dump_save_ram();
        assert_eq!(
            source.bus.vdp.write_control_from(
                0x0080,
                Region::NtscUs,
                VdpDmaSource::from_cartridge(&source.bus.cartridge, &source.bus.ram)
            ),
            Err(crate::VdpPortError::Schedule(crate::VdpScheduleError::Dma(
                crate::VdpDmaError::UnsupportedSource(target)
            )))
        );
        assert_eq!(source.bus.vdp, before);
        assert_eq!(source.dump_save_ram(), battery);
    }
    let mut program = Vec::new();
    for value in [
        0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9600, 0x9710, 0x4400, 0x0080,
    ] {
        program.extend([0x33fc, value, 0xc0, 4]);
    }
    program.extend([0x60fe, 0x4e71]);
    let mut source = machine(Profile::Split, &program, true);
    source.load_save_ram(&[0x77; 32768]).unwrap();
    overlay(
        &mut source,
        Profile::Split,
        &patches("200000:A55A+200002:B66B"),
    );
    for _ in 0..256 {
        source.step_instruction().unwrap();
        if source.vdp().memory().vram()[0x400..0x404] == [0xff, 0x77, 0xff, 0x77] {
            break;
        }
    }
    assert_eq!(
        &source.vdp().memory().vram()[0x400..0x404],
        &[0xff, 0x77, 0xff, 0x77]
    );
    assert_eq!(source.sound_fault(), None);
}
