use super::*;

pub(super) fn guest(
    region: Region,
    timing: TimingModel,
    concurrent_sound: bool,
    program: &[u16],
) -> Emulator {
    let template = machine(region, timing, concurrent_sound);
    let mut rom = template.rom_bytes().to_vec();
    words(&mut rom, 0x200, program);
    for vector in [9, 28] {
        words(&mut rom, vector * 4, &[0, 0x600]);
    }
    words(&mut rom, 0x600, &[0x4e73, 0x4e71]);
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

pub(super) fn words(rom: &mut [u8], address: usize, values: &[u16]) {
    for (index, value) in values.iter().enumerate() {
        rom[address + index * 2..address + index * 2 + 2].copy_from_slice(&value.to_be_bytes());
    }
}

pub(super) fn sound(address: u32, value: u8) -> DebugPortWrite {
    DebugPortWrite::Byte { address, value }
}

pub(super) fn direct(emu: &mut Emulator, write: DebugPortWrite) -> Result<CpuClocks, MachineFault> {
    let mut operation = Write(
        write
            .transfer()
            .or_else(|| emu.save_port_transfer(write))
            .unwrap(),
    );
    if emu.bus.owned.is_some() {
        emu.step_owned_operation(&mut operation)
            .map(|step| step.output)
    } else {
        emu.step_main_operation_with(&mut operation, &mut crate::sound_trace::NoSoundCapture)
            .map(|step| step.output)
    }
}

pub(super) fn instruction(emu: &mut Emulator) -> MachineStep {
    let before = emu.bus.now;
    let step = emu
        .step_instruction()
        .expect("real guest instruction failed");
    assert!(
        emu.bus.now > before,
        "successful instruction made no physical progress"
    );
    step
}

pub(super) fn paired_instruction(actual: &mut Emulator, expected: &mut Emulator) {
    let a = instruction(actual);
    let b = instruction(expected);
    assert_eq!(a, b);
}

pub(super) fn pcm(emu: &Emulator) -> Vec<u32> {
    emu.clone()
        .drain_audio_samples()
        .into_iter()
        .map(f32::to_bits)
        .collect()
}

pub(super) fn public_formats(emu: &Emulator) {
    assert_eq!(&emu.encode_state().unwrap()[..12], b"ZMDRAW01\x01\0\0\0");
    assert_eq!(
        &emu.encode_frontend_state().unwrap()[..12],
        b"ZMDFRONT\x01\0\0\0"
    );
}

pub(super) fn output(actual: &Emulator, expected: &Emulator) {
    public_formats(actual);
    public_formats(expected);
    assert_eq!(actual.frame_ready(), expected.frame_ready());
    assert_eq!(actual.frame_count(), expected.frame_count());
    assert_eq!(
        actual.framebuffer_dimensions(),
        expected.framebuffer_dimensions()
    );
    assert_eq!(actual.framebuffer(), expected.framebuffer());
    assert_eq!(actual.video_error(), expected.video_error());
    assert_eq!(pcm(actual), pcm(expected));
}

pub(super) fn trace_and_hint() {
    trace_and_hint_for(control(0x8f02));
}

pub(super) fn trace_and_hint_for(write: DebugPortWrite) {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let template = guest(
                region,
                timing,
                concurrent,
                &[0x46fc, 0xa000, 0x4e71, 0x60fe],
            );
            let mut rom = template.rom_bytes().to_vec();
            words(&mut rom, 0x600, &[0x0257, 0x7fff, 0x4e73, 0x4e71]);
            let mut trace = Emulator::new_with_timing(
                rom,
                LoadConfig {
                    region,
                    concurrent_sound: concurrent,
                },
                timing,
            )
            .unwrap();
            trace.set_call_stack_recording(true);
            assert!(matches!(trace.step_instruction().unwrap().cpu,
                TimedOutcome::Instruction(step) if step.opcode == 0x46fc));
            assert_eq!(trace.registers().status_register(), 0xa000);
            assert_eq!(trace.registers().pc, 0x204);
            assert!(!trace.cpu.trace_pending());
            let inputs = trace.cpu.queued_instruction_inputs();
            let interrupt = *trace.cpu.interrupt_input();
            trace.debug_write_ports(&[write]).unwrap();
            assert_eq!(trace.cpu.queued_instruction_inputs(), inputs);
            assert_eq!(*trace.cpu.interrupt_input(), interrupt);
            assert_eq!(trace.registers().status_register(), 0xa000);
            assert!(!trace.cpu.trace_pending());
            assert!(matches!(trace.step_instruction().unwrap().cpu,
                TimedOutcome::Instruction(step) if step.opcode == 0x4e71 && step.exception == Some(9)));
            assert!(!trace.cpu.trace_pending());
            assert_eq!(trace.registers().pc, 0x600);
            assert_eq!(trace.registers().stack_pointer(), 0xff2ffa);
            for (index, byte) in [0xa0, 0, 0, 0, 2, 6].into_iter().enumerate() {
                assert_eq!(trace.peek(0xff2ffa + index as u32), Some(byte));
            }
            assert_eq!(trace.call_stack().count(), 1);
            trace.step_instruction().unwrap();
            trace.step_instruction().unwrap();
            assert_eq!(trace.registers().pc, 0x206);
            assert_eq!(trace.registers().status_register(), 0x2000);
            assert_eq!(trace.registers().stack_pointer(), 0xff3000);
            assert_eq!(trace.call_stack().count(), 0);
            public_formats(&trace);

            let mut hint = guest(
                region,
                timing,
                concurrent,
                &[
                    0x46fc, 0x2000, 0x33fc, 0x8104, 0x00c0, 4, 0x33fc, 0x8a00, 0x00c0, 4, 0x33fc,
                    0x8f02, 0x00c0, 4, 0x33fc, 0x8010, 0x00c0, 4, 0x4ef9, 0, 0x21c, 0x4e71,
                ],
            );
            hint.set_call_stack_recording(true);
            let deadline = hint.bus.now.get() + 8 * 3420;
            let mut reached = false;
            for _ in 0..16_384 {
                if hint.bus.now.get() >= deadline {
                    break;
                }
                if hint.vdp().interrupt_level() == 4 && matches!(hint.registers().pc, 0x21c | 0x224)
                {
                    let mut next = hint.clone();
                    if matches!(
                        next.step_instruction().unwrap().cpu,
                        TimedOutcome::Interrupt {
                            level: 4,
                            vector: 28,
                            ..
                        }
                    ) {
                        reached = true;
                        break;
                    }
                }
                let pc = hint.registers().pc;
                let sr = hint.registers().status_register();
                let level = hint.interrupt_level();
                let interrupt = *hint.cpu.interrupt_input();
                let step = instruction(&mut hint);
                assert!(
                    matches!(step.cpu, TimedOutcome::Instruction(_)),
                    "HINT setup {region:?}/{timing:?} concurrent={concurrent} pc={pc:06x} sr={sr:04x} irq={level} input={interrupt:?}: {:?}",
                    step.cpu
                );
            }
            assert!(reached, "no actual HINT4 boundary within eight scanlines");
            assert_eq!(hint.interrupt_level(), 4);
            assert_eq!(hint.registers().status_register(), 0x2008);
            let mut boundary = zeff_emu_common::save_state::StateWriter::new();
            hint.cpu.write_boundary_state(&mut boundary).unwrap();
            let boundary_tag = boundary.into_bytes()[0];
            assert!(matches!(boundary_tag, 1 | 2));
            let interrupt = *hint.cpu.interrupt_input();
            let registers = hint.registers();
            let return_low = match registers.pc {
                0x21c => 0x1c,
                0x224 => 0x24,
                pc => panic!("HINT interrupted outside its two literal guest boundaries: {pc:x}"),
            };
            let inputs = hint.cpu.queued_instruction_inputs();
            hint.debug_write_ports(&[write]).unwrap();
            let mut after_boundary = zeff_emu_common::save_state::StateWriter::new();
            hint.cpu.write_boundary_state(&mut after_boundary).unwrap();
            assert_eq!(after_boundary.into_bytes()[0], boundary_tag);
            assert_eq!(*hint.cpu.interrupt_input(), interrupt);
            assert_eq!(hint.registers(), registers);
            assert_eq!(hint.cpu.queued_instruction_inputs(), inputs);
            assert!(matches!(
                hint.step_instruction().unwrap().cpu,
                TimedOutcome::Interrupt {
                    level: 4,
                    vector: 28,
                    ..
                }
            ));
            assert_eq!(hint.registers().pc, 0x600);
            for (index, byte) in [0x20, 8, 0, 0, 2, return_low].into_iter().enumerate() {
                assert_eq!(hint.peek(0xff2ffa + index as u32), Some(byte));
            }
            hint.step_instruction().unwrap();
            assert_eq!(hint.registers().pc, registers.pc);
            assert_eq!(hint.registers().stack_pointer(), 0xff3000);
            assert_eq!(hint.call_stack().count(), 0);
            public_formats(&hint);
        }
    }
}

