use super::*;
use crate::hardware::vdp::Vdp;
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn hardware_irq_is_sampled_into_status_replay_without_changing_manual_line() {
    relative_jump_boundaries();
    relative_jump_replays();
    let mut rom = vec![0; 16];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
    let mut cartridge = Cartridge::new(rom).unwrap();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..4].copy_from_slice(&[0x3a, 0x00, 0x40, 0x76]);
    let vdp = Vdp::bootstrap_mode5();
    let irq = vdp
        .interrupts()
        .sound_interrupt(vdp.registers(), crate::Region::NtscUs);
    let at = 224 * crate::constants::VDP_LINE_TICKS + 40;
    let mut audio = Audio::default();
    audio.advance_to(at);
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut interval = SoundInterval::new(at..at + 151, &mut audio);
    interval.sound_irq = Some(&irq);
    schedule.advance(&mut cpu, &mut memory, &mut cartridge, &mut [], interval);
    let pending = schedule.pending.as_ref().unwrap();
    assert!(pending.memory.irq_asserted);
    assert!(pending.replay.unwrap().irq_asserted);
    assert!(!memory.irq_asserted);
    let mut writer = StateWriter::new();
    schedule.write_state(&cpu, &mut writer).unwrap();
    let bytes = writer.into_bytes();
    let mut restored = SoundSchedule::default();
    let mut restored_cpu = Cpu::new();
    restored
        .read_state(&mut restored_cpu, &mut StateReader::new(&bytes))
        .unwrap();
    assert_eq!(restored, schedule);
    assert_eq!(restored_cpu, cpu);
    let mut pending = schedule.pending.take().unwrap();
    pending
        .refresh_reads(&cpu, &memory, &cartridge, &audio)
        .unwrap();
    assert!(pending.memory.irq_asserted);
    assert!(!memory.irq_asserted);
}

