use super::*;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_m68k::TRACE;

mod cases;
mod output;
mod psg;
mod save;
mod sound_aperture;
mod uart;
mod vdp;

const PROFILES: [(TimingModel, bool); 3] = [
    (TimingModel::Standard, false),
    (TimingModel::Standard, true),
    (TimingModel::Precise, true),
];
const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

fn machine(region: Region, timing: TimingModel, concurrent_sound: bool) -> Emulator {
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 0x30, 0, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x200..0x208].copy_from_slice(&[0x70, 3, 0x60, 0xfc, 0x4e, 0x71, 0x4e, 0x71]);
    Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
        timing,
    )
    .unwrap()
}

fn control(value: u16) -> DebugPortWrite {
    DebugPortWrite::Word {
        address: 0xc00004,
        value,
    }
}

fn same_checkpoint(actual: &Emulator, expected: &Emulator) {
    assert_eq!(actual, expected);
    assert_eq!(
        actual.cpu.queued_instruction_inputs(),
        expected.cpu.queued_instruction_inputs()
    );
    assert_eq!(
        actual.encode_debugger_checkpoint().unwrap(),
        expected.encode_debugger_checkpoint().unwrap()
    );
    assert_eq!(actual.call_stack_status(), expected.call_stack_status());
    assert_eq!(
        actual.call_stack().collect::<Vec<_>>(),
        expected.call_stack().collect::<Vec<_>>()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn literal_vdp_and_sound_ports_keep_cpu_queue_and_restore_owned_continuation() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            for recording in [false, true] {
                let mut emu = machine(region, timing, concurrent);
                emu.set_call_stack_recording(recording);
                let registers = emu.registers();
                let inputs = emu.cpu.queued_instruction_inputs();
                let clock = emu.cpu_clock();
                let tick = emu.bus.now;
                let status = emu.call_stack_status();
                let report = emu
                    .debug_write_ports(&[
                        control(0x8f02),
                        control(0x4000),
                        control(0),
                        DebugPortWrite::Word {
                            address: 0xc00000,
                            value: 0x1234,
                        },
                        DebugPortWrite::Byte {
                            address: 0xa11100,
                            value: 1,
                        },
                        DebugPortWrite::Byte {
                            address: 0xa11200,
                            value: 1,
                        },
                    ])
                    .unwrap();
                assert_eq!(report.instructions, 0);
                assert_eq!(report.writes, 6);
                assert!(report.cpu_clocks.0 >= 24);
                assert_eq!(report.cpu_clocks.0, emu.cpu_clock().0 - clock.0);
                assert_eq!(
                    Some(report.elapsed),
                    emu.bus.now.checked_duration_since(tick)
                );
                assert_eq!(report.elapsed.get(), report.cpu_clocks.0 * 7);
                assert_eq!(emu.registers(), registers);
                assert_eq!(emu.cpu.queued_instruction_inputs(), inputs);
                assert_eq!(emu.call_stack_status(), status);
                assert_eq!(emu.call_stack().count(), 0);
                assert_eq!(emu.vdp_registers()[15], Some(2));
                let bytes = emu.encode_debugger_checkpoint().unwrap();
                let mut restored = emu.clone();
                restored.decode_debugger_checkpoint(&bytes).unwrap();
                for _ in 0..128 {
                    assert_eq!(emu.step_instruction(), restored.step_instruction());
                }
                same_checkpoint(&emu, &restored);
                assert_eq!(&emu.vdp().memory().vram()[..2], &[0x12, 0x34]);
                assert!(emu.bus.active_sound_memory().requested);
                assert!(!emu.bus.active_sound_memory().reset_asserted);
                assert_eq!(emu.drain_audio_samples(), restored.drain_audio_samples());
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn excluded_port_widths_aliases_and_empty_list_are_exactly_inert() {
    let excluded = [
        DebugPortWrite::Byte {
            address: 0xc00000,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xc00004,
            value: 1,
        },
        DebugPortWrite::Word {
            address: 0xc00001,
            value: 1,
        },
        DebugPortWrite::Word {
            address: 0xc00002,
            value: 1,
        },
        DebugPortWrite::Word {
            address: 0xc00006,
            value: 1,
        },
        DebugPortWrite::Word {
            address: 0x1c00004,
            value: 1,
        },
        DebugPortWrite::Word {
            address: 0xa11100,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xa11101,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xa11201,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xa02000,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xa04004,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xa06001,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xc00013,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xa14000,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xa130f1,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xa10009,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xff0000,
            value: 1,
        },
    ];
    for (timing, concurrent) in PROFILES {
        let mut emu = machine(Region::NtscUs, timing, concurrent);
        emu.set_call_stack_recording(true);
        if timing == TimingModel::Standard {
            emu.reset_and_begin_audio_trace(100).unwrap();
        }
        let before = emu.clone();
        for write in excluded {
            assert_eq!(
                emu.debug_write_ports(&[control(0x8f02), write]),
                Err(DebugPortError::UnsupportedPort { index: 1 })
            );
            same_checkpoint(&emu, &before);
        }
        assert_eq!(emu.debug_write_ports(&[]).unwrap().cpu_clocks, CpuClocks(0));
        same_checkpoint(&emu, &before);
        emu.bus.audio.begin_batch();
        let active = emu.clone();
        assert_eq!(
            emu.debug_write_ports(&[control(0x8f02)]),
            Err(DebugPortError::Rejected("audio batch is active"))
        );
        assert_eq!(emu, active);
        emu.bus.audio.finish_batch();
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn current_overlay_does_not_replace_queued_old_opcode_during_timed_write() {
    vdp::full_fifo();
    vdp::active_dma();
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let mut emu = machine(region, timing, concurrent);
            let original_inputs = emu.cpu.queued_instruction_inputs().unwrap();
            assert_eq!(original_inputs[0].value, 0x7003);
            assert_eq!(original_inputs[0].origin[0].storage_offset(), Some(0x200));
            emu.install_rom_patches(&[CheatPatch::WideRomWrite {
                address: 0x201,
                value: CheatValue::Constant(9),
            }])
            .unwrap();
            assert_eq!(emu.peek(0x201), Some(9));
            emu.debug_write_ports(&[control(0x8f02)]).unwrap();
            assert_eq!(
                emu.cpu.queued_instruction_inputs().unwrap(),
                original_inputs
            );
            let step = emu.step_instruction().unwrap();
            assert!(matches!(step.cpu, TimedOutcome::Instruction(step) if step.opcode == 0x7003));
            assert_eq!(emu.registers().d[0], 3);
            emu.step_instruction().unwrap();
            emu.step_instruction().unwrap();
            assert_eq!(emu.registers().d[0], 9);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sr_trace_and_pending_interrupt_input_are_not_sampled_or_masked_by_ports() {
    cases::trace_and_hint();
    for (timing, concurrent) in PROFILES {
        let mut emu = machine(Region::NtscUs, timing, concurrent);
        emu.cpu.registers.set_status_register(0x2000 | TRACE);
        assert!(emu.cpu.interrupt_input().set_level(6));
        assert!(emu.set_interrupt_level(7));
        let registers = emu.registers();
        let interrupt = *emu.cpu.interrupt_input();
        emu.debug_write_ports(&[control(0x8f02)]).unwrap();
        assert_eq!(emu.registers(), registers);
        assert_eq!(*emu.cpu.interrupt_input(), interrupt);
        assert!(!emu.cpu.trace_pending());
        assert_eq!(emu.interrupt_level(), 7);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn standard_real_device_failure_discards_prefix_clocks_audio_and_tracker_changes() {
    cases::precise_failure();
    for region in REGIONS {
        for concurrent in [false, true] {
            let mut emu = machine(region, TimingModel::Standard, concurrent);
            emu.set_call_stack_recording(true);
            emu.reset_and_begin_audio_trace(100).unwrap();
            let before = emu.clone();
            let writes = [
                DebugPortWrite::Byte {
                    address: 0xa11100,
                    value: 1,
                },
                control(0x8c80),
            ];
            let mut prefix = before.clone();
            prefix
                .step_main_operation_with(
                    &mut Write(DebugDataWrite::Byte {
                        address: 0xa11100,
                        value: 1,
                    }),
                    &mut crate::sound_trace::NoSoundCapture,
                )
                .unwrap();
            assert!(prefix.bus.sound.requested);
            assert!(prefix.cpu_clock() > before.cpu_clock());
            let fault = prefix.step_main_operation_with(
                &mut Write(DebugDataWrite::Word {
                    address: 0xc00004,
                    value: 0x8c80,
                }),
                &mut crate::sound_trace::NoSoundCapture,
            );
            assert!(fault.is_err());
            assert!(prefix.cpu_clock() > before.cpu_clock());
            assert!(matches!(
                emu.debug_write_ports(&writes),
                Err(DebugPortError::Execution { index: 1, .. })
            ));
            same_checkpoint(&emu, &before);
            assert_eq!(emu.bus.audio, before.bus.audio);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn active_sound_pending_work_and_nonzero_pcm_survive_timed_ports_and_restore() {
    output::active_sound_held();
    output::precise_sound_phase();
    output::frame_crossing();
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn timed_ports_keep_real_observed_call_frames_and_disabled_tracker() {
    for (timing, concurrent_sound) in PROFILES {
        for recording in [false, true] {
            let template = machine(Region::NtscUs, timing, concurrent_sound);
            let mut rom = template.rom_bytes().to_vec();
            rom[0x200..0x20a]
                .copy_from_slice(&[0x61, 4, 0x60, 0xfe, 0x4e, 0x71, 0x70, 3, 0x4e, 0x75]);
            let mut emu = Emulator::new_with_timing(
                rom,
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound,
                },
                timing,
            )
            .unwrap();
            emu.set_call_stack_recording(recording);
            emu.step_instruction().unwrap();
            assert_eq!(emu.registers().pc, 0x206);
            let status = emu.call_stack_status();
            let frames = emu.call_stack().copied().collect::<Vec<_>>();
            assert_eq!(frames.len(), usize::from(recording));
            emu.debug_write_ports(&[control(0x8f02)]).unwrap();
            assert_eq!(emu.call_stack_status(), status);
            assert_eq!(emu.call_stack().copied().collect::<Vec<_>>(), frames);
            assert_eq!(emu.cpu.control_flow_recording(), recording);
            assert_eq!(
                emu.cpu.clone().drain_control_flow_events().events().count(),
                0
            );
            emu.step_instruction().unwrap();
            emu.step_instruction().unwrap();
            assert_eq!(emu.registers().pc, 0x202);
            assert_eq!(emu.call_stack().count(), 0);
        }
    }
}
