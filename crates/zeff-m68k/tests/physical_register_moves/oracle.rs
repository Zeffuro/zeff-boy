use super::*;

#[derive(Clone, Copy)]
enum Flags {
    Old,
    Sized,
    Low,
    Full,
    HighOld,
}

#[derive(Clone, Copy)]
struct Row {
    phase: InstructionPhase,
    flags: Flags,
    pc: u32,
    update: i32,
    queued: bool,
}

fn row(phase: InstructionPhase, flags: Flags, pc: u32, update: i32, queued: bool) -> Row {
    Row {
        phase,
        flags,
        pc,
        update,
        queued,
    }
}

fn rows(form: Form) -> Vec<Row> {
    use Flags::{Full, HighOld, Low, Old, Sized};
    use InstructionPhase::{
        ExtensionFirst as E4, ExtensionSecond as E6, OperandRefill as F, OperandWriteHigh as WH,
        OperandWriteLow as WL,
    };
    let long = form.size == Size::Long;
    let flags = if long { Full } else { Sized };
    if form.direct() {
        return vec![row(F, flags, 4, 0, true)];
    }
    let mut result = match form.family {
        Family::Displacement | Family::Indexed => vec![row(E4, Old, 4, 0, false)],
        Family::AbsoluteWord => vec![row(E4, if long { Low } else { Sized }, 4, 0, false)],
        Family::AbsoluteLong => vec![
            row(E4, Old, 2, 0, false),
            row(E6, if long { Low } else { Sized }, 6, 0, false),
        ],
        _ => vec![],
    };
    if form.family == Family::Pre {
        if long {
            result.extend([
                row(F, Low, 4, 0, false),
                row(WL, Full, 4, 0, false),
                row(WH, Full, 4, -4, true),
            ]);
        } else {
            result.extend([row(F, Sized, 4, 0, false), row(WH, Sized, 4, -2, true)]);
        }
        return result;
    }
    let pc = if form.family == Family::AbsoluteLong {
        6
    } else {
        4
    };
    let first = if !long {
        Sized
    } else {
        match form.family {
            Family::Indirect | Family::Post => Old,
            Family::Displacement | Family::Indexed => HighOld,
            _ => Full,
        }
    };
    result.push(row(WH, first, pc, 0, false));
    if long {
        result.push(row(
            WL,
            if matches!(form.family, Family::AbsoluteWord | Family::AbsoluteLong) {
                Full
            } else {
                Low
            },
            pc,
            0,
            false,
        ));
    }
    result.push(row(
        F,
        flags,
        form.length() + 2,
        if form.family == Family::Post {
            if long { 4 } else { 2 }
        } else {
            0
        },
        true,
    ));
    result
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    pub access: BusAccess,
    pub value: Option<u16>,
    pub phase: InstructionPhase,
    pub registers: Registers,
    pub internal_pc: u32,
    pub ird: u16,
}

