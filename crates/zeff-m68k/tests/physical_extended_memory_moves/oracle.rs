use super::*;

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

pub(super) fn source_address(case: Case, words: &BTreeMap<u32, u16>) -> u32 {
    let e = case.e0();
    let base = match case.form.source {
        Source::PcDisplacement | Source::PcIndexed => case.r.pc.wrapping_add(2),
        _ => case.r.address_register(usize::from(case.source)).unwrap(),
    };
    match case.form.source {
        Source::Indirect | Source::Post => base,
        Source::Pre => base.wrapping_sub(case.form.stride(case.source)),
        Source::Displacement | Source::PcDisplacement => {
            base.wrapping_add_signed(i32::from(e as i16))
        }
        Source::Indexed | Source::PcIndexed => {
            let index = usize::from(e >> 12 & 7);
            let raw = if e & 0x8000 == 0 {
                case.r.d[index]
            } else {
                case.r.address_register(index).unwrap()
            };
            let offset = if e & 0x800 == 0 {
                i32::from(raw as i16) as u32
            } else {
                raw
            };
            base.wrapping_add(offset)
                .wrapping_add_signed(i32::from(e as u8 as i8))
        }
        Source::AbsoluteWord => i32::from(e as i16) as u32,
        Source::AbsoluteLong => {
            u32::from(e) << 16 | u32::from(sample(words, case.r.pc.wrapping_add(4)))
        }
    }
}

pub(super) fn value(case: Case, words: &BTreeMap<u32, u16>) -> u32 {
    let address = source_address(case, words);
    let high = sample(words, address);
    match case.form.size {
        Size::Byte if address & 1 == 0 => u32::from(high >> 8),
        Size::Byte => u32::from(high & 255),
        Size::Word => u32::from(high),
        Size::Long => u32::from(high) << 16 | u32::from(sample(words, address.wrapping_add(2))),
    }
}

pub(super) fn destination_extension(case: Case, words: &BTreeMap<u32, u16>) -> u16 {
    if case.form.extensions() == 0 {
        case.e0()
    } else {
        sample(words, case.r.pc.wrapping_add(case.form.cursor()))
    }
}

pub(super) fn destination_address(case: Case, words: &BTreeMap<u32, u16>) -> u32 {
    let e = destination_extension(case, words);
    let r = case.after_source();
    let base = r.address_register(usize::from(case.destination)).unwrap();
    match case.form.destination {
        Destination::Displacement => base.wrapping_add_signed(i32::from(e as i16)),
        Destination::Indexed => {
            let selector = usize::from(e >> 12 & 7);
            let raw = if e & 0x8000 != 0 {
                r.address_register(selector).unwrap()
            } else {
                r.d[selector]
            };
            let index = if e & 0x800 != 0 {
                raw
            } else {
                i32::from(raw as i16) as u32
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(e as u8 as i8))
        }
        Destination::AbsoluteWord => i32::from(e as i16) as u32,
        Destination::AbsoluteLong => {
            u32::from(e) << 16
                | u32::from(sample(
                    words,
                    case.r.pc.wrapping_add(case.form.cursor() + 2),
                ))
        }
    }
}

pub(super) fn next(case: Case, words: &BTreeMap<u32, u16>) -> u16 {
    let transfers = plan(case, words);
    let at = if case.form.destination == Destination::AbsoluteLong {
        transfers.len() - 2
    } else {
        case.form.reads()
    };
    let shadow = memory(words.clone(), &transfers[..at]);
    sample(&shadow, case.r.pc.wrapping_add(case.form.length()))
}

