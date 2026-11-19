use super::*;

pub(super) fn scenario(
    case: usize,
    initial: Registers,
    start: u64,
) -> (
    PrefetchCpu,
    Device,
    InstructionPhase,
    Group0Capture,
    Registers,
    u64,
) {
    let pc = initial.pc;
    let (opcode, queued, failed, phase, address, internal_pc, ird, prior) = match case {
        0 => (
            0x4e71,
            0x7201,
            0,
            InstructionPhase::SequentialRefill,
            pc.wrapping_add(4),
            pc.wrapping_add(4),
            0x7201,
            0,
        ),
        1 => (
            0x7aa5,
            0x4e71,
            0,
            InstructionPhase::SequentialRefill,
            pc.wrapping_add(4),
            pc.wrapping_add(4),
            0x4e71,
            0,
        ),
        2 => (
            0x6008,
            0x7201,
            0,
            InstructionPhase::BranchFirst,
            pc.wrapping_add(10),
            pc.wrapping_add(2),
            0x6008,
            2,
        ),
        3 => (
            0x6008,
            0x7201,
            1,
            InstructionPhase::BranchSecond,
            pc.wrapping_add(12),
            pc.wrapping_add(12),
            0x7480,
            9,
        ),
        _ => unreachable!(),
    };
    let mut phase_registers = initial;
    if case == 1 {
        moveq(&mut phase_registers, 5, 0xa5);
    }
    let capture = Group0Capture {
        vector: 2,
        internal_pc,
        phase_sr: phase_registers.status_register(),
        ird,
        logical_address: address,
        function_code: access(address, initial.supervisor()).function_code,
        direction: Direction::Read,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    };
    let cpu = PrefetchCpu::from_prefetched(initial, [opcode, queued], CpuClocks(start)).unwrap();
    let mut device = Device::new(start);
    device.instruction_elapsed = [7, 17];
    device.instruction_elapsed[failed] = 17;
    device.instruction_failure = Some((failed, Failure::Boundary));
    device.word(pc.wrapping_add(10), 0x7480);
    (cpu, device, phase, capture, phase_registers, prior)
}

pub(super) fn entry_events(
    capture: Group0Capture,
    sp: u32,
    origin: u64,
    elapsed: &[u32; 11],
) -> Vec<Event> {
    let mut clock = origin + 4;
    let mut events = vec![Event::Idle(origin, 4)];
    let ssw = (capture.ird & 0xffe0) | capture.function_code as u16 | 16;
    for (index, (offset, value)) in [
        (2, capture.internal_pc as u16),
        (6, capture.phase_sr),
        (4, (capture.internal_pc >> 16) as u16),
        (8, capture.ird),
        (10, capture.logical_address as u16),
        (14, ssw),
        (12, (capture.logical_address >> 16) as u16),
    ]
    .into_iter()
    .enumerate()
    {
        let mut access = access(sp.wrapping_sub(offset), true);
        access.function_code = FunctionCode::SupervisorData;
        access.kind = AccessKind::Stack;
        events.push(Event::EntryWrite(access, value, clock));
        clock += u64::from(elapsed[index]);
    }
    for (index, address) in [
        u32::from(capture.vector) * 4,
        u32::from(capture.vector) * 4 + 2,
        0x1234_4000,
        0x1234_4002,
    ]
    .into_iter()
    .enumerate()
    {
        if index == 3 {
            events.push(Event::Idle(clock, 2));
            clock += 2;
        }
        let mut access = access(address, true);
        if index < 2 {
            access.function_code = FunctionCode::SupervisorData;
            access.kind = AccessKind::Vector;
        }
        events.push(Event::EntryRead(access, clock));
        clock += u64::from(elapsed[index + 7]);
    }
    events
}