pub(super) fn flags(sr: u16, size: Size, value: u32) -> u16 {
    let (mask, sign) = match size {
        Size::Byte => (255, 128),
        Size::Word => (65535, 32768),
        Size::Long => (u32::MAX, 0x8000_0000),
    };
    (sr & !15)
        | if value & mask == 0 {
            4
        } else if value & sign != 0 {
            8
        } else {
            0
        }
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

pub(super) fn plan(case: Case, words: &BTreeMap<u32, u16>) -> Vec<Transfer> {
    let ea = case.effective(sample(words, case.r.pc.wrapping_add(4)));
    let next = if case.form.length() == 2 {
        case.e0
    } else {
        sample(words, case.r.pc.wrapping_add(case.form.length()))
    };
    let value = case.value();
    rows(case.form)
        .into_iter()
        .map(|row| {
            let mut registers = case.r;
            let sr = case.r.status_register();
            registers.set_status_register(match row.flags {
                Flags::Old => sr,
                Flags::Sized => flags(sr, case.form.size, value),
                Flags::Low => flags(sr, Size::Word, value),
                Flags::Full => flags(sr, Size::Long, value),
                Flags::HighOld => {
                    (sr & !12)
                        | if value & 0x8000_0000 != 0 { 8 } else { 0 }
                        | if sr & 4 != 0 && value >> 16 == 0 {
                            4
                        } else {
                            0
                        }
                }
            });
            if case.form.direct() {
                let old = registers.d[usize::from(case.destination)];
                registers.d[usize::from(case.destination)] = match case.form.size {
                    Size::Byte => old & !255 | value & 255,
                    Size::Word => old & !65535 | value & 65535,
                    Size::Long => value,
                };
            } else if row.update != 0 {
                registers.set_address_register(
                    usize::from(case.destination),
                    case.r
                        .address_register(usize::from(case.destination))
                        .unwrap()
                        .wrapping_add_signed(row.update),
                );
            }
            let address = match row.phase {
                InstructionPhase::ExtensionFirst => case.r.pc.wrapping_add(4),
                InstructionPhase::ExtensionSecond => case.r.pc.wrapping_add(6),
                InstructionPhase::OperandRefill => case.r.pc.wrapping_add(case.form.length() + 2),
                InstructionPhase::OperandWriteLow => ea.wrapping_add(2),
                _ => ea,
            };
            let write = matches!(
                row.phase,
                InstructionPhase::OperandWriteHigh | InstructionPhase::OperandWriteLow
            );
            Transfer {
                access: access(
                    address,
                    case.r.supervisor(),
                    !write,
                    if write {
                        AccessKind::Data
                    } else {
                        AccessKind::Prefetch
                    },
                ),
                value: write.then_some(
                    if case.form.size == Size::Word
                        || row.phase == InstructionPhase::OperandWriteLow
                    {
                        value as u16
                    } else {
                        (value >> 16) as u16
                    },
                ),
                phase: row.phase,
                registers,
                internal_pc: case.r.pc.wrapping_add(row.pc),
                ird: if row.queued {
                    next
                } else {
                    case.form.opcode(case.destination, case.source)
                },
            }
        })
        .collect()
}

pub(super) fn clocks(case: Case, elapsed: &[u32; 5], count: usize) -> u64 {
    case.form.idle() + elapsed[..count].iter().map(|n| u64::from(*n)).sum::<u64>()
}

pub(super) fn events(case: Case, transfers: &[Transfer], elapsed: &[u32; 5]) -> Vec<Event> {
    let mut result = Vec::new();
    let mut clock = case.start;
    if case.form.idle() != 0 {
        result.push(Event::Idle(clock, 2));
        clock += 2;
    }
    for (index, t) in transfers.iter().enumerate() {
        result.push(match t.value {
            Some(v) => Event::Write(t.access, v, clock),
            None => Event::Read(t.access, clock),
        });
        clock += u64::from(elapsed[index]);
    }
    result
}

pub(super) fn memory(mut words: BTreeMap<u32, u16>, transfers: &[Transfer]) -> BTreeMap<u32, u16> {
    for t in transfers {
        if let Some(v) = t.value {
            words.insert(t.access.bus_address, v);
        }
    }
    words
}

pub(super) fn fetched(words: BTreeMap<u32, u16>, transfers: &[Transfer]) -> u16 {
    let at = transfers
        .iter()
        .position(|t| t.phase == InstructionPhase::OperandRefill)
        .unwrap();
    sample(
        &memory(words, &transfers[..at]),
        transfers[at].access.logical_address,
    )
}

pub(super) fn capture(t: Transfer, vector: u8) -> Group0Capture {
    Group0Capture {
        vector,
        internal_pc: t.internal_pc,
        phase_sr: t.registers.status_register(),
        ird: t.ird,
        logical_address: t.access.logical_address,
        function_code: t.access.function_code,
        direction: direction(t),
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    }
}

pub(super) fn direction(t: Transfer) -> Direction {
    if t.value.is_some() {
        Direction::Write
    } else {
        Direction::Read
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

pub(super) fn frame(bus: &Device, sp: u32, c: Group0Capture, start: u64) {
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
