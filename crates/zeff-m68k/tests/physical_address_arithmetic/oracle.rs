use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    pub access: BusAccess,
    pub phase: InstructionPhase,
    pub registers: Registers,
    pub internal_pc: u32,
    pub ird: u16,
}

pub(super) fn sample(words: &BTreeMap<u32, u16>, address: u32) -> u16 {
    words
        .get(&(address & BUS_ADDRESS_MASK & !1))
        .copied()
        .unwrap_or(0x4e71)
}

pub(super) fn access(address: u32, supervisor: bool, program: bool, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & BUS_ADDRESS_MASK & !1,
        function_code: match (program, supervisor) {
            (false, false) => FunctionCode::UserData,
            (false, true) => FunctionCode::SupervisorData,
            (true, false) => FunctionCode::UserProgram,
            (true, true) => FunctionCode::SupervisorProgram,
        },
        kind,
        lanes: ByteLanes::Both,
    }
}

pub(super) fn address(case: Case, words: &BTreeMap<u32, u16>) -> u32 {
    let base = if matches!(case.form.ea, Ea::PcDisplacement | Ea::PcIndex) {
        case.r.pc.wrapping_add(2)
    } else {
        case.r.address_register(usize::from(case.register)).unwrap()
    };
    match case.form.ea {
        Ea::Indirect | Ea::Post => base,
        Ea::Pre => base.wrapping_sub(case.form.stride()),
        Ea::Displacement | Ea::PcDisplacement => {
            base.wrapping_add_signed(i32::from(case.extension as i16))
        }
        Ea::Index | Ea::PcIndex => {
            let selector = usize::from(case.extension >> 12 & 7);
            let raw = if case.extension & 0x8000 != 0 {
                case.r.address_register(selector).unwrap()
            } else {
                case.r.d[selector]
            };
            let index = if case.extension & 0x800 != 0 {
                raw
            } else {
                i32::from(raw as i16) as u32
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(case.extension as u8 as i8))
        }
        Ea::AbsoluteWord => i32::from(case.extension as i16) as u32,
        Ea::AbsoluteLong => {
            u32::from(case.extension) << 16 | u32::from(sample(words, case.r.pc.wrapping_add(4)))
        }
    }
}

pub(super) fn value(case: Case, words: &BTreeMap<u32, u16>) -> u32 {
    let ea = address(case, words);
    let high = u32::from(sample(words, ea));
    match case.form.size {
        Size::Byte if ea & 1 == 0 => high >> 8,
        Size::Byte => high & 255,
        Size::Word => high,
        Size::Long => high << 16 | u32::from(sample(words, ea.wrapping_add(2))),
    }
}

pub(super) fn arithmetic(case: Case, words: &BTreeMap<u32, u16>) -> (u32, u32) {
    let mut r = case.r;
    let source_ea = address(case, words);
    if case.form.ea == Ea::Pre {
        r.set_address_register(usize::from(case.register), source_ea);
    }
    if case.form.ea == Ea::Post {
        r.set_address_register(
            usize::from(case.register),
            source_ea.wrapping_add(case.form.stride()),
        );
    }
    let destination = r.address_register(usize::from(case.destination)).unwrap();
    let raw = value(case, words);
    let source = if case.form.size == Size::Word {
        i32::from(raw as i16) as u32
    } else {
        raw
    };
    let result = if case.form.operation == Operation::Add {
        destination.wrapping_add(source)
    } else {
        destination.wrapping_sub(source)
    };
    (destination & 0xffff0000 | result & 65535, result)
}

pub(super) fn next(case: Case, words: &BTreeMap<u32, u16>) -> u16 {
    if case.form.extensions() == 0 {
        case.next
    } else {
        sample(words, case.r.pc.wrapping_add(case.form.length()))
    }
}

pub(super) fn plan(case: Case, words: &BTreeMap<u32, u16>) -> Vec<Transfer> {
    let mut result = Vec::new();
    let mut r = case.r;
    let opcode = case.opcode();
    for i in 0..case.form.extensions() {
        result.push(Transfer {
            access: access(
                case.r.pc.wrapping_add(4 + 2 * i as u32),
                r.supervisor(),
                true,
                AccessKind::Prefetch,
            ),
            phase: if i == 0 {
                InstructionPhase::ExtensionFirst
            } else {
                InstructionPhase::ExtensionSecond
            },
            registers: r,
            internal_pc: case.r.pc.wrapping_add(if i == 1 {
                6
            } else if case.form.ea == Ea::AbsoluteWord {
                4
            } else {
                2
            }),
            ird: opcode,
        });
    }
    let source = address(case, words);
    for low in 0..=usize::from(case.form.size == Size::Long) {
        if case.form.ea == Ea::Pre && low == 0 {
            r.set_address_register(usize::from(case.register), source);
        }
        if case.form.ea == Ea::Post && (case.form.size == Size::Word || low == 1) {
            r.set_address_register(
                usize::from(case.register),
                source.wrapping_add(case.form.stride()),
            );
        }
        result.push(Transfer {
            access: access(
                source.wrapping_add(low as u32 * 2),
                r.supervisor(),
                matches!(case.form.ea, Ea::PcDisplacement | Ea::PcIndex),
                AccessKind::Data,
            ),
            phase: if low == 0 {
                InstructionPhase::OperandReadHigh
            } else {
                InstructionPhase::OperandReadLow
            },
            registers: r,
            internal_pc: case.r.pc.wrapping_add(match case.form.ea {
                Ea::AbsoluteLong => 6,
                Ea::AbsoluteWord => 4,
                Ea::Pre if case.form.size == Size::Word => 4,
                _ => 2,
            }),
            ird: opcode,
        });
    }
    r.set_address_register(usize::from(case.destination), arithmetic(case, words).0);
    let address = case.r.pc.wrapping_add(case.form.length() + 2);
    result.push(Transfer {
        access: access(address, r.supervisor(), true, AccessKind::Prefetch),
        phase: InstructionPhase::OperandRefill,
        registers: r,
        internal_pc: address,
        ird: next(case, words),
    });
    result
}

