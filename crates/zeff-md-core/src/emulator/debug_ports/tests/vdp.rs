use super::{cases::*, *};

fn configured(region: Region, timing: TimingModel, concurrent: bool, controls: &[u16]) -> Emulator {
    let mut program = Vec::new();
    for word in controls {
        program.extend([0x33fc, *word, 0x00c0, 4]);
    }
    program.extend([0x7003, 0x60fc, 0x4e71]);
    let mut emu = guest(region, timing, concurrent, &program);
    for _ in controls {
        assert!(matches!(emu.step_instruction().unwrap().cpu,
            TimedOutcome::Instruction(step) if step.opcode == 0x33fc));
    }
    emu
}

fn overlay_next(emu: &mut Emulator) -> [zeff_m68k::timing::InstructionInput; 2] {
    let inputs = emu.cpu.queued_instruction_inputs().unwrap();
    let pc = emu.registers().pc;
    assert_eq!(inputs[0].value, 0x7003);
    assert_eq!(
        inputs[0].origin.map(|byte| byte.storage_offset()),
        [Some(u64::from(pc)), Some(u64::from(pc + 1))]
    );
    emu.install_rom_patches(&[CheatPatch::WideRomWrite {
        address: pc + 1,
        value: CheatValue::Constant(9),
    }])
    .unwrap();
    assert_eq!(emu.peek(pc + 1), Some(9));
    inputs
}

fn consume_old_then_current(emu: &mut Emulator) {
    assert!(matches!(emu.step_instruction().unwrap().cpu,
        TimedOutcome::Instruction(step) if step.opcode == 0x7003));
    assert_eq!(emu.registers().d[0], 3);
    emu.step_instruction().unwrap();
    assert!(matches!(emu.step_instruction().unwrap().cpu,
        TimedOutcome::Instruction(step) if step.opcode == 0x7009));
    assert_eq!(emu.registers().d[0], 9);
}

fn witnessed_write(emu: &mut Emulator, write: DebugPortWrite) -> (CpuClocks, bool) {
    emu.check_debug_ports().unwrap();
    emu.validate_debug_port_checkpoint().unwrap();
    emu.invalidate_audio_trace(AudioTraceInvalidation::ExternalMutation);
    emu.bus.audio.trace_origin = None;
    emu.begin_call_stack_operation();
    let (clocks, witness) = emu
        .bus
        .owned_context()
        .unwrap()
        .write_with_fifo_witness(&mut emu.cpu, write.transfer().unwrap());
    assert!(!emu.bus.clock_fault);
    emu.main_stopped = emu.cpu.stopped();
    emu.finish_call_stack_operation(true);
    emu.finish_execution(None);
    emu.validate_debug_port_checkpoint().unwrap();
    (clocks, witness.full_wait.is_some())
}

pub(super) fn full_fifo() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let mut emu = configured(
                region,
                timing,
                concurrent,
                &[0x8f02, 0x8570, 0x8144, 0x4000, 0],
            );
            assert_eq!(emu.vdp_registers()[5], Some(0x70));
            let inputs = overlay_next(&mut emu);
            let mut wrote = 0_u16;
            let mut full_wait = false;
            let mut saw_full = false;
            let mut max_clocks = 0;
            let mut waits = 0;
            for index in 0..128_u16 {
                let full = if timing == TimingModel::Precise {
                    emu.vdp().owned_observation().unwrap().unwrap().status & 0x100 != 0
                } else {
                    emu.vdp().fifo().is_full()
                };
                let write = DebugPortWrite::Word {
                    address: 0xc00000,
                    value: 0xa100 | index,
                };
                let observed = (timing == TimingModel::Precise).then(|| {
                    let mut control = emu.clone();
                    let witness = witnessed_write(&mut control, write);
                    (control, witness)
                });
                let report = emu.debug_write_ports(&[write]).unwrap();
                let pressure = if let Some((control, (clocks, full_wait))) = observed {
                    assert_eq!(report.cpu_clocks, clocks);
                    same_checkpoint(&emu, &control);
                    output(&emu, &control);
                    full_wait
                } else {
                    full
                };
                wrote = index + 1;
                saw_full |= full;
                max_clocks = max_clocks.max(report.cpu_clocks.0);
                waits += u32::from(report.cpu_clocks.0 > 4);
                assert_eq!(report.instructions, 0);
                assert_eq!(emu.cpu.queued_instruction_inputs().unwrap(), inputs);
                if pressure && report.cpu_clocks.0 > 4 {
                    assert_eq!(report.elapsed.get(), report.cpu_clocks.0 * 7);
                    full_wait = true;
                    break;
                }
            }
            assert!(
                full_wait,
                "no full-FIFO wait: {region:?}/{timing:?}/concurrent={concurrent} \
                 writes={wrote} saw_full={saw_full} max_clocks={max_clocks} waits={waits}"
            );
            consume_old_then_current(&mut emu);
            let bytes = emu.encode_debugger_checkpoint().unwrap();
            let mut restored = emu.clone();
            restored.decode_debugger_checkpoint(&bytes).unwrap();
            for _ in 0..512 {
                paired_instruction(&mut emu, &mut restored);
            }
            for index in 0..wrote {
                assert_eq!(
                    &emu.vdp().memory().vram()[usize::from(index) * 2..][..2],
                    &[0xa1, index as u8]
                );
            }
            same_checkpoint(&emu, &restored);
            output(&emu, &restored);
        }
    }
}

