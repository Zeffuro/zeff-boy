use super::*;
use crate::emulator::{MachineFault, WarmResetRejection};
use zeff_emu_common::time::MasterTicks;
use zeff_m68k::bus::Direction;
use zeff_m68k::timing::TimingFault;

fn write_byte(program: &mut Vec<u16>, address: u32, value: u8) {
    program.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn write_word(program: &mut Vec<u16>, address: u32, value: u16) {
    program.extend([0x33fc, value, (address >> 16) as u16, address as u16]);
}

fn boot_instruction(program: &[u16], region: Region, concurrent: bool) -> Emulator {
    let (rom, mut firmware) = images(&[0x60fe, 0x4e71]);
    write_words(&mut firmware, 0x200, program);
    Emulator::new_with_tmss(
        rom,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
        firmware,
    )
    .unwrap()
}

fn reset_refusals(machine: &mut Emulator) {
    let before = machine.clone();
    let encoded = states(machine);
    assert_eq!(
        machine.reset_cpu(),
        Err(TimingFault::ResetOutputUnsupported)
    );
    assert_eq!(
        machine.check_warm_reset(),
        Err(WarmResetRejection::CartridgeOwner)
    );
    assert_eq!(*machine, before);
    assert_eq!(states(machine), encoded);
}

fn refused_guest_write(machine: &mut Emulator, expected: u32) {
    let Err(MachineFault::Main(TimingFault::Bus {
        source: crate::BusError::UnsupportedAccess { address, direction },
        ..
    })) = machine.step_instruction()
    else {
        panic!("expected TMSS write refusal");
    };
    assert_eq!(address, expected);
    assert_eq!(direction, Direction::Write);
}

pub(super) fn transitions(region: Region, concurrent: bool) {
    for (instruction, locked_vdp) in [
        (&[0x33fc, 0x8144, 0x00c0, 4, 0x60fe, 0x4e71][..], true),
        (&[0x13fc, 1, 0x00a1, 0x1200, 0x60fe, 0x4e71][..], false),
    ] {
        let mut m = boot_instruction(instruction, region, concurrent);
        m.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
        reset_refusals(&mut m);
        let clock = m.bus.cpu_clock.0;
        let firmware = m.bus.tmss.clone();
        let vdp = m.bus.vdp.clone();
        let overlay = m.bus.cartridge.rom_overlay.clone();
        refused_guest_write(&mut m, if locked_vdp { 0xc00004 } else { 0xa11200 });
        assert_eq!(m.bus.tmss, firmware);
        assert_eq!(m.bus.cartridge.rom_overlay, overlay);
        assert!(m.bus.tmss.as_ref().unwrap().boot_mapped());
        assert!(m.sound_reset_asserted());
        if locked_vdp {
            // Three stream fetches precede the refused data transfer.
            let fetch_end = clock + 12;
            assert_eq!(m.bus.cpu_clock.0, fetch_end);
            let mut expected = *vdp.registers();
            assert_eq!(
                expected.advance_to(MasterTicks::new(
                    fetch_end * crate::constants::MAIN_CPU_DIVISOR
                )),
                Ok(None)
            );
            assert_eq!(m.bus.vdp.registers(), &expected);
        }
    }

    let mut program = Vec::new();
    write_byte(&mut program, 0xa11100, 1);
    write_byte(&mut program, 0xa00000, 0x00);
    write_byte(&mut program, 0xa00001, 0x18);
    write_byte(&mut program, 0xa00002, 0xfd);
    write_byte(&mut program, 0xa11200, 1);
    write_byte(&mut program, 0xa11100, 0);
    program.extend([0x4e71; 16]);
    let before_remap = program.len();
    write_byte(&mut program, 0xa14101, 0);
    program.extend([0x60fe, 0x4e71]);
    let mut m = machine(&program, region, concurrent);
    m.install_rom_patches(&patches(
        "000000:00FF + 000002:1000 + 000004:0001 + 000006:0100 + 018000:A55A",
    ))
    .unwrap();
    reset_refusals(&mut m);
    enter_cartridge(&mut m);
    let cycles = m.sound_cpu().cycles();
    for _ in 0..22 {
        m.step_instruction().unwrap();
    }
    assert_eq!(m.registers().pc, CODE + before_remap as u32 * 2);
    assert!(!m.sound_reset_asserted() && !m.sound_bus_requested());
    if concurrent {
        assert!(m.sound_cpu().cycles() > cycles);
    } else {
        assert_eq!(m.sound_cpu().cycles(), cycles);
    }
    reset_refusals(&mut m);
    let firmware = m.bus.tmss.clone();
    let overlay = m.bus.cartridge.rom_overlay.clone();
    refused_guest_write(&mut m, 0xa14100);
    assert_eq!(m.bus.tmss, firmware);
    assert!(!m.bus.tmss.as_ref().unwrap().boot_mapped());
    assert_eq!(m.bus.cartridge.rom_overlay, overlay);
    assert!(!m.sound_reset_asserted());
    m.reset();
    assert!(m.bus.tmss.as_ref().unwrap().boot_mapped());
    assert!(!m.bus.tmss.as_ref().unwrap().unlocked());
    assert!(m.sound_reset_asserted());
    assert_eq!(m.registers().pc, 0x200);
    assert_eq!(m.registers().ssp, 0x00ff_fffc);
    assert_eq!(m.bus.cartridge.effective_initial().pc, CODE);
    assert_eq!(m.bus.cartridge.effective_initial().ssp, 0x00ff_1000);
    assert_eq!(m.bus.cartridge.rom_overlay, overlay);
    assert_eq!(m.queued_words(), Some([0x33fc, 0x5345]));
    assert!(
        m.cpu
            .queued_instruction_inputs()
            .unwrap()
            .iter()
            .all(|input| input
                .origin
                .iter()
                .all(|origin| origin.storage_offset().is_none()))
    );
    enter_cartridge(&mut m);
    assert_eq!(m.peek(DATA), Some(0xa5));
}

fn output_program() -> Vec<u16> {
    let mut program = Vec::new();
    for value in [0x8004, 0x8144, 0x8700, 0xc000, 0] {
        write_word(&mut program, 0xc00004, value);
    }
    write_word(&mut program, 0xc00000, 0x000e);
    for value in [0x80, 0x10, 0x90] {
        write_byte(&mut program, 0xc00011, value);
    }
    program.extend([0x60fe, 0x4e71]);
    program
}

fn same_output(actual: &mut Emulator, expected: &mut Emulator) {
    assert_eq!(states(actual), states(expected));
    assert_eq!(
        actual.framebuffer_dimensions(),
        expected.framebuffer_dimensions()
    );
    assert_eq!(actual.framebuffer(), expected.framebuffer());
    let actual_pcm = actual.drain_audio_samples();
    let expected_pcm = expected.drain_audio_samples();
    assert!(!actual_pcm.is_empty());
    assert!(actual_pcm.iter().any(|sample| *sample != 0.0));
    assert!(
        actual_pcm
            .iter()
            .map(|sample| sample.to_bits())
            .eq(expected_pcm.iter().map(|sample| sample.to_bits()))
    );
    assert_eq!(states(actual), states(expected));
}

pub(super) fn complete_output(region: Region, concurrent: bool) {
    let program = output_program();
    let mut source = machine(&program, region, concurrent);
    source.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
    enter_cartridge(&mut source);
    source.step_field().unwrap();
    assert_eq!(source.psg_snapshot().volumes[0], 0);
    assert!(
        source
            .framebuffer()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] != [0, 0, 0])
    );
    let saved = states(&source);
    for set in [Some(0xa5), Some(0x5a), None] {
        let mut changed = source.clone();
        let mut unchanged = source.clone();
        changed
            .install_rom_patches(
                &set.map(|value| vec![patch(DATA, value)])
                    .unwrap_or_default(),
            )
            .unwrap();
        assert_eq!(states(&changed), states(&unchanged));
        assert_eq!(changed.peek(DATA), Some(set.unwrap_or(0x12)));
        changed.step_field().unwrap();
        unchanged.step_field().unwrap();
        same_output(&mut changed, &mut unchanged);
        for codec in 0..3 {
            let mut actual = source.clone();
            let mut expected = source.clone();
            let current = set
                .map(|value| vec![patch(DATA, value)])
                .unwrap_or_default();
            actual.install_rom_patches(&current).unwrap();
            expected.install_rom_patches(&current).unwrap();
            assert_eq!(states(&actual), saved);
            actual.step_field().unwrap();
            restore(&mut actual, &saved, codec);
            restore(&mut expected, &saved, codec);
            assert_eq!(actual.peek(DATA), Some(set.unwrap_or(0x12)));
            if codec != 0 {
                same_output(&mut actual, &mut expected);
            } else {
                assert_eq!(states(&actual), states(&expected));
                assert_eq!(actual.drain_audio_samples(), expected.drain_audio_samples());
            }
            for _ in 0..2 {
                actual.step_field().unwrap();
                expected.step_field().unwrap();
                same_output(&mut actual, &mut expected);
            }
            actual.reset();
            expected.reset();
            assert_eq!(states(&actual), states(&expected));
            enter_cartridge(&mut actual);
            enter_cartridge(&mut expected);
            actual.step_field().unwrap();
            expected.step_field().unwrap();
            same_output(&mut actual, &mut expected);
            assert_eq!(actual.peek(DATA), Some(set.unwrap_or(0x12)));
        }
    }
}

