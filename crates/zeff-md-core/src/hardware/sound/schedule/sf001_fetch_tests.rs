use super::*;
use crate::cartridge::sf001::Revision;
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn board(linear: &[u8], switched: &[u8]) -> Cartridge {
    let mut bytes = crate::cartridge::sf001::tests::rom(Revision::HiddenSave);
    bytes[0x200..0x200 + linear.len()].copy_from_slice(linear);
    bytes[0x380200..0x380200 + switched.len()].copy_from_slice(switched);
    bytes[0x3ffffe..].fill(0);
    let sum = bytes[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, pair| {
            sum.wrapping_add(u16::from_be_bytes([pair[0], pair[1]]))
        });
    bytes[0x3ffffe..].copy_from_slice(&0x3e08_u16.wrapping_sub(sum).to_be_bytes());
    let cartridge = Cartridge::new(bytes).unwrap();
    assert!(cartridge.sf001.is_some());
    cartridge
}

fn memory(program: &[u8]) -> SoundMemory {
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..program.len()].copy_from_slice(program);
    memory
}

fn restore(cpu: &Cpu, schedule: &SoundSchedule, cartridge: &Cartridge) -> (Cpu, SoundSchedule) {
    let mut writer = StateWriter::new();
    schedule.write_state(cpu, &mut writer).unwrap();
    let bytes = writer.into_bytes();
    let mut other_cpu = Cpu::new();
    let mut other = SoundSchedule::default();
    other
        .read_state(&mut other_cpu, &mut StateReader::new(&bytes))
        .unwrap();
    other.validate_cartridge(cartridge).unwrap();
    assert_eq!((&other_cpu, &other), (cpu, schedule));
    (other_cpu, other)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_prefix_m1_phases_wait_restore_and_compare_actual_fetched_bytes() {
    for sequence in [
        &[0xcb, 0][..],
        &[0xed, 0x44][..],
        &[0xdd, 0][..],
        &[0xfd, 0][..],
        &[0xdd, 0xfd, 0xdd, 0][..],
        &[0xed, 0x4f][..],
    ] {
        for phase in 0..sequence.len() {
            for changed in [false, true] {
                let mut switched = sequence.to_vec();
                if changed {
                    switched[phase] ^= 1;
                }
                let mut cartridge = board(sequence, &switched);
                let mut cpu = Cpu::new();
                let mut memory = memory(&[0xc3, 0, 0x82]);
                let mut audio = Audio::default();
                let mut schedule = SoundSchedule::default();
                let start = 150 + phase as u64 * 60;
                schedule.advance(
                    &mut cpu,
                    &mut memory,
                    &mut cartridge,
                    &mut [],
                    SoundInterval::new(0..start + 1, &mut audio),
                );
                assert_eq!(
                    schedule.fault, None,
                    "sequence {sequence:02x?} phase {phase}"
                );
                let pending = schedule.pending.as_ref().unwrap();
                assert_eq!(
                    (pending.instruction.cycles, pending.replay.unwrap().accesses),
                    (4, 1)
                );
                assert_eq!(
                    pending.bank_access,
                    Some(BankAccess {
                        address: 0x200 + phase as u32,
                        direction: Direction::Read,
                        value: sequence[phase]
                    })
                );
                assert_eq!(cpu.regs().pc, 0x8200 + phase as u16);
                assert!(schedule.state.next_segment_reads_opcode());
                let occupied = [Reservation {
                    requester: Requester::MAIN,
                    ticks: start..1000,
                }];
                schedule.advance(
                    &mut cpu,
                    &mut memory,
                    &mut cartridge,
                    &mut [],
                    SoundInterval {
                        ticks: start + 1..1001,
                        occupied: &occupied,
                        sound_irq: None,
                        audio: &mut audio,
                    },
                );
                assert_eq!(schedule.fault, None);
                let deadline = schedule.pending.as_ref().unwrap().deadline;
                assert!(deadline > 1001);
                let expected = schedule.pending.as_ref().unwrap().cpu.clone();
                cartridge.sf001.as_mut().unwrap().write(0xe00, 0x80);
                memory.bank = 0x1ff;
                let (other_cpu, other_schedule) = restore(&cpu, &schedule, &cartridge);
                let mut other = (
                    other_cpu,
                    memory.clone(),
                    cartridge.clone(),
                    audio.clone(),
                    other_schedule,
                );
                let before = (
                    cpu.clone(),
                    memory.clone(),
                    cartridge.clone(),
                    schedule.state,
                );
                for (cpu, memory, cartridge, audio, schedule) in [
                    (
                        &mut cpu,
                        &mut memory,
                        &mut cartridge,
                        &mut audio,
                        &mut schedule,
                    ),
                    (
                        &mut other.0,
                        &mut other.1,
                        &mut other.2,
                        &mut other.3,
                        &mut other.4,
                    ),
                ] {
                    schedule.advance(
                        cpu,
                        memory,
                        cartridge,
                        &mut [],
                        SoundInterval::new(1001..deadline, audio),
                    );
                    if changed {
                        assert!(schedule.fault.is_some());
                        assert_eq!(
                            (
                                cpu.clone(),
                                memory.clone(),
                                cartridge.clone(),
                                schedule.state
                            ),
                            before
                        );
                    } else {
                        assert_eq!(schedule.fault, None);
                        assert_eq!(*cpu, expected);
                        assert_eq!(memory.bank, 0);
                    }
                    assert!(schedule.pending.is_none());
                }
                assert_eq!((cpu, memory, cartridge, audio, schedule), other);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_repeated_prefix_m1_survives_r_wrap_and_saturated_fetch_count() {
    let mut program = vec![0xdd; 260];
    program.push(0);
    let original = board(&program, &program);
    for seed in [0x7e, 0xfe] {
        let mut cartridge = original.clone();
        let mut cpu = Cpu::new();
        let mut memory = memory(&[0x3e, seed, 0xed, 0x4f, 0xc3, 0, 0x82]);
        let mut audio = Audio::default();
        let mut schedule = SoundSchedule::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(0..390, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!((cpu.regs().pc, cpu.regs().r), (0x8200, seed + 1));
        let mut now = 390;
        for phase in 0..program.len() {
            let prior_r = cpu.regs().r;
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut [],
                SoundInterval::new(now..now + 1, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            let pending = schedule.pending.as_ref().unwrap();
            assert_eq!(pending.instruction.cycles, 4);
            assert_eq!(pending.bank_access.unwrap().address, 0x200 + phase as u32);
            assert_eq!(
                pending.cpu.regs().r,
                (prior_r & 0x80) | (prior_r.wrapping_add(1) & 0x7f)
            );
            if phase >= 255 {
                assert_eq!(pending.cpu.last_m1_fetch_count(), 255);
            }
            restore(&cpu, &schedule, &cartridge);
            cartridge
                .sf001
                .as_mut()
                .unwrap()
                .write(0xe00, if phase & 1 == 0 { 0x80 } else { 0 });
            let deadline = pending.deadline;
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut [],
                SoundInterval::new(now + 1..deadline, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            assert!(schedule.pending.is_none());
            now = deadline;
        }
        assert_eq!(cpu.regs().pc, 0x8200 + program.len() as u16);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_four_cycle_rmw_read_at_current_pc_samples_data_without_opcode_binding() {
    let mut cartridge = board(&[0x34, 0x5a], &[0x34, 0x42]);
    let mut cpu = Cpu::new();
    let mut memory = memory(&[0x21, 1, 0x82, 0xc3, 0, 0x82]);
    let mut audio = Audio::default();
    let mut schedule = SoundSchedule::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..360, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!((cpu.regs().pc, cpu.regs().hl()), (0x8201, 0x8201));
    assert!(!schedule.state.next_segment_reads_opcode());
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(360..361, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(
        schedule
            .pending
            .as_ref()
            .unwrap()
            .bank_access
            .unwrap()
            .value,
        0x5a
    );
    cartridge.sf001.as_mut().unwrap().write(0xe00, 0x80);
    restore(&cpu, &schedule, &cartridge);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(361..420, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert!(schedule.pending.is_none());
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(420..421, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(
        schedule.pending.as_ref().unwrap().bank_access,
        Some(BankAccess {
            address: 0x201,
            direction: Direction::Write,
            value: 0x43
        })
    );
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(421..465, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert!(schedule.pending.is_none());
    assert_eq!(cartridge.read(0x201), Some(0x42));
}

fn indexed_phases(prefix: u8, terminal: Option<u8>) {
    let mut linear = vec![0; 17];
    let opcode = terminal.unwrap_or(0x37);
    linear[..4].copy_from_slice(&[
        prefix,
        if terminal.is_some() { 0xcb } else { 0x36 },
        0x10,
        opcode,
    ]);
    linear[16] = 0x81;
    let mut switched = linear.clone();
    if terminal.is_some() {
        switched[16] = 0x42;
    } else {
        switched[3] = 0x42;
    }
    let mut cartridge = board(&linear, &switched);
    let mut cpu = Cpu::new();
    let mut memory = memory(&[prefix, 0x21, 0, 0x82, 0xc3, 0, 0x82]);
    let mut audio = Audio::default();
    let mut schedule = SoundSchedule::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..360, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().pc, 0x8200);
    let phases = if terminal == Some(0x46) {
        vec![(4, true), (4, true), (3, false), (5, true), (4, false)]
    } else if terminal.is_some() {
        vec![
            (4, true),
            (4, true),
            (3, false),
            (5, true),
            (4, false),
            (3, false),
        ]
    } else {
        vec![(4, true), (4, true), (3, false), (5, false), (3, false)]
    };
    let mut now = 360;
    for (phase, (cycles, opcode_read)) in phases.into_iter().enumerate() {
        assert_eq!(schedule.state.next_segment_reads_opcode(), opcode_read);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(now..now + 1, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        let pending = schedule.pending.as_ref().unwrap();
        assert_eq!(pending.instruction.cycles, cycles);
        let captured = pending.bank_access.unwrap();
        let write_phase = terminal.is_some() && phase == 5 || terminal.is_none() && phase == 4;
        assert_eq!(
            captured.direction,
            if write_phase {
                Direction::Write
            } else {
                Direction::Read
            }
        );
        if write_phase {
            assert_eq!(captured.value, if terminal.is_some() { 3 } else { 0x42 });
        }
        let occupied = [Reservation {
            requester: Requester::MAIN,
            ticks: now..now + 60,
        }];
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval {
                ticks: now + 1..now + 61,
                occupied: &occupied,
                sound_irq: None,
                audio: &mut audio,
            },
        );
        assert_eq!(schedule.fault, None);
        let deadline = schedule.pending.as_ref().unwrap().deadline;
        assert!(deadline > now + 61);
        cartridge.sf001.as_mut().unwrap().write(
            0xe00,
            if terminal.is_some() && phase == 4 {
                0
            } else {
                0x80
            },
        );
        let (other_cpu, other_schedule) = restore(&cpu, &schedule, &cartridge);
        let mut other = (
            other_cpu,
            memory.clone(),
            cartridge.clone(),
            audio.clone(),
            other_schedule,
        );
        for (cpu, memory, cartridge, audio, schedule) in [
            (
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut audio,
                &mut schedule,
            ),
            (
                &mut other.0,
                &mut other.1,
                &mut other.2,
                &mut other.3,
                &mut other.4,
            ),
        ] {
            schedule.advance(
                cpu,
                memory,
                cartridge,
                &mut [],
                SoundInterval::new(now + 61..deadline, audio),
            );
            assert_eq!(schedule.fault, None);
            assert!(schedule.pending.is_none());
        }
        assert_eq!(
            (&cpu, &memory, &cartridge, &audio, &schedule),
            (&other.0, &other.1, &other.2, &other.3, &other.4)
        );
        now = deadline;
    }
    assert_eq!(cpu.regs().pc, 0x8204);
    if terminal == Some(0) {
        assert_eq!(cpu.regs().b, 3);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_index_cb_terminal_opcode_and_data_rmw_phases_restore_after_each_wait() {
    for prefix in [0xdd, 0xfd] {
        for terminal in [0, 0x46] {
            indexed_phases(prefix, Some(terminal));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_index_immediate_five_cycle_operand_replays_changed_value_before_write() {
    for prefix in [0xdd, 0xfd] {
        indexed_phases(prefix, None);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_index_cb_terminal_opcode_change_rejects_before_data_or_write() {
    for prefix in [0xdd, 0xfd] {
        let mut cartridge = board(&[prefix, 0xcb, 0x10, 0], &[prefix, 0xcb, 0x10, 1]);
        let mut cpu = Cpu::new();
        let mut memory = memory(&[prefix, 0x21, 0, 0x82, 0xc3, 0, 0x82]);
        let mut audio = Audio::default();
        let mut schedule = SoundSchedule::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(0..525, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert!(schedule.pending.is_none());
        assert!(schedule.state.next_segment_reads_opcode());
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(525..526, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        let pending = schedule.pending.as_ref().unwrap();
        assert_eq!(pending.instruction.cycles, 5);
        assert_eq!(pending.bank_access.unwrap().address, 0x203);
        cartridge.sf001.as_mut().unwrap().write(0xe00, 0x80);
        restore(&cpu, &schedule, &cartridge);
        let before = (
            cpu.clone(),
            memory.clone(),
            cartridge.clone(),
            schedule.state,
        );
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(526..600, &mut audio),
        );
        assert_eq!(
            schedule.fault,
            Some(SoundFault::UnstableCartridgeRead { pc: 0x8200 })
        );
        assert_eq!(
            (
                cpu.clone(),
                memory.clone(),
                cartridge.clone(),
                schedule.state
            ),
            before
        );
        assert!(schedule.pending.is_none());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_halted_refresh_reads_changed_mapping_without_binding_ignored_byte() {
    let mut cartridge = board(&[0x76, 0x5a], &[0x76, 0xa6]);
    let mut cpu = Cpu::new();
    let mut memory = memory(&[0xc3, 0, 0x82]);
    let mut audio = Audio::default();
    let mut schedule = SoundSchedule::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..211, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.state(), zeff_z80::CpuState::Halted);
    assert_eq!(cpu.regs().pc, 0x8201);
    let pending = schedule.pending.as_ref().unwrap();
    assert_eq!(pending.instruction.cycles, 4);
    assert_eq!(pending.bank_access.unwrap().value, 0x5a);
    cartridge.sf001.as_mut().unwrap().write(0xe00, 0x80);
    restore(&cpu, &schedule, &cartridge);
    let mut refreshed = pending.clone();
    refreshed
        .refresh_reads(&cpu, &memory, &cartridge, &audio)
        .unwrap();
    assert_eq!(refreshed.bank_access.unwrap().value, 0xa6);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(211..270, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.state(), zeff_z80::CpuState::Halted);
    assert_eq!(cpu.regs().pc, 0x8201);
    assert!(schedule.pending.is_none());
}