fn fifo_full(emu: &Emulator, timing: TimingModel) -> bool {
    if timing == TimingModel::Precise {
        emu.vdp().owned_observation().unwrap().unwrap().status & 0x100 != 0
    } else {
        emu.vdp().fifo().is_full()
    }
}

fn dma_write(emu: &mut Emulator, write: DebugPortWrite) -> (DebugPortReport, bool) {
    if emu.timing_model() == TimingModel::Standard {
        return (emu.debug_write_ports(&[write]).unwrap(), false);
    }
    let mut control = emu.clone();
    control.check_debug_ports().unwrap();
    control.validate_debug_port_checkpoint().unwrap();
    control.invalidate_audio_trace(AudioTraceInvalidation::ExternalMutation);
    control.bus.audio.trace_origin = None;
    control.begin_call_stack_operation();
    let (clocks, witness) = control
        .bus
        .owned_context()
        .unwrap()
        .write_with_dma_witness(&mut control.cpu, write.transfer().unwrap());
    assert!(!control.bus.clock_fault);
    control.main_stopped = control.cpu.stopped();
    control.finish_call_stack_operation(true);
    control.finish_execution(None);
    control.validate_debug_port_checkpoint().unwrap();
    let report = emu.debug_write_ports(&[write]).unwrap();
    assert_eq!(report.cpu_clocks, clocks);
    same_checkpoint(emu, &control);
    output(emu, &control);
    (report, witness.dma_pressure.is_some())
}

fn dma_candidate(
    mut emu: Emulator,
    timing: TimingModel,
    misses: &mut [u32; 3],
) -> Option<Emulator> {
    let inputs = emu.cpu.queued_instruction_inputs().unwrap();
    assert_eq!(emu.vdp_registers()[23], Some(0));
    assert_eq!(emu.vdp().dma_activity_epoch(), 0);
    let setup = emu
        .debug_write_ports(&[control(0x9700), control(0x4400)])
        .unwrap();
    assert_eq!(setup.instructions, 0);
    assert_eq!(setup.writes, 2);
    assert_eq!(emu.cpu.queued_instruction_inputs().unwrap(), inputs);
    if timing == TimingModel::Standard {
        if !fifo_full(&emu, timing) {
            misses[0] += 1;
            return None;
        }
        assert!(fifo_full(&emu, timing));
    }
    assert_eq!(
        &emu.vdp_registers()[19..24],
        &[Some(0x40), Some(0), Some(0x80), Some(1), Some(0)]
    );
    assert_eq!(&emu.vdp().memory().vram()[0x47e..0x480], &[0, 0]);
    assert_eq!(emu.cpu.queued_instruction_inputs().unwrap(), inputs);
    let registers = emu.registers();
    let epoch = emu.vdp().dma_activity_epoch();
    let (trigger, mut pressure) = dma_write(&mut emu, control(0x0080));
    assert_eq!(trigger.instructions, 0);
    assert_eq!(trigger.writes, 1);
    assert_eq!(trigger.elapsed.get(), trigger.cpu_clocks.0 * 7);
    if timing == TimingModel::Standard {
        assert!(
            trigger.cpu_clocks.0 > 4,
            "DMA command did not charge its real fetch wait"
        );
    }
    let mut waited = trigger.cpu_clocks.0 > 4;
    let mut requested = false;
    for _ in 0..128 {
        if requested && emu.vdp_registers()[19..21] == [Some(0), Some(0)] {
            break;
        }
        let (report, observed) = dma_write(&mut emu, sound(0xa11100, 1));
        pressure |= observed;
        assert_eq!(report.instructions, 0);
        assert!(report.elapsed.get() > 0);
        assert_eq!(report.elapsed.get(), report.cpu_clocks.0 * 7);
        waited |= report.cpu_clocks.0 > 4;
        requested = true;
        assert_eq!(emu.cpu.queued_instruction_inputs().unwrap(), inputs);
    }
    assert!(
        waited,
        "DMA trigger/arbitration never charged actual wait clocks"
    );
    assert!(requested);
    assert_eq!(
        &emu.vdp_registers()[19..24],
        &[Some(0), Some(0), Some(0xc0), Some(1), Some(0)]
    );
    assert_ne!(emu.vdp().dma_activity_epoch(), epoch);
    assert_eq!(emu.registers(), registers);
    assert_eq!(emu.cpu.queued_instruction_inputs().unwrap(), inputs);
    assert!(emu.bus.active_sound_memory().requested);
    let nonempty = if timing == TimingModel::Precise {
        emu.vdp().owned_observation().unwrap().unwrap().status & 0x200 == 0
    } else {
        !emu.vdp().fifo().is_empty()
    };
    if !nonempty
        || emu.vdp().memory().vram()[0x47e..0x480] != [0, 0]
        || timing == TimingModel::Precise && !pressure
    {
        misses[1] += u32::from(timing == TimingModel::Precise && !pressure);
        misses[2] += u32::from(!nonempty || emu.vdp().memory().vram()[0x47e..0x480] != [0, 0]);
        return None;
    }
    assert!(timing == TimingModel::Standard || pressure);
    assert_eq!(&emu.vdp().memory().vram()[0x47e..0x480], &[0, 0]);
    if timing == TimingModel::Precise {
        assert_eq!(
            emu.vdp().owned_observation().unwrap().unwrap().status & 0x200,
            0
        );
    } else {
        assert!(!emu.vdp().fifo().is_empty());
    }
    Some(emu)
}

