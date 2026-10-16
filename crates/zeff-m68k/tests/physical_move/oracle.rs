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
    let address = initial.address_register(usize::from(an)).unwrap();
    let opcode = form.opcode(dn, an);
    let mut phase_registers = initial;
    let mut transfers = Vec::new();
    if form.write && form.size != Size::Long {
        phase_registers.set_status_register(flags(initial.status_register(), form.size, value));
    }
    let word = if form.size == Size::Byte {
        if address & 1 == 0 {
            (value as u16 & 255) << 8
        } else {
            value as u16 & 255
        }
    } else if form.size == Size::Long {
        (value >> 16) as u16
    } else {
        value as u16
    };
    transfers.push(Transfer {
        access: access(address, form.size, initial.supervisor(), true),
        value: form.write.then_some(word),
        phase: if form.write {
            InstructionPhase::OperandWriteHigh
        } else {
            InstructionPhase::OperandReadHigh
        },
        registers: phase_registers,
        internal_pc: initial.pc.wrapping_add(if form.write { 4 } else { 2 }),
        ird: opcode,
    });
    if form.size == Size::Long {
        if form.write {
            phase_registers.set_status_register(flags(
                initial.status_register(),
                Size::Word,
                value,
            ));
        }
        transfers.push(Transfer {
            access: access(
                address.wrapping_add(2),
                Size::Long,
                initial.supervisor(),
                true,
            ),
            value: form.write.then_some(value as u16),
            phase: if form.write {
                InstructionPhase::OperandWriteLow
            } else {
                InstructionPhase::OperandReadLow
            },
            registers: phase_registers,
            internal_pc: initial.pc.wrapping_add(if form.write { 4 } else { 2 }),
            ird: opcode,
        });
    }
    phase_registers.set_status_register(flags(initial.status_register(), form.size, value));
    if !form.write {
        phase_registers.d[usize::from(dn)] = loaded(initial.d[usize::from(dn)], form.size, value);
    }
    transfers.push(Transfer {
        access: access(
            initial.pc.wrapping_add(4),
            Size::Word,
            initial.supervisor(),
            false,
        ),
        value: None,
        phase: InstructionPhase::OperandRefill,
        registers: phase_registers,
        internal_pc: initial.pc.wrapping_add(4),
        ird: queued,
    });
    transfers
}

pub(super) fn events(plan: &[Transfer], elapsed: &[u32; 3], start: u64) -> Vec<Event> {
    let mut clock = start;
    plan.iter()
        .enumerate()
        .map(|(index, transfer)| {
            let event = match transfer.value {
                Some(value) => Event::Write(transfer.access, value, clock),
                None => Event::Read(transfer.access, clock),
            };
            clock += u64::from(elapsed[index]);
            event
        })
        .collect()
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

pub(super) fn frame(device: &Device, sp: u32, captured: Group0Capture) {
    let ssw = captured.ird & 0xffe0
        | captured.function_code as u16
        | if captured.direction == Direction::Read {
            16
        } else {
            0
        };
    let expected: Vec<_> = [
        ssw,
        (captured.logical_address >> 16) as u16,
        captured.logical_address as u16,
        captured.ird,
        captured.phase_sr,
        (captured.internal_pc >> 16) as u16,
        captured.internal_pc as u16,
    ]
    .into_iter()
    .flat_map(u16::to_be_bytes)
    .collect();
    let actual: Vec<_> = (0..7)
        .flat_map(|index| {
            device.words[&(sp.wrapping_sub(14).wrapping_add(index * 2) & BUS_ADDRESS_MASK)]
                .to_be_bytes()
        })
        .collect();
    assert_eq!(actual, expected);
    let expected: Vec<_> = [
        (2, captured.internal_pc as u16),
        (6, captured.phase_sr),
        (4, (captured.internal_pc >> 16) as u16),
        (8, captured.ird),
        (10, captured.logical_address as u16),
        (14, ssw),
        (12, (captured.logical_address >> 16) as u16),
    ]
    .into_iter()
    .map(|(offset, value)| (sp.wrapping_sub(offset), value))
    .collect();
    let actual: Vec<_> = device
        .events
        .iter()
        .filter_map(|event| match event {
            Event::EntryWrite(access, value, _) => {
                assert_eq!(access.function_code, FunctionCode::SupervisorData);
                assert_eq!(access.kind, AccessKind::Stack);
                assert_eq!(access.lanes, ByteLanes::Both);
                Some((access.logical_address, *value))
            }
            _ => None,
        })
        .collect();
    assert_eq!(actual, expected);
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
