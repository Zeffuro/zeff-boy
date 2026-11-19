use super::*;
use crate::emulator::{Emulator, LoadConfig, Region, TimingModel};
use zeff_emu_common::time::MachineTiming;

pub(super) const PROFILES: [(TimingModel, bool); 3] = [
    (TimingModel::Standard, false),
    (TimingModel::Standard, true),
    (TimingModel::Precise, true),
];

pub(super) fn fixture(
    program: &[u16],
    region: Region,
    timing: TimingModel,
    concurrent: bool,
) -> Emulator {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
        timing,
    )
    .unwrap()
}

pub(super) fn step(actual: &mut Emulator, expected: &mut Emulator) -> Vec<MemoryAccess> {
    let mut events = Vec::new();
    assert_eq!(
        actual.step_instruction_observed(&mut |event| events.push(event)),
        expected.step_instruction()
    );
    assert_eq!(actual, expected);
    assert_eq!(
        actual.encode_state().unwrap(),
        expected.encode_state().unwrap()
    );
    assert_eq!(
        actual.encode_frontend_state().unwrap(),
        expected.encode_frontend_state().unwrap()
    );
    events
}

fn bytes(events: &[MemoryAccess]) -> Vec<(u32, u8, Direction)> {
    events
        .iter()
        .map(|event| (event.address, event.value.unwrap(), event.direction))
        .collect()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn actual_cpu_widths_ports_stop_and_continuation_preserve_all_profiles_and_regions() {
    let program = [
        0x13fc, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x13fc, 0xa5,
        0xff, 0x41, 0x33fc, 0x1234, 0xff, 0x42, 0x23fc, 0x89ab, 0xcdef, 0xff, 0x44, 0x1039, 0xff,
        0x41, 0x3039, 0xff, 0x42, 0x2039, 0xff, 0x44, 0x4e72, 0x2700,
    ];
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for (timing, concurrent) in PROFILES {
            let mut actual = fixture(&program, region, timing, concurrent);
            actual.set_sample_rate(44_100);
            let mut expected = actual.clone();
            let mut events = Vec::new();
            for _ in 0..10 {
                events.extend(step(&mut actual, &mut expected));
            }
            assert_eq!(
                bytes(&events),
                [
                    (0xc00011, 0x85, Direction::Write),
                    (0xc00011, 6, Direction::Write),
                    (0xc00011, 0x90, Direction::Write),
                    (0xff0041, 0xa5, Direction::Write),
                    (0xff0042, 0x12, Direction::Write),
                    (0xff0043, 0x34, Direction::Write),
                    (0xff0044, 0x89, Direction::Write),
                    (0xff0045, 0xab, Direction::Write),
                    (0xff0046, 0xcd, Direction::Write),
                    (0xff0047, 0xef, Direction::Write),
                    (0xff0041, 0xa5, Direction::Read),
                    (0xff0042, 0x12, Direction::Read),
                    (0xff0043, 0x34, Direction::Read),
                    (0xff0044, 0x89, Direction::Read),
                    (0xff0045, 0xab, Direction::Read),
                    (0xff0046, 0xcd, Direction::Read),
                    (0xff0047, 0xef, Direction::Read),
                ]
            );
            assert_eq!(
                events
                    .iter()
                    .map(|event| event.old_value)
                    .collect::<Vec<_>>(),
                [None, None, None]
                    .into_iter()
                    .chain([Some(0); 7])
                    .chain([None; 7])
                    .collect::<Vec<_>>()
            );
            assert!(actual.main_cpu_stopped());
            for _ in 0..3 {
                assert!(step(&mut actual, &mut expected).is_empty());
            }
            actual.step_field().unwrap();
            expected.step_field().unwrap();
            assert_eq!(actual, expected);
            assert_eq!(actual.framebuffer(), expected.framebuffer());
            let pcm = actual.drain_audio_samples();
            assert!(pcm.iter().any(|sample| *sample != 0.0));
            assert!(
                pcm.iter().map(|sample| sample.to_bits()).eq(expected
                    .drain_audio_samples()
                    .iter()
                    .map(|sample| sample.to_bits()))
            );
            assert_eq!(
                actual.encode_state().unwrap(),
                expected.encode_state().unwrap()
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn stack_and_program_space_operand_reads_are_observed_without_fetches_or_discards() {
    for (timing, concurrent) in PROFILES {
        let mut actual = fixture(
            &[0x4cba, 1, 6, 0x4e71, 0x60fe, 0x1234],
            Region::NtscUs,
            timing,
            concurrent,
        );
        let mut expected = actual.clone();
        assert_eq!(
            bytes(&step(&mut actual, &mut expected)),
            [
                (0x20a, 0x12, Direction::Read),
                (0x20b, 0x34, Direction::Read),
            ]
        );
        assert_eq!(actual.registers().d[0], 0x1234);

        let mut actual = fixture(
            &[0x6104, 0x60fe, 0x4e71, 0x4e75, 0x4e71],
            Region::NtscUs,
            timing,
            concurrent,
        );
        let mut expected = actual.clone();
        let writes = step(&mut actual, &mut expected);
        assert_eq!(writes.len(), 4);
        assert!(
            writes
                .iter()
                .all(|event| event.direction == Direction::Write && event.address >= 0xfffff8)
        );
        let reads = step(&mut actual, &mut expected);
        assert_eq!(reads.len(), 4);
        assert!(
            reads
                .iter()
                .all(|event| event.direction == Direction::Read && event.address >= 0xfffff8)
        );
        assert_eq!(actual.registers().pc, 0x202);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unchanged_writes_predecrement_order_and_physical_wrap_report_actual_driven_bytes() {
    for (timing, concurrent) in PROFILES {
        let mut actual = fixture(
            &[0x2100, 0x4e71, 0x4e71],
            Region::NtscUs,
            timing,
            concurrent,
        );
        actual.cpu.registers.a[0] = 0xff0044;
        actual.cpu.registers.d[0] = 0x12345678;
        let mut expected = actual.clone();
        assert_eq!(
            bytes(&step(&mut actual, &mut expected)),
            [
                (0xff0042, 0x56, Direction::Write),
                (0xff0043, 0x78, Direction::Write),
                (0xff0040, 0x12, Direction::Write),
                (0xff0041, 0x34, Direction::Write),
            ]
        );

        let mut actual = fixture(
            &[0x13fc, 0, 0xff, 0x41, 0x4e71, 0x4e71],
            Region::NtscUs,
            timing,
            concurrent,
        );
        let mut expected = actual.clone();
        assert_eq!(
            bytes(&step(&mut actual, &mut expected)),
            [(0xff0041, 0, Direction::Write)]
        );
    }
    for concurrent in [false, true] {
        let mut actual = fixture(
            &[0x23fc, 0x1234, 0x5678, 0xff, 0xfffe, 0x4e71, 0x4e71],
            Region::NtscUs,
            TimingModel::Standard,
            concurrent,
        );
        let mut expected = actual.clone();
        assert_eq!(
            bytes(&step(&mut actual, &mut expected)),
            [
                (0xfffffe, 0x12, Direction::Write),
                (0xffffff, 0x34, Direction::Write),
                (0, 0x56, Direction::Write),
                (1, 0x78, Direction::Write),
            ]
        );
        assert_eq!(actual.peek(0), Some(0));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ram_tas_observation_preserves_ownership_suppressed_storage_and_fault_priority() {
    for concurrent in [false, true] {
        let mut actual = fixture(
            &[0x4ad0, 0x4e71, 0x4e71],
            Region::NtscUs,
            TimingModel::Standard,
            concurrent,
        );
        actual.cpu.registers.a[0] = 0xff0041;
        let mut expected = actual.clone();
        assert_eq!(
            bytes(&step(&mut actual, &mut expected)),
            [
                (0xff0041, 0, Direction::Read),
                (0xff0041, 0x80, Direction::Write),
            ]
        );
        assert_eq!(actual.peek(0xff0041), Some(0));
        assert!(actual.bus.rmw.is_none());

        let mut actual = fixture(
            &[0x23fc, 0x1234, 0x5678, 0x7f, 0xfffe, 0x4e71, 0x4e71],
            Region::NtscUs,
            TimingModel::Standard,
            concurrent,
        );
        let mut expected = actual.clone();
        let mut events = Vec::new();
        let failure = actual.step_instruction_observed(&mut |event| events.push(event));
        assert!(failure.is_err());
        assert_eq!(failure, expected.step_instruction());
        assert_eq!(actual, expected);
        assert_eq!(
            bytes(&events),
            [
                (0x7ffffe, 0x12, Direction::Write),
                (0x7fffff, 0x34, Direction::Write)
            ]
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn z80_mutations_during_main_stop_do_not_produce_cpu_access_events() {
    let mut program = Vec::new();
    let mut write = |address: u32, value: u8| {
        program.extend([0x13fc, value.into(), (address >> 16) as u16, address as u16]);
    };
    write(0xa11100, 1);
    write(0xa11200, 1);
    let mut sound = vec![0; 32];
    sound.extend([0x21, 0, 1, 0x34, 0x18, 0xfd]);
    for (offset, &value) in sound.iter().enumerate() {
        write(0xa00000 + offset as u32, value);
    }
    write(0xa11100, 0);
    program.extend([0x4e72, 0x2700, 0x60fe, 0x4e71]);
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut actual = fixture(&program, Region::NtscUs, timing, true);
        for _ in 0..128 {
            if actual.main_cpu_stopped() {
                break;
            }
            actual.step_instruction().unwrap();
        }
        assert!(actual.main_cpu_stopped(), "{timing:?}");
        assert!(
            !actual.sound_reset_asserted() && !actual.sound_bus_requested(),
            "{timing:?}"
        );
        assert_eq!(&actual.bus.active_sound_memory().ram[..sound.len()], &sound);
        let before = actual.bus.active_sound_memory().read(0x100).unwrap();
        let initial_pc = actual.sound_cpu().regs().pc;
        let initial_hl = actual.sound_cpu().regs().hl();
        let initial_cycles = actual.sound_cpu().cycles();
        let start = actual.timing_snapshot().now();
        let mut expected = actual.clone();
        let mut changed = false;
        for _ in 0..2048 {
            assert!(step(&mut actual, &mut expected).is_empty());
            if actual.bus.active_sound_memory().read(0x100) != Some(before) {
                changed = true;
                break;
            }
        }
        assert!(actual.timing_snapshot().now() > start);
        assert!(
            changed,
            "{timing:?}: SOUND PC/HL/cycles {initial_pc:04x}/{initial_hl:04x}/{initial_cycles} -> {:04x}/{:04x}/{}; reset={} requested={} code={:02x?}",
            actual.sound_cpu().regs().pc,
            actual.sound_cpu().regs().hl(),
            actual.sound_cpu().cycles(),
            actual.sound_reset_asserted(),
            actual.sound_bus_requested(),
            &actual.bus.active_sound_memory().ram[..sound.len()]
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn vdp_dma_copy_progress_during_cpu_fetches_is_not_a_cpu_memory_access() {
    let region = Region::NtscUs;
    for concurrent in [false, true] {
        let mut actual = fixture(&[0x4e71; 8], region, TimingModel::Standard, concurrent);
        actual.bus.now = zeff_emu_common::time::MasterTicks::ZERO;
        actual.bus.vdp = crate::Vdp::bootstrap_mode5();
        actual.bus.audio = crate::hardware::audio::Audio::default();
        for (index, value) in [(15, 2), (19, 2), (20, 0), (21, 0), (22, 0), (23, 0xc0)] {
            actual.bus.advance_machine(1).unwrap();
            let write = actual.bus.vdp.queue_register(index, value, region).unwrap();
            actual
                .bus
                .advance_machine(write.publish_at.get() - actual.bus.now.get())
                .unwrap();
        }
        actual.bus.vdp.write_control(0x4000, region).unwrap();
        actual.bus.vdp.write_control(0, region).unwrap();
        actual.bus.vdp.write_data(0x1234, region).unwrap();
        actual
            .bus
            .advance_machine(201 - actual.bus.now.get())
            .unwrap();
        actual.bus.advance_machine(1).unwrap();
        let write = actual.bus.vdp.queue_register(15, 1, region).unwrap();
        actual
            .bus
            .advance_machine(write.publish_at.get() - actual.bus.now.get())
            .unwrap();
        actual
            .bus
            .advance_machine(1001 - actual.bus.now.get())
            .unwrap();
        actual
            .bus
            .vdp
            .resume_dma_from_known_phase(
                crate::ControlCommand {
                    address: 0x100,
                    code: 0x30,
                },
                None,
                region,
            )
            .unwrap();
        let mut expected = actual.clone();
        for _ in 0..3 {
            assert!(step(&mut actual, &mut expected).is_empty());
        }
        assert_eq!(&actual.vdp().memory().vram()[0x100..0x102], &[0x12, 0x34]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn clr_discarded_operands_report_unknown_samples_and_preserve_complete_execution() {
    for (timing, concurrent) in PROFILES {
        for (opcode, address, width) in [
            (0x4239, 0xff0041_u32, 1),
            (0x4279, 0xff0040, 2),
            (0x42b9, 0xff0040, 4),
        ] {
            let mut program = vec![
                0x13fc, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11,
            ];
            program.extend([
                opcode,
                (address >> 16) as u16,
                address as u16,
                0x4e72,
                0x2700,
                0x4e71,
            ]);
            let mut actual = fixture(&program, Region::NtscUs, timing, concurrent);
            actual.set_sample_rate(44_100);
            for _ in 0..3 {
                actual.step_instruction().unwrap();
            }
            actual.bus.ram[0x40..0x44].copy_from_slice(&[0xa5, 0x5a, 0x12, 0x34]);
            let mut expected = actual.clone();
            let events = step(&mut actual, &mut expected);
            let reads: Vec<_> = events
                .iter()
                .filter(|event| event.direction == Direction::Read)
                .collect();
            let writes: Vec<_> = events
                .iter()
                .filter(|event| event.direction == Direction::Write)
                .collect();
            assert_eq!(reads.len(), width);
            assert_eq!(writes.len(), width);
            assert_eq!(
                reads
                    .iter()
                    .map(|event| (event.address, event.value))
                    .collect::<Vec<_>>(),
                (0..width)
                    .map(|offset| (address + offset as u32, None))
                    .collect::<Vec<_>>()
            );
            assert!(reads.iter().all(|event| event.old_value.is_none()));
            let expected_writes = match width {
                1 => vec![(address, Some(0x5a), Some(0))],
                2 => vec![
                    (address, Some(0xa5), Some(0)),
                    (address + 1, Some(0x5a), Some(0)),
                ],
                4 => vec![
                    (address + 2, Some(0x12), Some(0)),
                    (address + 3, Some(0x34), Some(0)),
                    (address, Some(0xa5), Some(0)),
                    (address + 1, Some(0x5a), Some(0)),
                ],
                _ => unreachable!(),
            };
            assert_eq!(
                writes
                    .iter()
                    .map(|event| (event.address, event.old_value, event.value))
                    .collect::<Vec<_>>(),
                expected_writes
            );
            assert!(writes.iter().all(|event| event.value == Some(0)));
            for offset in 0..width {
                assert_eq!(actual.peek(address + offset as u32), Some(0));
            }
            if width == 1 {
                assert_eq!(actual.peek(0xff0040), Some(0xa5));
            }
            assert!(step(&mut actual, &mut expected).is_empty());
            actual.step_field().unwrap();
            expected.step_field().unwrap();
            assert_eq!(actual, expected);
            assert_eq!(
                actual.encode_state().unwrap(),
                expected.encode_state().unwrap()
            );
            let pcm = actual.drain_audio_samples();
            assert!(pcm.iter().any(|sample| *sample != 0.0));
            assert!(
                pcm.iter().map(|sample| sample.to_bits()).eq(expected
                    .drain_audio_samples()
                    .iter()
                    .map(|sample| sample.to_bits()))
            );
        }
    }
    for concurrent in [false, true] {
        let mut actual = fixture(
            &[0x4239, 0x80, 0, 0x4e71, 0x4e71],
            Region::NtscUs,
            TimingModel::Standard,
            concurrent,
        );
        let mut expected = actual.clone();
        let mut events = Vec::new();
        let result = actual.step_instruction_observed(&mut |event| events.push(event));
        assert!(result.is_err());
        assert_eq!(result, expected.step_instruction());
        assert_eq!(actual, expected);
        assert!(events.is_empty());
    }
}