pub(super) fn clocks(case: Case, elapsed: &[u32; 5], count: usize, idle: bool) -> u64 {
    elapsed[..count].iter().map(|n| u64::from(*n)).sum::<u64>()
        + if idle && case.form.idle() { 2 } else { 0 }
}

pub(super) fn events(case: Case, plan: &[Transfer], elapsed: &[u32; 5], idle: bool) -> Vec<Event> {
    let mut clock = case.start;
    let mut result = Vec::new();
    if idle && case.form.idle() {
        result.push(Event::Idle(clock, 2));
        clock += 2;
    }
    for (t, elapsed) in plan.iter().zip(elapsed) {
        result.push(Event::Read(t.access, clock));
        clock += u64::from(*elapsed);
    }
    result
}

pub(super) fn completed(case: Case, waited: bool) {
    let (mut cpu, mut bus) = case.seed();
    if waited {
        bus.elapsed = [7, 11, 5, 13, 9];
    }
    let initial = bus.words.clone();
    let plan = plan(case, &initial);
    let tail = if case.form.size == Size::Word { 4 } else { 2 };
    let clocks = clocks(case, &bus.elapsed, plan.len(), true) + tail;
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: case.opcode(),
                clocks,
                exception: None
            }
        ))
    );
    let mut r = plan.last().unwrap().registers;
    r.set_address_register(usize::from(case.destination), arithmetic(case, &initial).1);
    r.pc = case.r.pc.wrapping_add(case.form.length());
    assert_eq!(cpu.registers, r);
    assert_eq!(
        cpu.queued_words(),
        Some([next(case, &initial), sample(&initial, r.pc.wrapping_add(2))])
    );
    assert_eq!(bus.words, initial);
    let mut expected_events = events(case, &plan, &bus.elapsed, true);
    if tail == 4 {
        expected_events.push(Event::Idle(case.start + clocks - 4, 2));
    }
    expected_events.push(Event::Idle(case.start + clocks - 2, 2));
    assert_eq!(bus.events, expected_events);
    assert_eq!(bus.count, plan.len());
    assert_eq!(cpu.clock().0, case.start + clocks);
    assert_eq!(cpu.clock().0, bus.clock);
}

pub(super) fn capture(t: Transfer, vector: u8) -> Group0Capture {
    Group0Capture {
        vector,
        internal_pc: t.internal_pc,
        phase_sr: t.registers.status_register(),
        ird: t.ird,
        logical_address: t.access.logical_address,
        function_code: t.access.function_code,
        direction: Direction::Read,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    }
}

pub(super) fn entry_events(sp: u32, c: Group0Capture, start: u64) -> Vec<Event> {
    let ssw = c.ird & 0xffe0
        | c.function_code as u16
        | if c.direction == Direction::Read {
            16
        } else {
            0
        };
    let mut clock = start + 4;
    let mut result = vec![Event::Idle(start, 4)];
    for (delta, value) in [
        (2, c.internal_pc as u16),
        (6, c.phase_sr),
        (4, (c.internal_pc >> 16) as u16),
        (8, c.ird),
        (10, c.logical_address as u16),
        (14, ssw),
        (12, (c.logical_address >> 16) as u16),
    ] {
        result.push(Event::EntryWrite(
            access(sp.wrapping_sub(delta), true, false, AccessKind::Stack),
            value,
            clock,
        ));
        clock += 4;
    }
    for address in [u32::from(c.vector) * 4, u32::from(c.vector) * 4 + 2] {
        result.push(Event::EntryRead(
            access(address, true, false, AccessKind::Vector),
            clock,
        ));
        clock += 4;
    }
    result.push(Event::EntryRead(
        access(0x4321_4000, true, true, AccessKind::Prefetch),
        clock,
    ));
    clock += 4;
    result.push(Event::Idle(clock, 2));
    clock += 2;
    result.push(Event::EntryRead(
        access(0x4321_4002, true, true, AccessKind::Prefetch),
        clock,
    ));
    result
}

pub(super) fn entry_memory(
    mut words: BTreeMap<u32, u16>,
    sp: u32,
    c: Group0Capture,
) -> BTreeMap<u32, u16> {
    for event in entry_events(sp, c, 0) {
        if let Event::EntryWrite(a, v, _) = event {
            words.insert(a.bus_address, v);
        }
    }
    words
}

pub(super) fn frame(bus: &BusDevice, sp: u32, c: Group0Capture, start: u64) {
    let at = bus
        .events
        .iter()
        .position(|e| *e == Event::Idle(start, 4))
        .unwrap();
    assert_eq!(bus.events[at..], entry_events(sp, c, start));
    let words = [
        c.ird & 0xffe0
            | c.function_code as u16
            | if c.direction == Direction::Read {
                16
            } else {
                0
            },
        (c.logical_address >> 16) as u16,
        c.logical_address as u16,
        c.ird,
        c.phase_sr,
        (c.internal_pc >> 16) as u16,
        c.internal_pc as u16,
    ];
    for (i, word) in words.into_iter().enumerate() {
        assert_eq!(
            bus.sample(sp.wrapping_sub(14).wrapping_add(i as u32 * 2)),
            word
        );
    }
}

pub(super) fn entered(mut r: Registers) -> Registers {
    r.ssp = r.ssp.wrapping_sub(14);
    r.pc = 0x4321_4000;
    r.set_status_register(r.status_register() | SUPERVISOR);
    r
}
