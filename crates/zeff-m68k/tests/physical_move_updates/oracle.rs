use super::*;

#[derive(Clone, Copy)]
enum Flags {
    Old,
    Sized,
    Low,
    Full,
}

#[derive(Clone, Copy)]
struct Row {
    phase: InstructionPhase,
    address: i32,
    an: i32,
    flags: Flags,
    load: bool,
    pc: u32,
    queued: bool,
}

const STEP: i32 = 100;

const fn row(
    phase: InstructionPhase,
    address: i32,
    an: i32,
    flags: Flags,
    load: bool,
    capture: (u32, bool),
) -> Row {
    Row {
        phase,
        address,
        an,
        flags,
        load,
        pc: capture.0,
        queued: capture.1,
    }
}

fn rows(form: Form) -> Vec<Row> {
    use Flags::{Full, Low, Old, Sized};
    use InstructionPhase::{
        OperandReadHigh as RH, OperandReadLow as RL, OperandRefill as F, OperandWriteHigh as WH,
        OperandWriteLow as WL,
    };
    match (form.route, form.size) {
        (Route::PostLoad, Size::Byte | Size::Word) => vec![
            row(RH, 0, STEP, Old, false, (2, false)),
            row(F, 0, STEP, Sized, true, (4, true)),
        ],
        (Route::PostLoad, Size::Long) => vec![
            row(RH, 0, 0, Old, false, (2, false)),
            row(RL, 2, 4, Old, false, (2, false)),
            row(F, 0, 4, Full, true, (4, true)),
        ],
        (Route::PreLoad, Size::Byte | Size::Word) => vec![
            row(RH, -STEP, -STEP, Old, false, (4, false)),
            row(F, 0, -STEP, Sized, true, (4, true)),
        ],
        (Route::PreLoad, Size::Long) => vec![
            row(RH, -4, -4, Old, false, (2, false)),
            row(RL, -2, -4, Old, false, (2, false)),
            row(F, 0, -4, Full, true, (4, true)),
        ],
        (Route::PostStore, Size::Byte | Size::Word) => vec![
            row(WH, 0, 0, Sized, false, (4, false)),
            row(F, 0, STEP, Sized, false, (4, true)),
        ],
        (Route::PostStore, Size::Long) => vec![
            row(WH, 0, 0, Old, false, (4, false)),
            row(WL, 2, 0, Low, false, (4, false)),
            row(F, 0, 4, Full, false, (4, true)),
        ],
        (Route::PreStore, Size::Byte | Size::Word) => vec![
            row(F, 0, 0, Sized, false, (4, false)),
            row(WH, -STEP, -STEP, Sized, false, (4, true)),
        ],
        (Route::PreStore, Size::Long) => vec![
            row(F, 0, 0, Low, false, (4, false)),
            row(WL, -2, 0, Full, false, (4, false)),
            row(WH, -4, -4, Full, false, (4, true)),
        ],
    }
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

pub(super) fn loaded(old: u32, size: Size, value: u32) -> u32 {
    match size {
        Size::Byte => (old & !255) | (value & 255),
        Size::Word => (old & !65535) | (value & 65535),
        Size::Long => value,
    }
}

pub(super) fn access(address: u32, size: Size, supervisor: bool, data: bool) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & BUS_ADDRESS_MASK & !1,
        function_code: match (data, supervisor) {
            (true, false) => FunctionCode::UserData,
            (true, true) => FunctionCode::SupervisorData,
            (false, false) => FunctionCode::UserProgram,
            (false, true) => FunctionCode::SupervisorProgram,
        },
        kind: if data {
            AccessKind::Data
        } else {
            AccessKind::Prefetch
        },
        lanes: if size != Size::Byte {
            ByteLanes::Both
        } else if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
    }
}

pub(super) fn plan(
    form: Form,
    initial: Registers,
    dn: u16,
    an: u16,
    value: u32,
    queued: u16,
) -> Vec<Transfer> {
    let old_address = initial.address_register(usize::from(an)).unwrap();
    let step = form.stride(an) as i32;
    let delta = |n| match n {
        STEP => step,
        n if n == -STEP => -step,
        n => n,
    };
    rows(form)
        .iter()
        .map(|row| {
            let refill = row.phase == InstructionPhase::OperandRefill;
            let address = if refill {
                initial.pc.wrapping_add(4)
            } else {
                old_address.wrapping_add_signed(delta(row.address))
            };
            let mut registers = initial;
            registers.set_address_register(
                usize::from(an),
                old_address.wrapping_add_signed(delta(row.an)),
            );
            registers.set_status_register(match row.flags {
                Flags::Old => initial.status_register(),
                Flags::Sized => flags(initial.status_register(), form.size, value),
                Flags::Low => flags(initial.status_register(), Size::Word, value),
                Flags::Full => flags(initial.status_register(), Size::Long, value),
            });
            if row.load {
                registers.d[usize::from(dn)] = loaded(initial.d[usize::from(dn)], form.size, value);
            }
            let write = matches!(
                row.phase,
                InstructionPhase::OperandWriteHigh | InstructionPhase::OperandWriteLow
            );
            let word = match form.size {
                Size::Byte if address & 1 == 0 => (value as u16 & 255) << 8,
                Size::Byte => value as u16 & 255,
                Size::Word => value as u16,
                Size::Long if row.phase == InstructionPhase::OperandWriteLow => value as u16,
                Size::Long => (value >> 16) as u16,
            };
            Transfer {
                access: access(
                    address,
                    if refill { Size::Word } else { form.size },
                    initial.supervisor(),
                    !refill,
                ),
                value: write.then_some(word),
                phase: row.phase,
                registers,
                internal_pc: initial.pc.wrapping_add(row.pc),
                ird: if row.queued {
                    queued
                } else {
                    form.opcode(dn, an)
                },
            }
        })
        .collect()
}