pub(super) fn trace_changes(region: Region, concurrent: bool) {
    let mut m = machine(&output_program(), region, concurrent);
    m.reset_and_begin_audio_trace(32).unwrap();
    enter_cartridge(&mut m);
    for _ in 0..9 {
        m.step_instruction().unwrap();
    }
    assert!(m.bus.audio.trace.is_enabled());
    reset_refusals(&mut m);
    let before = m.clone();
    let active = states(&m);
    for codec in 0..3 {
        let truncated = &active[codec][..active[codec].len() - 1];
        let result = match codec {
            0 => m.decode_state(truncated),
            1 => m.decode_frontend_state(truncated),
            _ => m.decode_debugger_checkpoint(truncated),
        };
        assert!(result.is_err());
        assert_eq!(m, before);
        assert_eq!(states(&m), active);
        assert_eq!(
            m.clone().finish_audio_trace(),
            before.clone().finish_audio_trace()
        );
    }
    m.install_rom_patches(&[patch(DATA, 0x12)]).unwrap();
    m.install_rom_patches(&[CheatPatch::WideRomWriteIfEquals {
        address: DATA,
        value: CheatValue::Constant(0xa5),
        compare: CheatValue::Constant(0xee),
    }])
    .unwrap();
    assert_eq!(m, before);
    assert!(
        m.install_rom_patches(&[patch(DATA, 0xa5), patch(ROM_LEN as u32, 0)])
            .is_err()
    );
    assert_eq!(m, before);
    assert_eq!(
        m.clone().finish_audio_trace(),
        before.clone().finish_audio_trace()
    );
    m.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
    let accepted = m.clone();
    let encoded = states(&m);
    m.install_rom_patches(&[patch(DATA, 0), patch(DATA, 0xa5)])
        .unwrap();
    assert_eq!(m, accepted);
    assert!(m.reset_and_begin_audio_trace(32).is_err());
    assert_eq!(m, accepted);
    assert_eq!(states(&m), encoded);
    assert_eq!(
        m.clone().finish_audio_trace().unwrap().trace.invalidated,
        Some(AudioTraceInvalidation::ExternalMutation)
    );
    m.clear_rom_patches();
    assert_eq!(states(&m), encoded);
    let capture = m.finish_audio_trace().unwrap();
    assert!(!capture.trace.events.is_empty());
    assert_eq!(
        capture.trace.invalidated,
        Some(AudioTraceInvalidation::ExternalMutation)
    );
    assert!(capture.trace.validate_complete().is_err());
}