pub(super) fn plan(case: Case, words: &BTreeMap<u32, u16>) -> Vec<Transfer> {
    use InstructionPhase::{
        ExtensionFirst as E4, ExtensionSecond as E6, OperandReadHigh as RH, OperandReadLow as RL,
        OperandRefill as F, OperandWriteHigh as WH, OperandWriteLow as WL,
    };
    let opcode = case.form.opcode(case.source, case.destination);
    let mut r = case.r;
    let sr = r.status_register();
    let mut result = Vec::new();
    for index in 0..case.form.extensions() {
        result.push(Transfer {
            access: access(
                case.r.pc.wrapping_add(4 + index as u32 * 2),
                r.supervisor(),
                true,
                AccessKind::Prefetch,
            ),
            value: None,
            phase: if index == 0 { E4 } else { E6 },
            registers: r,
            internal_pc: case.r.pc.wrapping_add(if index == 1 {
                6
            } else if case.form.source == Source::AbsoluteWord {
                4
            } else {
                2
            }),
            ird: opcode,
        });
    }
    let source = source_address(case, words);
    let source_pc = case.r.pc.wrapping_add(match case.form.source {
        Source::AbsoluteLong => 6,
        Source::AbsoluteWord => 4,
        Source::Pre if case.form.size != Size::Long => 4,
        _ => 2,
    });
    for low in 0..=usize::from(case.form.size == Size::Long) {
        if case.form.source == Source::Pre && low == 0 {
            r.set_address_register(usize::from(case.source), source);
        }
        if case.form.source == Source::Post && (case.form.size != Size::Long || low == 1) {
            let old = case.r.address_register(usize::from(case.source)).unwrap();
            r.set_address_register(
                usize::from(case.source),
                old.wrapping_add(case.form.stride(case.source)),
            );
        }
        let address = source.wrapping_add(low as u32 * 2);
        let mut a = access(
            address,
            r.supervisor(),
            matches!(case.form.source, Source::PcDisplacement | Source::PcIndexed),
            AccessKind::Data,
        );
        if case.form.size == Size::Byte {
            a.lanes = if address & 1 == 0 {
                ByteLanes::Upper
            } else {
                ByteLanes::Lower
            };
        }
        result.push(Transfer {
            access: a,
            value: None,
            phase: if low == 0 { RH } else { RL },
            registers: r,
            internal_pc: source_pc,
            ird: opcode,
        });
    }
    let long = case.form.size == Size::Long;
    let absolute_long = case.form.destination == Destination::AbsoluteLong;
    let value = value(case, words);
    let c = case.r.pc.wrapping_add(case.form.cursor());
    let destination = destination_address(case, words);
    let mut shadow = words.clone();
    r.set_status_register(if long && absolute_long {
        sr
    } else {
        flags(sr, if long { Size::Word } else { case.form.size }, value)
    });
    result.push(Transfer {
        access: access(
            c.wrapping_add(2),
            r.supervisor(),
            true,
            AccessKind::Prefetch,
        ),
        value: None,
        phase: E4,
        registers: r,
        internal_pc: c.wrapping_add(2),
        ird: opcode,
    });
    let mut retained = if absolute_long {
        None
    } else {
        Some(sample(&shadow, c.wrapping_add(2)))
    };
    for low in 0..=usize::from(long) {
        let phase = if low == 0 { WH } else { WL };
        r.set_status_register(flags(
            sr,
            if long && low == 0 && absolute_long {
                Size::Word
            } else {
                case.form.size
            },
            value,
        ));
        let address = destination.wrapping_add(low as u32 * 2);
        let mut a = access(address, r.supervisor(), false, AccessKind::Data);
        if case.form.size == Size::Byte {
            a.lanes = if address & 1 == 0 {
                ByteLanes::Upper
            } else {
                ByteLanes::Lower
            };
        }
        let word = match case.form.size {
            Size::Byte if address & 1 == 0 => (value as u16 & 255) << 8,
            Size::Byte | Size::Word => {
                value as u16
                    & if case.form.size == Size::Byte {
                        255
                    } else {
                        65535
                    }
            }
            Size::Long if low == 1 => value as u16,
            Size::Long => (value >> 16) as u16,
        };
        let t = Transfer {
            access: a,
            value: Some(word),
            phase,
            registers: r,
            internal_pc: c.wrapping_add(2),
            ird: opcode,
        };
        shadow = memory(shadow, &[t]);
        result.push(t);
    }
    if absolute_long {
        result.push(Transfer {
            access: access(
                c.wrapping_add(4),
                r.supervisor(),
                true,
                AccessKind::Prefetch,
            ),
            value: None,
            phase: E6,
            registers: r,
            internal_pc: c.wrapping_add(4),
            ird: opcode,
        });
        retained = Some(sample(&shadow, c.wrapping_add(4)));
    }
    result.push(Transfer {
        access: access(
            case.r.pc.wrapping_add(case.form.length() + 2),
            r.supervisor(),
            true,
            AccessKind::Prefetch,
        ),
        value: None,
        phase: F,
        registers: r,
        internal_pc: case.r.pc.wrapping_add(case.form.length() + 2),
        ird: retained.unwrap(),
    });
    result
}

pub(super) fn idle_clocks(case: Case, count: usize, source_idle: bool) -> u64 {
    u64::from(case.form.idle() && source_idle) * 2
        + u64::from(case.form.destination_idle() && count > case.form.reads()) * 2
}

pub(super) fn clocks(case: Case, elapsed: &[u32; 9], count: usize, idle_done: bool) -> u64 {
    idle_clocks(case, count, idle_done)
        + elapsed[..count].iter().map(|n| u64::from(*n)).sum::<u64>()
}

pub(super) fn events(
    case: Case,
    transfers: &[Transfer],
    elapsed: &[u32; 9],
    idle_done: bool,
) -> Vec<Event> {
    let mut result = Vec::new();
    let mut clock = case.start;
    if case.form.idle() && idle_done {
        result.push(Event::Idle(clock, 2));
        clock += 2;
    }
    for (index, t) in transfers.iter().enumerate() {
        if index == case.form.reads() && case.form.destination_idle() {
            result.push(Event::Idle(clock, 2));
            clock += 2;
        }
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
            let old = words.get(&t.access.bus_address).copied().unwrap_or(0);
            let mask = match t.access.lanes {
                ByteLanes::Both => 65535,
                ByteLanes::Upper => 0xff00,
                ByteLanes::Lower => 255,
            };
            words.insert(t.access.bus_address, (old & !mask) | (v & mask));
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