fn relative_jump_boundaries() {
    use zeff_z80::ResetState;
    let mut rom = vec![0; 16];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
    let mut cartridge = Cartridge::new(rom).unwrap();
    for opcode in [0x10, 0x18, 0x20, 0x28, 0x30, 0x38] {
        for zero in [false, true] {
            for carry in [false, true] {
                for prefixes in [0, 1, 3] {
                    let mut memory = SoundMemory::default();
                    memory.set_reset(false);
                    let mut program = vec![0x3e, u8::from(!zero), 0xb7];
                    if carry {
                        program.push(0x37);
                    }
                    if opcode == 0x10 {
                        program.extend([0x06, u8::from(zero)]);
                    }
                    program.extend([0xdd, 0xfd, 0xdd][..prefixes].iter());
                    let terminal = program.len() as u16;
                    program.extend([opcode, 1, 0x76, 0x76]);
                    memory.ram[..program.len()].copy_from_slice(&program);
                    let mut cpu = Cpu::new();
                    for _ in 0..2 + usize::from(carry) + usize::from(opcode == 0x10) {
                        cpu.step_with_bus(&mut CandidateBus::new(&mut memory, &cartridge))
                            .unwrap();
                    }
                    let mut oracle = cpu.clone();
                    let mut oracle_memory = memory.clone();
                    oracle_memory.ram[usize::from(terminal) + 1] = 0xfe;
                    oracle
                        .step_with_bus(&mut CandidateBus::new(&mut oracle_memory, &cartridge))
                        .unwrap();
                    let mut state = BusState::default();
                    for _ in 0..prefixes {
                        assert_eq!(
                            cpu.step_bus_segment(
                                &mut CandidateBus::new(&mut memory, &cartridge),
                                &mut state
                            )
                            .unwrap()
                            .instruction
                            .cycles,
                            4
                        );
                    }
                    let flags = cpu.regs().f;
                    let refresh = cpu.regs().r;
                    let b = cpu.regs().b;
                    let mut bus = CandidateBus::new(&mut memory, &cartridge);
                    let step = cpu.step_bus_segment(&mut bus, &mut state).unwrap();
                    assert_eq!(
                        (step.instruction.cycles, step.complete, bus.accesses.get()),
                        (if opcode == 0x10 { 5 } else { 4 }, false, 1)
                    );
                    assert_eq!(
                        (cpu.regs().pc, cpu.regs().f, cpu.regs().b),
                        (terminal + 1, flags, b)
                    );
                    assert_eq!(
                        cpu.regs().r,
                        (refresh & 0x80) | (refresh.wrapping_add(1) & 0x7f)
                    );
                    memory.ram[usize::from(terminal) + 1] = 0xfe;
                    let mut writer = StateWriter::new();
                    cpu.write_continuation(&state, &mut writer);
                    let mut restored = Cpu::new_with_reset(ResetState::new(7, 9));
                    let mut restored_state = BusState::default();
                    restored
                        .read_continuation(
                            &mut restored_state,
                            &mut StateReader::new(&writer.into_bytes()),
                        )
                        .unwrap();
                    let taken = match opcode {
                        0x10 => !zero,
                        0x18 => true,
                        0x20 => !zero,
                        0x28 => zero,
                        0x30 => !carry,
                        _ => carry,
                    };
                    let mut other_memory = memory.clone();
                    for (cpu, state, memory) in [
                        (&mut cpu, &mut state, &mut memory),
                        (&mut restored, &mut restored_state, &mut other_memory),
                    ] {
                        let refresh = cpu.regs().r;
                        let mut bus = CandidateBus::new(memory, &cartridge);
                        let tail = cpu.step_bus_segment(&mut bus, state).unwrap();
                        assert_eq!(
                            (tail.instruction.cycles, tail.complete, bus.accesses.get()),
                            (if taken { 8 } else { 3 }, true, 1)
                        );
                        assert!(!state.pending());
                        assert_eq!(cpu.regs().r, refresh);
                        assert_eq!(*cpu, oracle);
                    }
                    assert_eq!(memory, other_memory);
                    let mut reset = restored;
                    reset.reset();
                    assert_eq!(reset, Cpu::new());
                }
            }
        }
    }
    for (opcode, use_epoch) in [(0x18, false), (0x18, true), (0x10, false), (0x10, true)] {
        let mut memory = SoundMemory::default();
        memory.set_reset(false);
        memory.ram[..4].copy_from_slice(&[0xfb, opcode, 1, 0x76]);
        let mut cpu = Cpu::new();
        cpu.step_with_bus(&mut CandidateBus::new(&mut memory, &cartridge))
            .unwrap();
        let m1_end = if opcode == 0x10 { 135 } else { 120 };
        let mut schedule = SoundSchedule::default();
        if use_epoch {
            schedule.begin_epoch_field();
        }
        let mut audio = Audio::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(60..61, &mut audio),
        );
        if use_epoch {
            assert_eq!(schedule.epoch_deadline(), Some(m1_end));
            schedule.materialize_epoch(&mut cpu, &memory, &mut crate::sound_trace::NoSoundCapture);
        }
        assert_eq!(schedule.pending.as_ref().unwrap().deadline, m1_end);
        memory.request(true);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(61..m1_end - 1, &mut audio),
        );
        assert!(!memory.granted);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(m1_end - 1..m1_end, &mut audio),
        );
        assert!(memory.granted && schedule.state.transfer_pending());
        assert_eq!(cpu.regs().pc, 2);
        assert_eq!(cpu.regs().b, 0);
        memory.ram[1] = 0x76;
        memory.ram[2] = 0xfe;
        memory.irq_asserted = true;
        memory.request(false);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(m1_end..m1_end + 120, &mut audio),
        );
        if use_epoch {
            schedule.finish_epoch_field(&mut cpu, &memory, &mut crate::sound_trace::NoSoundCapture);
        }
        assert_eq!(
            (cpu.regs().pc, cpu.cycles()),
            (1, if opcode == 0x10 { 17 } else { 16 })
        );
        assert_eq!(cpu.regs().b, if opcode == 0x10 { 0xff } else { 0 });
        assert!(cpu.interrupts_enabled());
        assert!(!cpu.last_step_was_interrupt());
        assert!(schedule.pending.is_none());
        assert_eq!(schedule.fault, None);
    }
}

