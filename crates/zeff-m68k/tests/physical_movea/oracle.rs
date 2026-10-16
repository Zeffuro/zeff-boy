use super::*;

#[derive(Clone, Copy)]
enum Update {
    Old,
    BeforeRead,
    BeforeLow,
    Destination,
}

#[derive(Clone, Copy)]
struct Row {
    phase: InstructionPhase,
    pc: u32,
    update: Update,
}

fn rows(form: Form) -> Vec<Row> {
    use InstructionPhase::{
        ExtensionFirst as E4, ExtensionSecond as E6, OperandReadHigh as RH, OperandReadLow as RL,
        OperandRefill as F,
    };
    use Update::{BeforeLow, BeforeRead, Destination, Old};
    let mut rows = match form.family {
        Family::Data | Family::Address | Family::Indirect | Family::Post | Family::Pre => vec![],
        Family::AbsoluteLong => vec![
            Row {
                phase: E4,
                pc: 2,
                update: Old,
            },
            Row {
                phase: E6,
                pc: 6,
                update: Old,
            },
        ],
        Family::Immediate if form.size == Size::Long => vec![
            Row {
                phase: E4,
                pc: 2,
                update: Old,
            },
            Row {
                phase: E6,
                pc: 6,
                update: Old,
            },
        ],
        Family::AbsoluteWord | Family::Immediate => vec![Row {
            phase: E4,
            pc: 4,
            update: Old,
        }],
        _ => vec![Row {
            phase: E4,
            pc: 2,
            update: Old,
        }],
    };
    if form.memory() {
        let pc = match form.family {
            Family::AbsoluteLong => 6,
            Family::AbsoluteWord => 4,
            Family::Pre if form.size == Size::Word => 4,
            _ => 2,
        };
        let update = if form.family == Family::Pre
            || form.family == Family::Post && form.size == Size::Word
        {
            BeforeRead
        } else {
            Old
        };
        rows.push(Row {
            phase: RH,
            pc,
            update,
        });
        if form.size == Size::Long {
            rows.push(Row {
                phase: RL,
                pc,
                update: if form.family == Family::Post {
                    BeforeLow
                } else {
                    update
                },
            });
        }
    }
    rows.push(Row {
        phase: F,
        pc: form.length() + 2,
        update: Destination,
    });
    rows
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    pub access: BusAccess,
    pub phase: InstructionPhase,
    pub registers: Registers,
    pub internal_pc: u32,
    pub ird: u16,
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

pub(super) fn sample(words: &BTreeMap<u32, u16>, address: u32) -> u16 {
    words
        .get(&(address & BUS_ADDRESS_MASK & !1))
        .copied()
        .unwrap_or(0x4e71)
}

pub(super) fn result(case: Case, words: &BTreeMap<u32, u16>) -> u32 {
    let low = sample(words, case.r.pc.wrapping_add(4));
    let raw = match case.form.family {
        Family::Data => case.r.d[usize::from(case.source)],
        Family::Address => case.r.address_register(usize::from(case.source)).unwrap(),
        Family::Immediate if case.form.size == Size::Long => {
            u32::from(case.e0) << 16 | u32::from(low)
        }
        Family::Immediate => u32::from(case.e0),
        _ => {
            let ea = case.effective(low);
            let high = sample(words, ea);
            if case.form.size == Size::Long {
                u32::from(high) << 16 | u32::from(sample(words, ea.wrapping_add(2)))
            } else {
                u32::from(high)
            }
        }
    };
    if case.form.size == Size::Long {
        raw
    } else {
        i32::from(raw as u16 as i16) as u32
    }
}

pub(super) fn plan(case: Case, words: &BTreeMap<u32, u16>) -> Vec<Transfer> {
    let ea = case.effective(sample(words, case.r.pc.wrapping_add(4)));
    let value = result(case, words);
    let next = if case.form.length() == 2 {
        case.e0
    } else {
        sample(words, case.r.pc.wrapping_add(case.form.length()))
    };
    let source_update = case
        .r
        .address_register(usize::from(case.source))
        .unwrap()
        .wrapping_add_signed(if case.form.family == Family::Pre {
            if case.form.size == Size::Long { -4 } else { -2 }
        } else if case.form.size == Size::Long {
            4
        } else {
            2
        });
    rows(case.form)
        .into_iter()
        .map(|row| {
            let mut registers = case.r;
            if matches!(
                row.update,
                Update::BeforeRead | Update::BeforeLow | Update::Destination
            ) && matches!(case.form.family, Family::Post | Family::Pre)
            {
                assert!(registers.set_address_register(usize::from(case.source), source_update));
            }
            if matches!(row.update, Update::Destination) {
                assert!(registers.set_address_register(usize::from(case.destination), value));
            }
            let operand = matches!(
                row.phase,
                InstructionPhase::OperandReadHigh | InstructionPhase::OperandReadLow
            );
            let address = match row.phase {
                InstructionPhase::ExtensionFirst => case.r.pc.wrapping_add(4),
                InstructionPhase::ExtensionSecond => case.r.pc.wrapping_add(6),
                InstructionPhase::OperandReadHigh => ea,
                InstructionPhase::OperandReadLow => ea.wrapping_add(2),
                _ => case.r.pc.wrapping_add(case.form.length() + 2),
            };
            Transfer {
                access: access(
                    address,
                    case.r.supervisor(),
                    !operand
                        || matches!(case.form.family, Family::PcDisplacement | Family::PcIndexed),
                    if operand {
                        AccessKind::Data
                    } else {
                        AccessKind::Prefetch
                    },
                ),
                phase: row.phase,
                registers,
                internal_pc: case.r.pc.wrapping_add(row.pc),
                ird: if row.phase == InstructionPhase::OperandRefill {
                    next
                } else {
                    case.form.opcode(case.destination, case.source)
                },
            }
        })
        .collect()
}

pub(super) fn events(case: Case, transfers: &[Transfer], elapsed: &[u32; 5]) -> Vec<Event> {
    let mut clock = case.start;
    let mut result = Vec::new();
    if case.form.idle() != 0 {
        result.push(Event::Idle(clock, 2));
        clock += 2;
    }
    for (i, t) in transfers.iter().enumerate() {
        result.push(Event::Read(t.access, clock));
        clock += u64::from(elapsed[i]);
    }
    result
}

pub(super) fn clocks(case: Case, elapsed: &[u32; 5], count: usize) -> u64 {
    case.form.idle() + elapsed[..count].iter().map(|&n| u64::from(n)).sum::<u64>()
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
    let ssw = c.ird & 0xffe0 | c.function_code as u16 | 16;
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

pub(super) fn frame(bus: &Device, sp: u32, c: Group0Capture, start: u64) {
    let at = bus
        .events
        .iter()
        .position(|e| *e == Event::Idle(start, 4))
        .unwrap();
    assert_eq!(bus.events[at..], entry_events(sp, c, start));
    let words = [
        c.ird & 0xffe0 | c.function_code as u16 | 16,
        (c.logical_address >> 16) as u16,
        c.logical_address as u16,
        c.ird,
        c.phase_sr,
        (c.internal_pc >> 16) as u16,
        c.internal_pc as u16,
    ];
    let expected: Vec<_> = words.into_iter().flat_map(u16::to_be_bytes).collect();
    let actual: Vec<_> = (0..7)
        .flat_map(|i| {
            bus.sample(sp.wrapping_sub(14).wrapping_add(i * 2))
                .to_be_bytes()
        })
        .collect();
    assert_eq!(actual, expected);
}

pub(super) fn entered(mut r: Registers) -> Registers {
    r.ssp = r.ssp.wrapping_sub(14);
    r.pc = 0x4321_4000;
    r.set_status_register(r.status_register() | SUPERVISOR);
    r
}
