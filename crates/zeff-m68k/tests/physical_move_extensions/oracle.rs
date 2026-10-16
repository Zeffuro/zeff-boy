use super::*;

#[derive(Clone, Copy)]
enum Flags {
    Old,
    Sized,
    HighOld,
    Low,
    Full,
}

#[derive(Clone, Copy)]
struct Row {
    phase: InstructionPhase,
    pc: u32,
    flags: Flags,
}

fn rows(form: Form) -> Vec<Row> {
    use Flags::{Full, HighOld, Low, Old, Sized};
    use InstructionPhase::{
        ExtensionFirst as E4, ExtensionSecond as E6, OperandReadHigh as RH, OperandReadLow as RL,
        OperandRefill as F, OperandWriteHigh as WH, OperandWriteLow as WL,
    };
    let mut rows = match (form.family, form.size, form.write) {
        (Family::AbsoluteLong, _, false) | (Family::Immediate, Size::Long, false) => vec![
            Row {
                phase: E4,
                pc: 2,
                flags: Old,
            },
            Row {
                phase: E6,
                pc: 6,
                flags: Old,
            },
        ],
        (Family::AbsoluteLong, Size::Long, true) => vec![
            Row {
                phase: E4,
                pc: 2,
                flags: Old,
            },
            Row {
                phase: E6,
                pc: 6,
                flags: Low,
            },
        ],
        (Family::AbsoluteLong, _, true) => vec![
            Row {
                phase: E4,
                pc: 2,
                flags: Old,
            },
            Row {
                phase: E6,
                pc: 6,
                flags: Sized,
            },
        ],
        (Family::AbsoluteWord, Size::Long, true) => vec![Row {
            phase: E4,
            pc: 4,
            flags: Low,
        }],
        (Family::AbsoluteWord, _, true) => vec![Row {
            phase: E4,
            pc: 4,
            flags: Sized,
        }],
        (Family::AbsoluteWord | Family::Immediate, _, false) => vec![Row {
            phase: E4,
            pc: 4,
            flags: Old,
        }],
        (_, _, true) => vec![Row {
            phase: E4,
            pc: 4,
            flags: Old,
        }],
        (_, _, false) => vec![Row {
            phase: E4,
            pc: 2,
            flags: Old,
        }],
    };
    if form.memory() {
        let pc = match (form.family, form.write) {
            (Family::AbsoluteLong, _) => 6,
            (Family::AbsoluteWord, _) | (_, true) => 4,
            _ => 2,
        };
        let flags = if !form.write {
            Old
        } else if form.size != Size::Long {
            Sized
        } else if matches!(form.family, Family::Displacement | Family::Indexed) {
            HighOld
        } else {
            Full
        };
        rows.push(Row {
            phase: if form.write { WH } else { RH },
            pc,
            flags,
        });
        if form.size == Size::Long {
            rows.push(Row {
                phase: if form.write { WL } else { RL },
                pc,
                flags: if !form.write {
                    Old
                } else if matches!(form.family, Family::Displacement | Family::Indexed) {
                    Low
                } else {
                    Full
                },
            });
        }
    }
    rows.push(Row {
        phase: F,
        pc: form.length() + 2,
        flags: Sized,
    });
    rows
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

pub(super) fn high_old(sr: u16, value: u32) -> u16 {
    (sr & !12)
        | if value & 0x8000_0000 != 0 { 8 } else { 0 }
        | if sr & 4 != 0 && value >> 16 == 0 {
            4
        } else {
            0
        }
}

pub(super) fn access(
    address: u32,
    size: Size,
    supervisor: bool,
    program: bool,
    kind: AccessKind,
) -> BusAccess {
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
        lanes: if size != Size::Byte {
            ByteLanes::Both
        } else if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
    }
}

pub(super) fn sample(words: &BTreeMap<u32, u16>, address: u32) -> u16 {
    words
        .get(&(address & BUS_ADDRESS_MASK & !1))
        .copied()
        .unwrap_or(0x4e71)
}

pub(super) fn result(case: Case, words: &BTreeMap<u32, u16>) -> u32 {
    if case.form.write {
        return case.r.d[usize::from(case.dn)];
    }
    let low = sample(words, case.r.pc.wrapping_add(4));
    if case.form.family == Family::Immediate {
        return match case.form.size {
            Size::Byte => u32::from(case.e0 & 255),
            Size::Word => u32::from(case.e0),
            Size::Long => (u32::from(case.e0) << 16) | u32::from(low),
        };
    }
    let address = case.effective(low);
    let high = sample(words, address);
    match case.form.size {
        Size::Byte if address & 1 == 0 => u32::from(high >> 8),
        Size::Byte => u32::from(high & 255),
        Size::Word => u32::from(high),
        Size::Long => (u32::from(high) << 16) | u32::from(sample(words, address.wrapping_add(2))),
    }
}

