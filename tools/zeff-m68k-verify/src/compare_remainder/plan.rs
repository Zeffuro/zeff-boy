use crate::reference::{Case, Phase};
use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, FunctionCode};

pub(super) type Memory = BTreeMap<u32, u8>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Idle(u32),
    Read(BusAccess, u16),
}

#[derive(Clone, Copy)]
pub(super) enum ReadKind {
    Prefetch,
    DataByte,
    ProgramByte,
}

pub(super) struct Plan {
    pub initial: Registers,
    pub expected: Registers,
    pub queue: [u16; 2],
    pub memory: Memory,
    pub steps: Vec<Step>,
    pub clocks: u64,
}

fn memory(bytes: &[(u32, u8)]) -> Result<Memory, String> {
    let mut map = Memory::new();
    for &(address, byte) in bytes {
        if address > BUS_ADDRESS_MASK || map.insert(address, byte).is_some() {
            return Err("duplicate or out-of-range RAM byte".into());
        }
    }
    Ok(map)
}

pub(super) fn sample(map: &Memory, address: u32) -> Result<u16, String> {
    let address = address & BUS_ADDRESS_MASK & !1;
    Ok(
        u16::from(*map.get(&address).ok_or("missing high RAM byte")?) << 8
            | u16::from(*map.get(&(address + 1)).ok_or("missing low RAM byte")?),
    )
}

pub(super) fn read(
    steps: &mut Vec<Step>,
    map: &Memory,
    address: u32,
    registers: Registers,
    kind: ReadKind,
) -> Result<u16, String> {
    let prefetch = matches!(kind, ReadKind::Prefetch);
    let byte = !prefetch;
    let program = !matches!(kind, ReadKind::DataByte);
    if !byte && address & 1 != 0 {
        return Err("odd word read excluded".into());
    }
    let value = sample(map, address)?;
    steps.push(Step::Read(
        BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK & !1,
            function_code: match (registers.supervisor(), program) {
                (false, false) => FunctionCode::UserData,
                (true, false) => FunctionCode::SupervisorData,
                (false, true) => FunctionCode::UserProgram,
                (true, true) => FunctionCode::SupervisorProgram,
            },
            kind: if prefetch {
                AccessKind::Prefetch
            } else {
                AccessKind::Data
            },
            lanes: if !byte {
                ByteLanes::Both
            } else if address & 1 == 0 {
                ByteLanes::Upper
            } else {
                ByteLanes::Lower
            },
        },
        value,
    ));
    Ok(if !byte {
        value
    } else if address & 1 == 0 {
        value >> 8
    } else {
        value & 0xff
    })
}

fn signed(value: u32, size: u16) -> i64 {
    match size {
        0 => i64::from(value as u8 as i8),
        1 => i64::from(value as u16 as i16),
        _ => i64::from(value as i32),
    }
}

impl Plan {
    pub fn new(case: &Case) -> Result<Self, String> {
        if !super::selected(case) || case.name.is_empty() {
            return Err("unselected comparison source/identity".into());
        }
        let opcode = case.initial.prefetch[0];
        let opmode = opcode >> 6 & 7;
        let address_destination = matches!(opmode, 3 | 7);
        let source_size = match opmode {
            0 => 0,
            1 | 3 => 1,
            _ => 2,
        };
        let cohort = match opmode {
            0 => "CMP.b",
            1 => "CMP.w",
            2 => "CMP.l",
            3 => "CMPA.w",
            _ => "CMPA.l",
        };
        if case.cohort != cohort {
            return Err("original comparison cohort differs".into());
        }
        let initial = case.initial.registers()?;
        if initial.status_register() & 0x8000 != 0 || initial.pc & 1 != 0 {
            return Err("trace or odd initial PC excluded".into());
        }
        let map = memory(&case.initial.ram)?;
        let mut expected = initial;
        let mut steps = Vec::new();
        let source = super::source::operand(
            source_size,
            case.initial.prefetch,
            initial,
            &mut expected,
            &map,
            &mut steps,
        )?;
        if let Some(tag) = case.normalization_exclusion.as_deref()
            && (tag != "exception_or_low_supervisor_data"
                || !initial.supervisor()
                || source
                    .address
                    .is_none_or(|ea| ea & BUS_ADDRESS_MASK & !1 >= 0x400))
        {
            return Err("unsupported comparison normalization tag".into());
        }
        let size = if address_destination { 2 } else { source_size };
        let mask = [0xff, 0xffff, u32::MAX][usize::from(size)];
        let bound = [128i64, 32768, 2147483648][usize::from(size)];
        let destination_register = usize::from(opcode >> 9 & 7);
        let destination = if address_destination {
            expected.address_register(destination_register).unwrap()
        } else {
            initial.d[destination_register]
        } & mask;
        let value = if address_destination && source_size == 1 {
            (source.value as i16 as i32) as u32
        } else {
            source.value
        } & mask;
        let difference = signed(destination, size) - signed(value, size);
        let result = difference as u32 & mask;
        let flags = (u16::from(signed(result, size) < 0) * 8)
            | (u16::from(result == 0) * 4)
            | (u16::from(difference < -bound || difference >= bound) * 2)
            | u16::from(destination < value);
        expected.set_status_register(initial.status_register() & !15 | flags);
        let fetched = read(
            &mut steps,
            &map,
            source.anchor.wrapping_add(2),
            expected,
            ReadKind::Prefetch,
        )?;
        if address_destination || source_size == 2 {
            steps.push(Step::Idle(2));
        }
        expected.pc = source.anchor;
        let queue = [source.retained, fetched];
        if expected != case.expected.registers()?
            || queue != case.expected.prefetch
            || map != memory(&case.expected.ram)?
        {
            return Err("manual comparison registers/queue/RAM differ from original".into());
        }
        let clocks = steps
            .iter()
            .map(|s| match s {
                Step::Idle(n) => u64::from(*n),
                Step::Read(..) => 4,
            })
            .sum();
        let plan = Self {
            initial,
            expected,
            queue,
            memory: map,
            steps,
            clocks,
        };
        plan.validate_phases(case)?;
        Ok(plan)
    }

    fn validate_phases(&self, case: &Case) -> Result<(), String> {
        if self.clocks != u64::from(case.reference_total_cycles)
            || self.steps.len() != case.transactions.len()
        {
            return Err("original comparison phase count/clocks differ".into());
        }
        for (step, phase) in self.steps.iter().zip(&case.transactions) {
            let matches = match (step, phase) {
                (Step::Idle(n), Phase::Idle { duration_cycles }) => n == duration_cycles,
                (Step::Read(access, value), Phase::Read(raw)) => {
                    let (uds, lds, mask) = match access.lanes {
                        ByteLanes::Both => (1, 1, 0xffff),
                        ByteLanes::Upper => (1, 0, 0xff00),
                        ByteLanes::Lower => (0, 1, 0xff),
                    };
                    raw.duration_cycles == 4
                        && raw.address_bus == access.bus_address
                        && raw.function_code == access.function_code as u32
                        && (raw.uds, raw.lds) == (uds, lds)
                        && raw.data_bus == u32::from(*value) & mask
                }
                _ => false,
            };
            if !matches {
                return Err("original comparison ordered phase differs".into());
            }
        }
        Ok(())
    }
}