pub(super) fn precise_failure() {
    for region in REGIONS {
        let mut emu = guest(
            region,
            TimingModel::Precise,
            true,
            &[0x6104, 0x60fe, 0x4e71, 0x7003, 0x4e75],
        );
        emu.set_call_stack_recording(true);
        emu.step_instruction().unwrap();
        assert_eq!(emu.registers().pc, 0x206);
        assert_eq!(emu.call_stack().count(), 1);
        let before = emu.clone();
        let mut writes = vec![sound(0xa11100, 1), control(0x8c02)];
        writes.extend([control(0x8f02); 64]);
        let mut direct = before.clone();
        super::cases::direct(&mut direct, writes[0]).unwrap();
        assert!(direct.bus.active_sound_memory().requested);
        assert!(direct.cpu_clock() > before.cpu_clock());
        let mut failure = None;
        for (index, write) in writes.iter().copied().enumerate().skip(1) {
            if let Err(fault) = super::cases::direct(&mut direct, write) {
                assert!(
                    format!("{fault:?}").contains("unsupported VDP sync/interlace/address profile")
                );
                failure = Some((index, fault));
                break;
            }
        }
        let (index, fault) = failure.expect("real interlace write never reached its owner refusal");
        assert!(index >= 1);
        assert!(direct.bus.now > before.bus.now);
        assert_eq!(
            emu.debug_write_ports(&writes),
            Err(DebugPortError::Execution { index, fault })
        );
        same_checkpoint(&emu, &before);
        assert_eq!(emu.call_stack().count(), 1);
        assert_eq!(emu.bus.audio, before.bus.audio);
        public_formats(&emu);
    }
}