#[test]
fn all_four_phase_captures_stack_literal_witnesses_and_owned_elapsed_once() {
    for supervisor in [false, true] {
        for ccr in 0..32 {
            for case in 0..4 {
                for pc in [0x2345_2000, 0xffff_fff8] {
                    let initial = registers(if supervisor { SUPERVISOR } else { 0 } | ccr, pc);
                    let (mut cpu, mut bus, _, captured, mut expected, prior) =
                        scenario(case, initial, 83);
                    bus.entry_elapsed = std::array::from_fn(|i| 4 + i as u32);
                    let result = cpu.step_physical_instruction(&mut bus).unwrap();
                    let opcode = [0x4e71, 0x7aa5, 0x6008, 0x6008][case];
                    assert_eq!(
                        result,
                        PhysicalInstructionStep::Group0 {
                            opcode,
                            capture: captured,
                            clocks: prior + 17 + 105
                        }
                    );
                    assert_frame(&bus, initial.ssp, captured);
                    expected.ssp = initial.ssp - 14;
                    expected.pc = 0x1234_4000;
                    expected.set_status_register(captured.phase_sr | SUPERVISOR);
                    assert_eq!(cpu.registers, expected);
                    assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
                    assert_eq!(cpu.clock().0, 83 + prior + 17 + 105);
                    assert_eq!(cpu.clock().0, bus.clock);
                    let entry_start = bus
                        .events
                        .iter()
                        .position(|event| matches!(event, Event::Idle(_, 4)))
                        .unwrap();
                    assert_eq!(
                        &bus.events[entry_start..],
                        entry_events(captured, initial.ssp, 83 + prior + 17, &bus.entry_elapsed)
                    );
                }
            }
        }
    }
}

#[test]
fn secondary_faults_after_moveq_keep_its_visible_partial_effect_and_never_recurse() {
    for failed in 0..11 {
        for outcome in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
            let initial = registers(0x001f, 0x2000);
            let (mut cpu, mut bus, _, captured, expected, _) = scenario(1, initial, 29);
            bus.entry_failure = Some((failed, outcome));
            let result = cpu.step_physical_instruction(&mut bus);
            assert!(matches!(result, Err(InstructionFault::Entry(_))));
            assert_eq!(cpu.registers, expected);
            assert_eq!(cpu.registers.d[5], 0xffff_ffa5);
            assert_eq!(cpu.registers.status_register(), 0x18);
            assert_eq!(bus.instruction_count, 1);
            assert_eq!(bus.entry_count, failed + 1);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(
                cpu.clock().0,
                29 + 17 + 4 + 4 * (failed as u64 + 1) + if failed == 10 { 2 } else { 0 }
            );
            assert_eq!(cpu.clock().0, bus.clock);
            let expected_events = entry_events(captured, initial.ssp, 46, &bus.entry_elapsed);
            let end = expected_events
                .iter()
                .enumerate()
                .filter(|(_, event)| matches!(event, Event::EntryRead(..) | Event::EntryWrite(..)))
                .nth(failed)
                .unwrap()
                .0;
            assert_eq!(&bus.events[1..], &expected_events[..=end]);
            for event in &bus.events {
                if let Event::EntryWrite(access, value, _) = event {
                    assert_eq!(bus.words[&access.bus_address], *value);
                }
            }
            let before = bus.events.clone();
            assert!(cpu.step_physical_instruction(&mut bus).is_err());
            assert_eq!(bus.events, before);
        }
    }
}

#[test]
fn moveq_fault_capture_preserves_extend_and_selects_every_destination() {
    for supervisor in [false, true] {
        for register in 0..8 {
            for immediate in [0, 1, 127, 128, 255] {
                for ccr in [0, 31] {
                    let initial = registers(if supervisor { SUPERVISOR } else { 0 } | ccr, 0x2000);
                    let opcode = 0x7000 | register << 9 | immediate;
                    let mut cpu =
                        PrefetchCpu::from_prefetched(initial, [opcode, 0x4e71], CpuClocks(0))
                            .unwrap();
                    let mut bus = Device::new(0);
                    bus.instruction_elapsed[0] = 13;
                    bus.instruction_failure = Some((0, Failure::Boundary));
                    let mut expected = initial;
                    moveq(&mut expected, usize::from(register), immediate as u8);
                    let captured = Group0Capture {
                        vector: 2,
                        internal_pc: 0x2004,
                        phase_sr: expected.status_register(),
                        ird: 0x4e71,
                        logical_address: 0x2004,
                        function_code: access(0x2004, supervisor).function_code,
                        direction: Direction::Read,
                        processing_class: ProcessingClass::InstructionOrGroup2,
                        ssw_profile: SswProfile::SelectedIrdUpperBits,
                    };
                    assert_eq!(
                        cpu.step_physical_instruction(&mut bus),
                        Ok(PhysicalInstructionStep::Group0 {
                            opcode,
                            capture: captured,
                            clocks: 63
                        })
                    );
                    assert_frame(&bus, initial.ssp, captured);
                    expected.ssp -= 14;
                    expected.pc = 0x1234_4000;
                    expected.set_status_register(expected.status_register() | SUPERVISOR);
                    assert_eq!(cpu.registers, expected);
                    assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
                    assert_eq!(cpu.clock().0, bus.clock);
                }
            }
        }
    }
}
