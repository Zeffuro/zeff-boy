use super::*;
use crate::emulator::InstructionObserver;
use zeff_m68k::timing::{InstructionByteOrigin, InstructionInput};

const TARGET: u32 = 0x202100;
const DATA: u32 = 0x202000;

fn machine(concurrent_sound: bool, program: &[u16]) -> Emulator {
    let mut bytes = crate::cartridge::registers::tests::normal_rom_fixture(LockOn::TwoMiB);
    write_words(&mut bytes, 0x200, &[0x4ef9, 0xff, 0x0200, 0x4e71]);
    write_words(
        &mut bytes,
        TARGET as usize,
        &[0x7001, 0x0640, 3, 0x4e75, 0x4e71],
    );
    write_words(&mut bytes, 0x1000, &[0x1234]);
    write_words(&mut bytes, 0x18000, &[0x1234, 0x5678]);
    write_words(&mut bytes, DATA as usize, &[0x5678, 0x9abc]);
    let cart = crate::cartridge::registers::tests::inserted_save_fixture_with_rom(bytes);
    let mut machine = Emulator::new_with_cartridge(
        cart,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
    )
    .unwrap();
    write_words(&mut machine.bus.ram, 0x200, program);
    consumer::setup(&mut machine, TARGET);
    machine.step_instruction().unwrap();
    assert_eq!(machine.registers().pc, 0xff0200);
    machine
}

fn set(value: Option<(u8, u8)>) -> Vec<CheatPatch> {
    value
        .map(|(opcode, data)| vec![patch(TARGET + 1, opcode), patch(DATA, data)])
        .unwrap_or_default()
}

fn offsets(input: InstructionInput) -> [Option<u64>; 2] {
    input.origin.map(InstructionByteOrigin::storage_offset)
}

#[derive(Default)]
struct Capture(Vec<(u32, InstructionInput)>);

impl InstructionObserver for Capture {
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.0.push((address, input));
    }
}