fn relative_jump_replays() {
    use zeff_z80::ResetState;
    let mut rom = vec![0; 16];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
    let mut cartridge = Cartridge::new(rom).unwrap();
    for (opcode, taken) in [(0x18, true), (0x28, false), (0x10, false), (0x10, true)] {
        for prefixes in [0, 2] {
            let djnz = opcode == 0x10;
            let start = 0x3fff - prefixes - if djnz { 2 } else { 0 };
            let mut cpu = Cpu::new_with_reset(ResetState::new(start, 0));
            let mut memory = SoundMemory::default();
            memory.set_reset(false);
            let mut program = Vec::new();
            if djnz {
                program.extend([0x06, if taken { 2 } else { 1 }]);
            }
            program.extend([0xdd, 0xfd][..usize::from(prefixes)].iter());
            program.push(opcode);
            let offset = usize::from(start) & SOUND_RAM_MASK;
            memory.ram[offset..offset + program.len()].copy_from_slice(&program);
            if djnz {
                cpu.step_with_bus(&mut CandidateBus::new(&mut memory, &cartridge))
                    .unwrap();
            }
            let mut state = BusState::default();
            for _ in 0..prefixes {
                cpu.step_bus_segment(&mut CandidateBus::new(&mut memory, &cartridge), &mut state)
                    .unwrap();
            }
            assert_eq!(cpu.regs().pc, 0x3fff);
            let pending = PendingSound::prepare_with(
                &cpu,
                &memory,
                &cartridge,
                Preparation {
                    now: cpu.cycles() * SOUND_CPU_DIVISOR,
                    state,
                    bounded: true,
                    opn2_status: 0,
                },
                false,
                Some(if djnz {
                    RelativeReplay::Djnz
                } else {
                    RelativeReplay::Jr
                }),
            )
            .unwrap();
            let schedule = SoundSchedule {
                state,
                pending: Some(pending),
                ..SoundSchedule::default()
            };
            let mut writer = StateWriter::new();
            schedule.write_state(&cpu, &mut writer).unwrap();
            let bytes = writer.into_bytes();
            let mut restored_cpu = Cpu::new();
            let mut schedule = SoundSchedule::default();
            let mut reader = StateReader::new(&bytes);
            schedule.read_state(&mut restored_cpu, &mut reader).unwrap();
            assert!(reader.is_exhausted());
            assert_eq!(restored_cpu, cpu);
            let mut writer = StateWriter::new();
            schedule.write_state(&cpu, &mut writer).unwrap();
            assert_eq!(writer.into_bytes(), bytes);
            let original = schedule.pending.take().unwrap();
            assert_eq!(original.replay.unwrap().accesses, 2);
            assert!(!original.state.pending());
            let mut memory = original.memory.clone();
            let mut audio = Audio::default();
            audio.write_opn2(0, 0, 0x22);
            audio.write_opn2(0, 1, 8);
            for waits in [0, 3] {
                let mut pending = original.clone();
                pending.extend_bus_segment(waits).unwrap();
                let deadline = pending.deadline;
                let before = (cpu.clone(), memory.clone());
                pending
                    .refresh_reads(&cpu, &memory, &cartridge, &audio)
                    .unwrap();
                assert_eq!(pending.deadline, deadline);
                assert_eq!(pending.start, original.start);
                assert!(pending.replay.is_none());
                assert_eq!((cpu.clone(), memory.clone()), before);
                assert_eq!(pending.cpu.regs().pc, if taken { 0x3f81 } else { 0x4001 });
                if original.memory.ram[0x1fff] == 0x10 {
                    assert_eq!(pending.cpu.regs().b, cpu.regs().b.wrapping_sub(1));
                }
                assert_eq!(
                    pending.cpu.cycles(),
                    cpu.cycles() + u64::from(original.instruction.cycles) + waits
                );
                let mut published = cpu.clone();
                let mut output = audio.clone();
                pending.publish(
                    &mut published,
                    &mut memory,
                    &mut [],
                    &mut output,
                    &mut cartridge,
                );
                assert_eq!(published, pending.cpu);
                output.flush_pending();
                assert_eq!(output.opn2_status_at(deadline), 0x80);
            }
            for mutation in 0..5 {
                let mut pending = original.clone();
                let mut changed = original.memory.clone();
                match mutation {
                    0 => changed.ram[0x1fff] = 0x00,
                    1 => pending.replay.as_mut().unwrap().accesses += 1,
                    2 => pending.deadline += 1,
                    3 => changed.ram[0x1fff] = 0xfe,
                    _ => pending.instruction.opcode ^= 8,
                }
                let before = pending.clone();
                assert!(
                    pending
                        .refresh_reads(&cpu, &changed, &cartridge, &audio)
                        .is_err()
                );
                assert_eq!(pending, before);
            }
        }
    }
}