pub(super) fn active_dma() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let controls = [0x8154, 0x8f02, 0x9340, 0x9400, 0x9580, 0x9601, 0x4600, 0];
            let mut program = Vec::new();
            for word in controls {
                program.extend([0x33fc, word, 0x00c0, 4]);
            }
            program.extend([0x7003, 0x60fc, 0x4e71]);
            let template = guest(region, timing, concurrent, &program);
            let mut rom = template.rom_bytes().to_vec();
            for index in 0..64_u16 {
                words(&mut rom, 0x300 + usize::from(index) * 2, &[0xa500 | index]);
            }
            let mut emu = Emulator::new_with_timing(
                rom,
                LoadConfig {
                    region,
                    concurrent_sound: concurrent,
                },
                timing,
            )
            .unwrap();
            for (index, word) in controls.into_iter().enumerate() {
                let pc = emu.registers().pc;
                let clock = emu.cpu_clock();
                emu.step_instruction().unwrap_or_else(|fault| {
                    panic!(
                        "DMA setup {region:?}/{timing:?}/{concurrent} index={index} \
                        word={word:04x} pc={pc:08x} clock={clock:?}: {fault:?}"
                    )
                });
            }
            let inputs = overlay_next(&mut emu);
            let pc = emu.registers().pc;
            emu.install_rom_patches(&[
                CheatPatch::WideRomWrite {
                    address: pc + 1,
                    value: CheatValue::Constant(9),
                },
                CheatPatch::WideRomWrite {
                    address: 0x37f,
                    value: CheatValue::Constant(0xcc),
                },
            ])
            .unwrap();
            assert_eq!(&emu.rom_bytes()[0x37e..0x380], &[0xa5, 0x3f]);
            assert_eq!(emu.peek(0x37f), Some(0xcc));
            assert_eq!(emu.vdp_registers()[5], Some(0));
            let mut misses = [0; 3];
            let mut prepared = None;
            for index in 0..128_u16 {
                let report = emu
                    .debug_write_ports(&[DebugPortWrite::Word {
                        address: 0xc00000,
                        value: 0xa100 | index,
                    }])
                    .unwrap();
                assert_eq!(report.instructions, 0);
                assert_eq!(emu.cpu.queued_instruction_inputs().unwrap(), inputs);
                if let Some(candidate) = dma_candidate(emu.clone(), timing, &mut misses) {
                    prepared = Some(candidate);
                    break;
                }
            }
            let mut emu = prepared.unwrap_or_else(|| {
                panic!(
                    "no real DMA full-pressure and unretired-tail endpoint within128 \
                    writes: {region:?}/{timing:?}/{concurrent} misses={misses:?}"
                )
            });
            emu.install_rom_patches(&[
                CheatPatch::WideRomWrite {
                    address: pc + 1,
                    value: CheatValue::Constant(9),
                },
                CheatPatch::WideRomWrite {
                    address: 0x37f,
                    value: CheatValue::Constant(0xdd),
                },
            ])
            .unwrap();
            assert_eq!(emu.peek(0x37f), Some(0xdd));
            assert_eq!(&emu.rom_bytes()[0x37e..0x380], &[0xa5, 0x3f]);
            assert_eq!(emu.cpu.queued_instruction_inputs().unwrap(), inputs);
            let bytes = emu.encode_debugger_checkpoint().unwrap();
            let mut restored = emu.clone();
            restored.decode_debugger_checkpoint(&bytes).unwrap();
            assert_eq!(restored.peek(0x37f), Some(0xdd));
            consume_old_then_current(&mut emu);
            consume_old_then_current(&mut restored);
            for _ in 0..128 {
                paired_instruction(&mut emu, &mut restored);
            }
            assert_eq!(&emu.vdp().memory().vram()[0x400..0x402], &[0xa5, 0]);
            assert_eq!(&emu.vdp().memory().vram()[0x47e..0x480], &[0xa5, 0xcc]);
            same_checkpoint(&emu, &restored);
            output(&emu, &restored);
        }
    }
}
