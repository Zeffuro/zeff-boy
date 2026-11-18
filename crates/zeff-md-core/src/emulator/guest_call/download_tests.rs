use super::tests::{REGIONS, ram};
use super::*;
use std::sync::Arc;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_m68k::timing::ControlFlowEntryKind;

mod queue_tests;
mod refusal_tests;
mod undo_tests;

const START: u32 = 0x50000;
const RAM_TARGET: u32 = 0xff1000;

fn words(bytes: &mut [u8], address: u32, values: &[u16]) {
    for (index, value) in values.iter().enumerate() {
        let at = address as usize + index * 2;
        bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
}

fn image(pc: u32, program: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0; 0x300000];
    bytes[..4].copy_from_slice(&0xfffffc_u32.to_be_bytes());
    bytes[4..8].copy_from_slice(&pc.to_be_bytes());
    bytes[0x80..0x90].copy_from_slice(b"DOWNLOAD_RAM_TST");
    bytes[0x100..0x104].copy_from_slice(b"SEGA");
    bytes[0x1b0..0x1bc].fill(b' ');
    words(&mut bytes, pc, program);
    bytes
}

fn machine(bytes: Vec<u8>, region: Region, concurrent_sound: bool) -> Emulator {
    let mut machine = Emulator::new(
        bytes,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap();
    machine.set_sample_rate(48_000);
    machine
}

fn patch(address: u32, value: u8) -> CheatPatch {
    CheatPatch::WideRomWrite {
        address,
        value: CheatValue::Constant(value),
    }
}

fn unknown_queue(machine: &Emulator) {
    assert!(
        machine
            .cpu
            .queued_instruction_inputs()
            .unwrap()
            .iter()
            .all(|input| input
                .origin
                .iter()
                .all(|origin| origin.storage_offset().is_none()))
    );
}

fn same(actual: &Emulator, expected: &Emulator) {
    assert_eq!(actual, expected);
    assert_eq!(
        actual.encode_state().unwrap(),
        expected.encode_state().unwrap()
    );
    assert_eq!(
        actual.encode_frontend_state().unwrap(),
        expected.encode_frontend_state().unwrap()
    );
    assert_eq!(
        actual.encode_debugger_checkpoint().unwrap(),
        expected.encode_debugger_checkpoint().unwrap()
    );
    assert_eq!(actual.call_stack_status(), expected.call_stack_status());
    assert_eq!(
        actual.call_stack().copied().collect::<Vec<_>>(),
        expected.call_stack().copied().collect::<Vec<_>>()
    );
    assert_eq!(actual.framebuffer(), expected.framebuffer());
}

fn future_field(actual: &mut Emulator, expected: &mut Emulator) {
    actual.step_field().unwrap();
    expected.step_field().unwrap();
    same(actual, expected);
    let pcm = actual.drain_audio_samples();
    assert!(!pcm.is_empty() && pcm.iter().any(|sample| *sample != 0.0));
    assert!(
        pcm.iter().map(|sample| sample.to_bits()).eq(expected
            .drain_audio_samples()
            .iter()
            .map(|sample| sample.to_bits()))
    );
    same(actual, expected);
}

fn equal_jsr_hardware(actual: &Emulator, normal: &Emulator) {
    let mut normal = normal.clone();
    // The two real guest programs differ only to align their return PCs.
    normal.bus.cartridge = actual.bus.cartridge.clone();
    assert_eq!(actual, &normal);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_ram_call_first_matches_real_jsr_entry_return_and_owned_undo() {
    for region in REGIONS {
        for concurrent in [false, true] {
            let mut actual = machine(
                image(START, &[0x41f9, 0xff, 0x1000, 0x4e71, 0x4e71]),
                region,
                concurrent,
            );
            let mut normal = machine(
                image(START - 2, &[0x41f9, 0xff, 0x1000, 0x4e90, 0x4e71, 0x4e71]),
                region,
                concurrent,
            );
            for machine in [&mut actual, &mut normal] {
                ram(machine, RAM_TARGET, &[0x7007, 0x4e75, 0x4e71]);
                machine.step_instruction().unwrap();
            }
            assert_eq!(actual.registers().pc, START + 6);
            assert_eq!(normal.registers().pc, START + 4);
            let before = actual.clone();
            let checkpoint = actual.encode_debugger_checkpoint().unwrap();
            let board = actual.bus.cartridge.download.clone();
            let normal_board = normal.bus.cartridge.download.clone();
            assert_eq!(actual.check_debug_call(RAM_TARGET), Ok(()));
            same(&actual, &before);
            let tick = actual.bus.now;
            let clock = actual.cpu_clock();
            let context = actual.begin_debug_call(RAM_TARGET).unwrap();
            let jsr = normal.step_instruction().unwrap();
            assert!(matches!(jsr.cpu, TimedOutcome::Instruction(step) if step.opcode == 0x4e90));
            assert_eq!(
                actual.bus.now.checked_duration_since(tick),
                Some(jsr.elapsed)
            );
            assert_eq!(actual.cpu_clock().0 - clock.0, 16);
            assert_eq!(actual.registers().stack_pointer(), 0xfffff8);
            assert_eq!(&actual.work_ram()[0xfff8..0xfffc], &[0, 5, 0, 6]);
            assert_eq!(actual.queued_words(), Some([0x7007, 0x4e75]));
            assert_eq!(actual.bus.cartridge.download, board);
            assert_eq!(normal.bus.cartridge.download, normal_board);
            equal_jsr_hardware(&actual, &normal);
            assert!(!actual.debug_call_returned(&context));
            for opcode in [0x7007, 0x4e75] {
                let step = actual.step_instruction().unwrap();
                assert!(
                    matches!(step.cpu, TimedOutcome::Instruction(step) if step.opcode == opcode)
                );
                assert_eq!(step, normal.step_instruction().unwrap());
                assert_eq!(actual.bus.cartridge.download, board);
                assert_eq!(normal.bus.cartridge.download, normal_board);
                equal_jsr_hardware(&actual, &normal);
            }
            assert!(actual.debug_call_returned(&context));
            let bus = actual.bus.clone();
            let clock = actual.cpu_clock();
            actual.finish_debug_call(context).unwrap();
            assert_eq!(actual.bus, bus);
            assert_eq!(actual.cpu_clock(), clock);
            assert_eq!(actual.registers().d[0], 7);
            assert_eq!(actual.registers().pc, START + 6);
            assert_eq!(actual.queued_words(), Some([0x4e71, 0x4e71]));
            unknown_queue(&actual);
            assert_eq!(actual.rom_offset_for_cpu_address(START + 6), None);
            assert!(!actual.supports_rom_breakpoints());
            equal_jsr_hardware(&actual, &normal);
            actual.decode_debugger_checkpoint(&checkpoint).unwrap();
            same(&actual, &before);
        }
    }
}
