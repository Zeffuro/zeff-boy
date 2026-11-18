use super::*;
use zeff_m68k::{EXTEND, NEGATIVE, SUPERVISOR, TRACE};

pub(super) const PROFILES: [(TimingModel, bool); 3] = [
    (TimingModel::Standard, false),
    (TimingModel::Standard, true),
    (TimingModel::Precise, true),
];
pub(super) const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

pub(super) fn fixture(
    program: &[u16],
    pc: u32,
    sp: u32,
    region: Region,
    timing: TimingModel,
    concurrent: bool,
) -> Emulator {
    let mut rom = vec![0; 0x1000];
    rom[..4].copy_from_slice(&sp.to_be_bytes());
    rom[4..8].copy_from_slice(&pc.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    for (index, word) in program.iter().enumerate() {
        let at = pc as usize + index * 2;
        rom[at..at + 2].copy_from_slice(&word.to_be_bytes());
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

pub(super) fn ram(machine: &mut Emulator, address: u32, words: &[u16]) {
    let writes: Vec<_> = words
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .enumerate()
        .map(|(offset, value)| ((address + offset as u32) & BUS_ADDRESS_MASK, value))
        .collect();
    machine.debug_write_ram(&writes).unwrap();
}

fn equal_execution(actual: &Emulator, expected: &Emulator) {
    assert_eq!(actual.cpu, expected.cpu);
    assert_eq!(actual.bus.now, expected.bus.now);
    assert_eq!(actual.bus.cpu_clock, expected.bus.cpu_clock);
    assert_eq!(actual.bus.clock_fault, expected.bus.clock_fault);
    assert_eq!(actual.bus.prefetch, expected.bus.prefetch);
    assert_eq!(actual.bus.ram, expected.bus.ram);
    assert_eq!(actual.bus.sound, expected.bus.sound);
    assert_eq!(actual.sound_cpu, expected.sound_cpu);
    assert_eq!(actual.sound_schedule, expected.sound_schedule);
    assert_eq!(actual.bus.owned, expected.bus.owned);
    assert_eq!(actual.bus.vdp, expected.bus.vdp);
    assert_eq!(actual.bus.video, expected.bus.video);
    assert_eq!(actual.bus.audio, expected.bus.audio);
    assert_eq!(actual.bus.input, expected.bus.input);
    assert_eq!(actual.bus.reset_pulses, expected.bus.reset_pulses);
    assert_eq!(actual.bus.reset_output, expected.bus.reset_output);
    assert_eq!(actual.bus.rmw, expected.bus.rmw);
    assert_eq!(actual.main_stopped, expected.main_stopped);
    assert_eq!(actual.bus.cartridge.save, expected.bus.cartridge.save);
    assert_eq!(
        actual.bus.cartridge.rom_overlay,
        expected.bus.cartridge.rom_overlay
    );
    assert!(actual.bus.cartridge.supports_rom_overlay());
    assert!(expected.bus.cartridge.supports_rom_overlay());
    assert_ne!(actual.rom_bytes(), expected.rom_bytes());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn clocked_entry_matches_real_jsr_devices_all_profiles_regions_and_wrapping_spans() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            for (sp, target) in [
                (0xff1004, 0xff1000),
                (0, 0xfffffc),
                (0x01000000, 0xfffffc),
                (0xff3000, 0xfffffe),
            ] {
                let program = [0x41f9, (target >> 16) as u16, target as u16, 0x4e71, 0x4e71];
                let normal_program = [
                    0x41f9,
                    (target >> 16) as u16,
                    target as u16,
                    0x4e90,
                    0x4e71,
                    0x4e71,
                ];
                let mut actual = fixture(&program, 0x200, sp, region, timing, concurrent);
                let mut expected = fixture(&normal_program, 0x1fe, sp, region, timing, concurrent);
                for machine in [&mut actual, &mut expected] {
                    ram(
                        machine,
                        target,
                        if target == 0xfffffe {
                            &[0x4e75]
                        } else {
                            &[0x4e75, 0x4e71]
                        },
                    );
                    machine.step_instruction().unwrap();
                }
                assert_eq!(actual.registers().pc, 0x206);
                assert_eq!(expected.registers().pc, 0x204);
                assert_eq!(actual.cpu_clock(), expected.cpu_clock());
                let before_clock = actual.cpu_clock();
                let before_tick = actual.bus.now;
                let context = actual.begin_debug_call(target).unwrap();
                let normal = expected.step_instruction().unwrap();
                assert!(
                    matches!(normal.cpu, TimedOutcome::Instruction(step) if step.opcode == 0x4e90)
                );
                assert_eq!(
                    actual.bus.now.checked_duration_since(before_tick),
                    Some(normal.elapsed)
                );
                assert!(actual.cpu_clock().0 - before_clock.0 >= 16);
                assert_eq!(actual.registers().stack_pointer(), sp.wrapping_sub(4));
                assert_eq!(actual.queued_words().unwrap()[0], 0x4e75);
                equal_execution(&actual, &expected);
                assert!(!actual.debug_call_returned(&context));
                actual.step_instruction().unwrap();
                expected.step_instruction().unwrap();
                assert!(actual.debug_call_returned(&context));
                actual.finish_debug_call(context).unwrap();
                equal_execution(&actual, &expected);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn real_callee_changes_return_code_retains_ccr_and_exact_frontend_pcm() {
    let program = [
        0x13fc, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x4ef9, 0xff,
        0x2000,
    ];
    let function = [
        0x70ff, 0x2240, 0x33fc, 0x7005, 0xff, 0x2000, 0x33fc, 0x7206, 0xff, 0x2002, 0x70ff, 0x4e75,
        0x4e71,
    ];
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let mut actual = fixture(&program, 0x200, 0xff3000, region, timing, concurrent);
            actual.set_sample_rate(48_000);
            ram(&mut actual, 0xff2000, &[0x60fe, 0x4e71, 0x60fe, 0x4e71]);
            ram(&mut actual, 0xff1000, &function);
            for _ in 0..516 {
                actual.step_instruction().unwrap();
            }
            assert_eq!(actual.registers().pc, 0xff2000);
            actual
                .cpu
                .registers
                .set_status_register(SUPERVISOR | TRACE | 0x0300 | EXTEND);
            assert!(actual.set_interrupt_level(6));
            let saved_irq = *actual.cpu.interrupt_input();
            let checkpoint = actual.encode_frontend_state().unwrap();
            let mut untouched = actual.clone();
            let pcm_before = untouched.drain_audio_samples();
            assert!(!pcm_before.is_empty());
            assert!(pcm_before.iter().any(|sample| *sample != 0.0));
            let context = actual.begin_debug_call(0xff1000).unwrap();
            for _ in 0..6 {
                actual.step_instruction().unwrap();
            }
            assert!(actual.debug_call_returned(&context));
            assert_eq!(actual.queued_words(), Some([0x7005, 0x7206]));
            let completed_bus = actual.bus.clone();
            let completed_clock = actual.cpu_clock();
            actual.finish_debug_call(context).unwrap();
            assert_eq!(actual.cpu.registers.d[0], u32::MAX);
            assert_eq!(actual.cpu.registers.a[1], u32::MAX);
            assert_eq!(
                actual.cpu.registers.status_register(),
                SUPERVISOR | TRACE | 0x0300 | EXTEND | NEGATIVE
            );
            assert_eq!(*actual.cpu.interrupt_input(), saved_irq);
            assert_eq!(actual.bus, completed_bus);
            assert_eq!(actual.cpu_clock(), completed_clock);
            let mut prefix = actual.clone();
            assert!(prefix.drain_audio_samples().starts_with(&pcm_before));
            let saved = actual.encode_frontend_state().unwrap();
            let mut restored = actual.clone();
            restored.decode_frontend_state(&saved).unwrap();
            assert_eq!(restored.encode_frontend_state().unwrap(), saved);
            let mut rollback = actual.clone();
            rollback.decode_frontend_state(&checkpoint).unwrap();
            assert_eq!(rollback.encode_frontend_state().unwrap(), checkpoint);
            for machine in [&mut actual, &mut restored] {
                let sr = machine.cpu.registers.status_register();
                machine.cpu.registers.set_status_register(sr & !TRACE);
                assert!(machine.set_interrupt_level(0));
            }
            for _ in 0..256 {
                assert_eq!(actual.step_instruction(), restored.step_instruction());
            }
            assert_eq!(actual.drain_audio_samples(), restored.drain_audio_samples());
            assert_eq!(
                actual.encode_frontend_state().unwrap(),
                restored.encode_frontend_state().unwrap()
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn entry_and_callee_advance_active_sound_like_the_existing_clocked_bus() {
    for region in REGIONS {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let template = super::super::cpu_continuation_tests::machine(region, false);
            let mut actual = Emulator::new_with_timing(
                template.rom_bytes().to_vec(),
                LoadConfig {
                    region,
                    concurrent_sound: true,
                },
                timing,
            )
            .unwrap();
            let cycles = |machine: &Emulator| {
                machine
                    .bus
                    .owned_sound_cpu()
                    .map_or(machine.sound_cpu.cycles(), |cpu| cpu.cycles())
            };
            for _ in 0..20_000 {
                if cycles(&actual) >= 400 {
                    break;
                }
                actual.step_instruction().unwrap();
            }
            assert!(cycles(&actual) >= 400);
            for _ in 0..2000 {
                actual.step_instruction().unwrap();
            }
            ram(&mut actual, 0xff1000, &[0x7001, 0x4e75]);
            let mut control = actual.clone();
            let before = actual.cpu_clock();
            let context = actual.begin_debug_call(0xff1000).unwrap();
            actual.step_instruction().unwrap();
            actual.step_instruction().unwrap();
            actual.finish_debug_call(context).unwrap();
            let clocks = u32::try_from(actual.cpu_clock().0 - before.0).unwrap();
            if timing == TimingModel::Precise {
                control.bus.owned_context().unwrap().idle(before, clocks);
            } else {
                control.bus.audio.begin_batch();
                ScheduledBus::new(
                    &mut control.bus,
                    &mut control.sound_cpu,
                    &mut control.sound_schedule,
                )
                .idle(before, clocks);
                control.bus.audio.finish_batch();
            }
            assert!(!control.bus.clock_fault);
            assert_eq!(actual.bus.now, control.bus.now);
            assert_eq!(
                actual.bus.active_sound_memory(),
                control.bus.active_sound_memory()
            );
            assert_eq!(actual.bus.owned_sound_cpu(), control.bus.owned_sound_cpu());
            assert_eq!(
                actual.bus.active_sound_state(),
                control.bus.active_sound_state()
            );
            assert_eq!(actual.sound_cpu, control.sound_cpu);
            assert_eq!(actual.sound_schedule, control.sound_schedule);
            assert_eq!(actual.bus.audio, control.bus.audio);
            assert_eq!(actual.bus.video, control.bus.video);
            let pcm = actual.drain_audio_samples();
            assert!(!pcm.is_empty());
            assert!(pcm.iter().any(|sample| *sample != 0.0));
            assert_eq!(pcm, control.drain_audio_samples());
        }
    }
}