pub(super) fn plan(case: Case, words: &BTreeMap<u32, u16>) -> Vec<Transfer> {
    let value = result(case, words);
    let ea = case.effective(sample(words, case.r.pc.wrapping_add(4)));
    let next = sample(words, case.r.pc.wrapping_add(case.form.length()));
    rows(case.form)
        .iter()
        .map(|row| {
            let mut registers = case.r;
            registers.set_status_register(match row.flags {
                Flags::Old => case.r.status_register(),
                Flags::Sized => flags(case.r.status_register(), case.form.size, value),
                Flags::HighOld => high_old(case.r.status_register(), value),
                Flags::Low => flags(case.r.status_register(), Size::Word, value),
                Flags::Full => flags(case.r.status_register(), Size::Long, value),
            });
            let refill = row.phase == InstructionPhase::OperandRefill;
            if refill && !case.form.write {
                let mask = match case.form.size {
                    Size::Byte => 255,
                    Size::Word => 65535,
                    Size::Long => u32::MAX,
                };
                registers.d[usize::from(case.dn)] =
                    (case.r.d[usize::from(case.dn)] & !mask) | (value & mask);
            }
            let operand = !matches!(
                row.phase,
                InstructionPhase::ExtensionFirst
                    | InstructionPhase::ExtensionSecond
                    | InstructionPhase::OperandRefill
            );
            let address = match row.phase {
                InstructionPhase::ExtensionFirst => case.r.pc.wrapping_add(4),
                InstructionPhase::ExtensionSecond => case.r.pc.wrapping_add(6),
                InstructionPhase::OperandRefill => case.r.pc.wrapping_add(case.form.length() + 2),
                InstructionPhase::OperandReadLow | InstructionPhase::OperandWriteLow => {
                    ea.wrapping_add(2)
                }
                _ => ea,
            };
            let write = matches!(
                row.phase,
                InstructionPhase::OperandWriteHigh | InstructionPhase::OperandWriteLow
            );
            let word = match case.form.size {
                Size::Byte if address & 1 == 0 => (value as u16 & 255) << 8,
                Size::Byte => value as u16 & 255,
                Size::Word => value as u16,
                Size::Long if row.phase == InstructionPhase::OperandWriteLow => value as u16,
                Size::Long => (value >> 16) as u16,
            };
            Transfer {
                access: access(
                    address,
                    if operand { case.form.size } else { Size::Word },
                    case.r.supervisor(),
                    !operand
                        || matches!(case.form.family, Family::PcDisplacement | Family::PcIndexed),
                    if operand {
                        AccessKind::Data
                    } else {
                        AccessKind::Prefetch
                    },
                ),
                value: write.then_some(word),
                phase: row.phase,
                registers,
                internal_pc: case.r.pc.wrapping_add(row.pc),
                ird: if refill {
                    next
                } else {
                    case.form.opcode(case.dn, case.an)
                },
            }
        })
        .collect()
}

pub(super) fn events(case: Case, transfers: &[Transfer], elapsed: &[u32; 5]) -> Vec<Event> {
    let mut clock = case.start;
    let mut result = Vec::new();
    if case.form.indexed() {
        result.push(Event::Idle(clock, 2));
        clock += 2;
    }
    for (index, transfer) in transfers.iter().enumerate() {
        result.push(match transfer.value {
            Some(value) => Event::Write(transfer.access, value, clock),
            None => Event::Read(transfer.access, clock),
        });
        clock += u64::from(elapsed[index]);
    }
    result
}

pub(super) fn clocks(case: Case, elapsed: &[u32; 5], count: usize) -> u64 {
    u64::from(case.form.indexed()) * 2 + elapsed[..count].iter().map(|&n| u64::from(n)).sum::<u64>()
}

pub(super) fn capture(transfer: Transfer, vector: u8) -> Group0Capture {
    Group0Capture {
        vector,
        internal_pc: transfer.internal_pc,
        phase_sr: transfer.registers.status_register(),
        ird: transfer.ird,
        logical_address: transfer.access.logical_address,
        function_code: transfer.access.function_code,
        direction: if transfer.value.is_some() {
            Direction::Write
        } else {
            Direction::Read
        },
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    }
}

pub(super) fn memory(mut words: BTreeMap<u32, u16>, transfers: &[Transfer]) -> BTreeMap<u32, u16> {
    for t in transfers {
        if let Some(value) = t.value {
            let old = words.get(&t.access.bus_address).copied().unwrap_or(0);
            let mask = match t.access.lanes {
                ByteLanes::Upper => 0xff00,
                ByteLanes::Lower => 255,
                ByteLanes::Both => 65535,
            };
            words.insert(t.access.bus_address, (old & !mask) | (value & mask));
        }
    }
    words
}

pub(super) fn entry_events(sp: u32, c: Group0Capture, start: u64) -> Vec<Event> {
    let ssw = (c.ird & 0xffe0)
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
            access(
                sp.wrapping_sub(delta),
                Size::Word,
                true,
                false,
                AccessKind::Stack,
            ),
            value,
            clock,
        ));
        clock += 4;
    }
    for address in [u32::from(c.vector) * 4, u32::from(c.vector) * 4 + 2] {
        result.push(Event::EntryRead(
            access(address, Size::Word, true, false, AccessKind::Vector),
            clock,
        ));
        clock += 4;
    }
    result.push(Event::EntryRead(
        access(0x4321_4000, Size::Word, true, true, AccessKind::Prefetch),
        clock,
    ));
    clock += 4;
    result.push(Event::Idle(clock, 2));
    clock += 2;
    result.push(Event::EntryRead(
        access(0x4321_4002, Size::Word, true, true, AccessKind::Prefetch),
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
        if let Event::EntryWrite(access, value, _) = event {
            words.insert(access.bus_address, value);
        }
    }
    words
}

pub(super) fn frame(bus: &Device, sp: u32, c: Group0Capture, start: u64) {
    let at = bus
        .events
        .iter()
        .position(|event| *event == Event::Idle(start, 4))
        .unwrap();
    assert_eq!(bus.events[at..], entry_events(sp, c, start));
    let ssw = (c.ird & 0xffe0)
        | c.function_code as u16
        | if c.direction == Direction::Read {
            16
        } else {
            0
        };
    let expected: Vec<_> = [
        ssw,
        (c.logical_address >> 16) as u16,
        c.logical_address as u16,
        c.ird,
        c.phase_sr,
        (c.internal_pc >> 16) as u16,
        c.internal_pc as u16,
    ]
    .into_iter()
    .flat_map(u16::to_be_bytes)
    .collect();
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