pub(super) fn events(
    form: Form,
    transfers: &[Transfer],
    elapsed: &[u32; 3],
    start: u64,
) -> Vec<Event> {
    let mut events = Vec::new();
    let mut clock = start;
    if form.idle() != 0 {
        events.push(Event::Idle(clock, form.idle()));
        clock += u64::from(form.idle());
    }
    for (index, transfer) in transfers.iter().enumerate() {
        events.push(match transfer.value {
            Some(value) => Event::Write(transfer.access, value, clock),
            None => Event::Read(transfer.access, clock),
        });
        clock += u64::from(elapsed[index]);
    }
    events
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
    for transfer in transfers {
        if let Some(value) = transfer.value {
            let address = transfer.access.bus_address;
            let old = words.get(&address).copied().unwrap_or(0);
            let mask = match transfer.access.lanes {
                ByteLanes::Upper => 0xff00,
                ByteLanes::Lower => 255,
                ByteLanes::Both => 65535,
            };
            words.insert(address, (old & !mask) | (value & mask));
        }
    }
    words
}

pub(super) fn fetched(words: BTreeMap<u32, u16>, transfers: &[Transfer]) -> u16 {
    let at = transfers
        .iter()
        .position(|t| t.phase == InstructionPhase::OperandRefill)
        .unwrap();
    memory(words, &transfers[..at])[&transfers[at].access.bus_address]
}

pub(super) fn entry_events(sp: u32, capture: Group0Capture, start: u64) -> Vec<Event> {
    let ssw = (capture.ird & 0xffe0)
        | capture.function_code as u16
        | if capture.direction == Direction::Read {
            16
        } else {
            0
        };
    let mut clock = start + 4;
    let mut result = vec![Event::Idle(start, 4)];
    for (offset, value) in [
        (2, capture.internal_pc as u16),
        (6, capture.phase_sr),
        (4, (capture.internal_pc >> 16) as u16),
        (8, capture.ird),
        (10, capture.logical_address as u16),
        (14, ssw),
        (12, (capture.logical_address >> 16) as u16),
    ] {
        let mut access = access(sp.wrapping_sub(offset), Size::Word, true, true);
        access.kind = AccessKind::Stack;
        result.push(Event::EntryWrite(access, value, clock));
        clock += 4;
    }
    for address in [
        u32::from(capture.vector) * 4,
        u32::from(capture.vector) * 4 + 2,
    ] {
        let mut access = access(address, Size::Word, true, true);
        access.kind = AccessKind::Vector;
        result.push(Event::EntryRead(access, clock));
        clock += 4;
    }
    result.push(Event::EntryRead(
        access(0x4321_4000, Size::Word, true, false),
        clock,
    ));
    clock += 4;
    result.push(Event::Idle(clock, 2));
    clock += 2;
    result.push(Event::EntryRead(
        access(0x4321_4002, Size::Word, true, false),
        clock,
    ));
    result
}

pub(super) fn frame(device: &Device, sp: u32, capture: Group0Capture, start: u64) {
    let expected = entry_events(sp, capture, start);
    let at = device
        .events
        .iter()
        .position(|event| *event == Event::Idle(start, 4))
        .unwrap();
    assert_eq!(device.events[at..], expected);
    let ssw = (capture.ird & 0xffe0)
        | capture.function_code as u16
        | if capture.direction == Direction::Read {
            16
        } else {
            0
        };
    let expected: Vec<_> = [
        ssw,
        (capture.logical_address >> 16) as u16,
        capture.logical_address as u16,
        capture.ird,
        capture.phase_sr,
        (capture.internal_pc >> 16) as u16,
        capture.internal_pc as u16,
    ]
    .into_iter()
    .flat_map(u16::to_be_bytes)
    .collect();
    let actual: Vec<_> = (0..7)
        .flat_map(|index| {
            device
                .sample(sp.wrapping_sub(14).wrapping_add(index * 2))
                .to_be_bytes()
        })
        .collect();
    assert_eq!(actual, expected);
}

pub(super) fn entered(mut registers: Registers) -> Registers {
    registers.ssp = registers.ssp.wrapping_sub(14);
    registers.pc = 0x4321_4000;
    registers.set_status_register(registers.status_register() | SUPERVISOR);
    registers
}

pub(super) fn entry_memory(
    mut words: BTreeMap<u32, u16>,
    sp: u32,
    capture: Group0Capture,
) -> BTreeMap<u32, u16> {
    for event in entry_events(sp, capture, 0) {
        if let Event::EntryWrite(access, value, _) = event {
            words.insert(access.bus_address, value);
        }
    }
    words
}

pub(super) fn assert_operand_memory(
    device: &Device,
    expected: &BTreeMap<u32, u16>,
    transfers: &[Transfer],
) {
    for transfer in transfers
        .iter()
        .filter(|t| t.access.kind == AccessKind::Data)
    {
        assert_eq!(
            device.words.get(&transfer.access.bus_address),
            expected.get(&transfer.access.bus_address)
        );
    }
}