pub(super) fn main_contract() {
    for concurrent in [false, true] {
        for enabled in [false, true] {
            let mut m = machine(concurrent, &[0x60fe, 0x4e71]);
            m.load_save_ram(&[0x5a; 512]).unwrap();
            let mut program = vec![
                0x13fc,
                u16::from(enabled),
                0xa1,
                0x30f1,
                0x13fc,
                0x77,
                0x20,
                3,
                0x1039,
                0,
                0x1000,
                0x1239,
                0x20,
                0x2000,
                0x3439,
                0x20,
                0x2000,
                0x1639,
                0x20,
                0x2001,
                0x1839,
                0x20,
                1,
                0x3a39,
                0x20,
                0x0400,
            ];
            if enabled {
                program.extend([0x7c03, 0x7e04]);
            } else {
                program.extend([0x3c39, 0x30, 0, 0x1e39, 0x3f, 0xffff]);
            }
            program.extend([0x4e75, 0x4e71]);
            write_words(&mut m.bus.ram, 0x400, &program);
            let bytes = m.bus.cartridge.bytes.clone();
            let digest = m.bus.cartridge.rom_digest();
            for (text, values) in [
                (None, [0x12, 0x56, 0x5678, 0x78, 0x2c2c, 0x2f]),
                (
                    Some("001000:A55A+202000:B66B+300000:C77C+3FFFFF:8D"),
                    [0xa5, 0xb6, 0xb66b, 0x6b, 0xc77c, 0x8d],
                ),
                (
                    Some("001000:96E1+202000:4826+300000:D88D+3FFFFF:6E"),
                    [0x96, 0x48, 0x4826, 0x26, 0xd88d, 0x6e],
                ),
                (None, [0x12, 0x56, 0x5678, 0x78, 0x2c2c, 0x2f]),
            ] {
                let before = m.encode_frontend_state().unwrap();
                consumer::overlay(&mut m, &text.map(patches).unwrap_or_default());
                assert_eq!(m.encode_frontend_state().unwrap(), before);
                let context = m.begin_debug_call(0xff0400).unwrap();
                for _ in 0..11 {
                    m.step_instruction().unwrap();
                }
                assert!(m.debug_call_returned(&context));
                m.finish_debug_call(context).unwrap();
                assert_eq!(
                    m.registers().d,
                    [
                        values[0],
                        values[1],
                        values[2],
                        values[3],
                        if enabled { 0x5a } else { 0x28 },
                        0x2828,
                        if enabled { 3 } else { values[4] },
                        if enabled { 4 } else { values[5] }
                    ]
                );
                assert_eq!(
                    m.dump_save_ram().unwrap()[1],
                    if enabled { 0x77 } else { 0x5a }
                );
                assert_eq!(
                    m.bus.cartridge.rom_source_offset(0x200001),
                    (!enabled).then_some(0x200001)
                );
                assert_eq!(
                    m.bus.cartridge.rom_source_offset(0x300000),
                    (!enabled).then_some(0x300000)
                );
                assert_eq!(m.bus.cartridge.rom_source_offset(DATA), Some(DATA as usize));
                let sampled = m.bus.cartridge.read_sampled(0x200001).unwrap();
                assert_eq!(sampled.value, if enabled { 0x5a } else { 0x28 });
                assert_eq!(
                    sampled.origin.storage_offset(),
                    (!enabled).then_some(0x200001)
                );
                if enabled {
                    for address in [0x200000, 0x300000, 0x3fffff] {
                        assert!(m.bus.cartridge.read_sampled(address).is_none());
                        assert_eq!(m.peek(address), None);
                    }
                    for (address, lanes) in [
                        (0x200000, ByteLanes::Upper),
                        (0x200000, ByteLanes::Both),
                        (0x300001, ByteLanes::Lower),
                    ] {
                        let mut rejected = m.clone();
                        assert!(read(&mut rejected, address, lanes).is_err());
                        assert_eq!(
                            rejected.dump_save_ram().unwrap(),
                            m.dump_save_ram().unwrap()
                        );
                    }
                }
                assert!(Arc::ptr_eq(&bytes, &m.bus.cartridge.bytes));
                assert_eq!(m.bus.cartridge.rom_digest(), digest);
                assert!(!m.supports_rom_cheats());
                assert_eq!(m.rom_offset_for_cpu_address(DATA), None);
                assert!(m.check_debug_call(TARGET).is_err());
                assert!(m.check_warm_reset().is_err());
                assert!(
                    crate::hardware::bus::Bus::new_owned(m.bus.cartridge.clone(), Region::NtscUs)
                        .is_err()
                );
                let before = m.clone();
                let owned = m.encode_debugger_checkpoint().unwrap();
                for bad in [0x400000, 0xa130f1, u32::MAX] {
                    assert!(
                        m.install_rom_patches(&[patch(DATA, 0xee), patch(bad, 1)])
                            .is_err()
                    );
                    assert_eq!(m, before);
                    assert_eq!(m.encode_debugger_checkpoint().unwrap(), owned);
                }
                assert!(
                    m.install_rom_patches(&patches("202000:EE+000007:01"))
                        .is_err()
                );
                assert_eq!(m, before);
                if enabled {
                    assert!(m.begin_debug_call(0x200000).is_err());
                    assert!(m.begin_debug_call(0x300000).is_err());
                    assert_eq!(m, before);
                }
            }
            let before = m.clone();
            assert!(m.load_save_ram(&[0; 511]).is_err());
            assert_eq!(m, before);
            for mutation in 0..3 {
                let mut invalid = m.clone();
                match mutation {
                    0 => invalid.bus.cartridge.bytes = m.bus.cartridge.bytes.to_vec().into(),
                    1 => Arc::make_mut(&mut invalid.bus.cartridge.bytes)[0x1000] ^= 1,
                    _ => invalid.bus.cartridge.sf002 = Some(crate::cartridge::sf002::Sf002::new()),
                }
                assert!(!invalid.supports_rom_cheats());
                let before = invalid.clone();
                assert!(invalid.install_rom_patches(&[patch(DATA, 0xee)]).is_err());
                assert_eq!(invalid, before);
            }
            let battery = m.dump_save_ram().unwrap();
            m.reset();
            assert_eq!(m.dump_save_ram().unwrap(), battery);
            assert!(!m.bus.cartridge.lock_on_save.as_ref().unwrap().enabled);
            assert_eq!(m.bus.cartridge.lock_on, Some(LockOn::TwoMiB));
            assert_eq!(m.registers().pc, 0x200);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unsealed_inserted_save_internal_owned_history_and_real_callee_keep_current_set() {
    for concurrent in [false, true] {
        let mut source = machine(concurrent, &[0x60fe, 0x4e71]);
        consumer::overlay(&mut source, &set(Some((7, 0xa5))));
        let undo = source.encode_debugger_checkpoint().unwrap();
        let old_queue = source.cpu.queued_instruction_inputs();
        let return_pc = source.registers().pc;
        let return_sp = source.registers().stack_pointer();
        let context = consumer::begin(&mut source, TARGET);
        let queued = source.cpu.queued_instruction_inputs().unwrap();
        assert_eq!(queued[0].value, 0x7007);
        assert_eq!(offsets(queued[0]), [Some(0x202100), Some(0x202101)]);
        let raw = source.encode_state().unwrap();
        let front = source.encode_frontend_state().unwrap();
        let owned = source.encode_debugger_checkpoint().unwrap();
        for current in [Some((9, 0x5a)), None] {
            for format in 0..3 {
                let mut receiver = source.clone();
                consumer::overlay(&mut receiver, &set(current));
                receiver.step_instruction().unwrap();
                receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
                let overlay = receiver.bus.cartridge.rom_overlay.clone();
                match format {
                    0 => {
                        receiver.decode_state(&raw).unwrap();
                        assert_eq!(receiver.encode_state().unwrap(), raw);
                    }
                    1 => {
                        receiver.decode_frontend_state(&front).unwrap();
                        assert_eq!(receiver.encode_frontend_state().unwrap(), front);
                    }
                    2 => {
                        receiver.decode_debugger_checkpoint(&owned).unwrap();
                        assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), owned);
                    }
                    _ => unreachable!(),
                }
                assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                assert_eq!(receiver.queued_words(), Some([0x7007, 0x0640]));
                if format == 2 {
                    assert_eq!(receiver.cpu.queued_instruction_inputs().unwrap(), queued);
                } else {
                    assert!(
                        receiver
                            .cpu
                            .queued_instruction_inputs()
                            .unwrap()
                            .iter()
                            .all(|input| input.origin == [InstructionByteOrigin::UNKNOWN; 2])
                    );
                }
                let before = receiver.clone();
                for bad in 0..3 {
                    let failed = match bad {
                        0 => receiver.decode_state(&raw[..raw.len() - 1]),
                        1 => receiver.decode_frontend_state(&front[..front.len() - 1]),
                        _ => receiver.decode_debugger_checkpoint(&owned[..owned.len() - 1]),
                    };
                    assert!(failed.is_err());
                    assert_eq!(receiver, before);
                }
                let mut capture = Capture::default();
                for _ in 0..3 {
                    receiver.step_instruction_traced(&mut capture).unwrap();
                }
                consumer::return_to_caller(&mut receiver);
                assert!(receiver.debug_call_returned(&context));
                assert_eq!(receiver.registers().d[0], 10);
                assert_eq!(
                    (
                        receiver.registers().pc,
                        receiver.registers().stack_pointer()
                    ),
                    (return_pc, return_sp)
                );
                assert_eq!(capture.0[0].0, TARGET);
                assert_eq!(capture.0[0].1.value, 0x7007);
                assert_eq!(
                    capture.0[0].1.origin,
                    if format == 2 {
                        queued[0].origin
                    } else {
                        [InstructionByteOrigin::UNKNOWN; 2]
                    }
                );
                assert_eq!(capture.0.last().unwrap().0, TARGET + 6);
                assert_eq!(
                    receiver.peek(TARGET + 1),
                    Some(if current.is_some() { 9 } else { 1 })
                );
            }
        }
        consumer::overlay(&mut source, &set(Some((9, 0x5a))));
        for _ in 0..3 {
            source.step_instruction().unwrap();
        }
        consumer::return_to_caller(&mut source);
        assert!(source.debug_call_returned(&context));
        source.finish_debug_call(context).unwrap();
        source.decode_debugger_checkpoint(&undo).unwrap();
        assert_eq!(source.cpu.queued_instruction_inputs(), old_queue);
        assert_eq!(source.peek(TARGET + 1), Some(9));
        assert_eq!(source.peek(DATA), Some(0x5a));
        let context = consumer::begin(&mut source, TARGET);
        for _ in 0..3 {
            source.step_instruction().unwrap();
        }
        consumer::return_to_caller(&mut source);
        assert!(source.debug_call_returned(&context));
        source.finish_debug_call(context).unwrap();
        assert_eq!(source.registers().d[0], 12);
        let initial = source.bus.cartridge.initial;
        consumer::overlay(
            &mut source,
            &patches("000004:AB30+000006:0100+300100:700B+300102:0640"),
        );
        source.reset();
        assert_eq!(source.bus.cartridge.initial, initial);
        assert_eq!(
            (source.registers().pc, source.registers().ssp),
            (0xab300100, 0x00fffffc)
        );
        assert_eq!(source.queued_words(), Some([0x700b, 0x0640]));
        let mut capture = Capture::default();
        source.step_instruction_traced(&mut capture).unwrap();
        assert_eq!(capture.0[0].0, 0xab300100);
        assert_eq!(offsets(capture.0[0].1), [Some(0x300100), Some(0x300101)]);
        assert_eq!(source.registers().d[0], 11);
        source.clear_rom_patches();
        source.reset();
        assert_eq!(
            (source.registers().pc, source.registers().ssp),
            (0x200, 0x00fffffc)
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unsealed_inserted_save_internal_sound_publication_keeps_current_rom_or_sram() {
    use super::sound::{arm, pending};
    for enabled in [false, true] {
        let mut donor = machine(
            true,
            &[0x13fc, u16::from(enabled), 0xa1, 0x30f1, 0x60fe, 0x4e71],
        );
        donor.load_save_ram(&[0x37; 512]).unwrap();
        donor.step_instruction().unwrap();
        consumer::overlay(&mut donor, &[patch(0x200001, 0xa5)]);
        assert!(donor.bus.cartridge.mode_sensitive_read(0x200001));
        arm(&mut donor, 0x200001, &[0x3a, 1, 0x80, 0x3a, 1, 0x80, 0x76]);
        pending(&mut donor, 0x200001);
        let prepared = donor.sound_schedule.pending.as_ref().unwrap().clone();
        assert!(prepared.has_read_replay());
        assert_eq!(prepared.cpu.regs().a, if enabled { 0x37 } else { 0xa5 });
        let deadline = donor.sound_deadline().unwrap().get();
        let raw = donor.encode_state().unwrap();
        let front = donor.encode_frontend_state().unwrap();
        let owned = donor.encode_debugger_checkpoint().unwrap();
        for current in [Some(0xa5), Some(0x5a), None] {
            for format in 0..3 {
                let mut receiver = donor.clone();
                consumer::overlay(
                    &mut receiver,
                    &current
                        .map(|value| vec![patch(0x200001, value)])
                        .unwrap_or_default(),
                );
                receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
                let overlay = receiver.bus.cartridge.rom_overlay.clone();
                match format {
                    0 => receiver.decode_state(&raw).unwrap(),
                    1 => receiver.decode_frontend_state(&front).unwrap(),
                    _ => receiver.decode_debugger_checkpoint(&owned).unwrap(),
                }
                assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&prepared));
                assert_eq!(receiver.sound_deadline().unwrap().get(), deadline);
                receiver.load_save_ram(&[0x77; 512]).unwrap();
                assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&prepared));
                let expected = if enabled {
                    0x77
                } else {
                    current.unwrap_or(0x28)
                };
                let before = receiver.sound_cpu.clone();
                let remaining = deadline - receiver.bus.now.get();
                assert!(remaining > 1);
                advance(&mut receiver, remaining - 1);
                assert_eq!(receiver.sound_cpu, before);
                advance(&mut receiver, 1);
                assert_eq!(receiver.sound_fault(), None);
                assert_eq!(receiver.sound_cpu.regs().a, expected);
                pending(&mut receiver, 0x200001);
                let remaining = receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
                advance(&mut receiver, remaining);
                assert_eq!(receiver.sound_cpu.regs().a, expected);
                let sampled = receiver.bus.cartridge.read_sampled(0x200001).unwrap();
                assert_eq!(sampled.value, expected);
                assert_eq!(
                    sampled.origin.storage_offset(),
                    (!enabled).then_some(0x200001)
                );
                assert_eq!(receiver.dump_save_ram().unwrap(), [0x77; 512]);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unsealed_inserted_save_dma_retains_rom_fifo_and_rejects_enabled_even_lane_and_upper_hole() {
    use super::super::sound_tests::{
        assert_dma_fifo_retains_sampled_word_with_overlay, dma_fifo_program,
    };
    assert_dma_fifo_retains_sampled_word_with_overlay(
        machine(true, &dma_fifo_program()),
        Region::NtscUs,
        consumer::overlay,
    );
    for (target, high) in [(0x200000, 0x9710), (0x300000, 0x9718)] {
        let mut program = vec![0x13fc, 1, 0xa1, 0x30f1, 0x13fc, 0x77, 0x20, 1];
        for value in [0x8114, 0x8f02, 0x9301, 0x9400, 0x9500, 0x9600, high, 0x4400] {
            program.extend([0x33fc, value, 0xc0, 4]);
        }
        program.extend([0x60fe, 0x4e71]);
        let mut source = machine(true, &program);
        consumer::overlay(&mut source, &patches("200000:A55A+300000:B66B"));
        for _ in 0..10 {
            source.step_instruction().unwrap();
        }
        assert!(source.bus.cartridge.lock_on_save.as_ref().unwrap().enabled);
        assert_eq!(source.dump_save_ram().unwrap()[0], 0x77);
        let before = source.bus.vdp.clone();
        let battery = source.dump_save_ram().unwrap();
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
        assert_eq!(source.dump_save_ram().unwrap(), battery);
        assert!(source.bus.cartridge.read_sampled(target).is_none());
    }
}
