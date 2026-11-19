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

fn read(
    steps: &mut Vec<Step>,
    map: &Memory,
    address: u32,
    r: Registers,
    program: bool,
) -> Result<u16, String> {
    if program && address & 1 != 0 {
        return Err("odd program read excluded".into());
    }
    let value = sample(map, address)?;
    let access = BusAccess {
        logical_address: address,
        bus_address: address & BUS_ADDRESS_MASK & !1,
        function_code: match (r.supervisor(), program) {
            (false, false) => FunctionCode::UserData,
            (true, false) => FunctionCode::SupervisorData,
            (false, true) => FunctionCode::UserProgram,
            (true, true) => FunctionCode::SupervisorProgram,
        },
        kind: if program {
            AccessKind::Prefetch
        } else {
            AccessKind::Data
        },
        lanes: if program {
            ByteLanes::Both
        } else if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
    };
    steps.push(Step::Read(access, value));
    Ok(if program {
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
            return Err("unselected CMPI source/identity".into());
        }
        let word = case.initial.prefetch[0];
        let size = word >> 6 & 3;
        let mode = word >> 3 & 7;
        let reg = usize::from(word & 7);
        if case.cohort != ["CMP.b", "CMP.w", "CMP.l"][usize::from(size)] {
            return Err("CMPI original comparison cohort differs".into());
        }
        let initial = case.initial.registers()?;
        if initial.status_register() & 0x8000 != 0 || initial.pc & 1 != 0 {
            return Err("trace or odd initial PC excluded".into());
        }
        let map = memory(&case.initial.ram)?;
        let mut expected = initial;
        let mut steps = Vec::new();
        let high = case.initial.prefetch[1];
        let first = read(&mut steps, &map, initial.pc.wrapping_add(4), initial, true)?;
        let (immediate, mut retained, mut anchor) = if size == 2 {
            (
                u32::from(high) << 16 | u32::from(first),
                read(&mut steps, &map, initial.pc.wrapping_add(6), initial, true)?,
                initial.pc.wrapping_add(6),
            )
        } else {
            (u32::from(high), first, initial.pc.wrapping_add(4))
        };
        let operand = if mode == 0 {
            if case.normalization_exclusion.is_some() {
                return Err("unexpected Dn normalization tag".into());
            }
            initial.d[reg]
        } else {
            let base = initial.address_register(reg).ok_or("address register")?;
            let increment = if reg == 7 { 2 } else { 1 };
            let address = match mode {
                2 | 3 => base,
                4 => {
                    steps.push(Step::Idle(2));
                    base.wrapping_sub(increment)
                }
                5 => {
                    let extension = retained;
                    anchor = anchor.wrapping_add(2);
                    retained = read(&mut steps, &map, anchor, initial, true)?;
                    base.wrapping_add_signed(i32::from(extension as i16))
                }
                6 => {
                    let extension = retained;
                    let index = usize::from(extension >> 12 & 7);
                    let index = if extension & 0x8000 == 0 {
                        initial.d[index]
                    } else {
                        initial.address_register(index).ok_or("index register")?
                    };
                    let index = if extension & 0x0800 == 0 {
                        (index as i16 as i32) as u32
                    } else {
                        index
                    };
                    steps.push(Step::Idle(2));
                    anchor = anchor.wrapping_add(2);
                    retained = read(&mut steps, &map, anchor, initial, true)?;
                    base.wrapping_add(index)
                        .wrapping_add_signed(i32::from(extension as u8 as i8))
                }
                7 if reg == 0 => {
                    let address = (retained as i16 as i32) as u32;
                    anchor = anchor.wrapping_add(2);
                    retained = read(&mut steps, &map, anchor, initial, true)?;
                    address
                }
                7 => {
                    let high = retained;
                    anchor = anchor.wrapping_add(2);
                    let low = read(&mut steps, &map, anchor, initial, true)?;
                    anchor = anchor.wrapping_add(2);
                    retained = read(&mut steps, &map, anchor, initial, true)?;
                    u32::from(high) << 16 | u32::from(low)
                }
                _ => return Err("unselected CMPI EA".into()),
            };
            if let Some(tag) = case.normalization_exclusion.as_deref()
                && (tag != "exception_or_low_supervisor_data"
                    || !initial.supervisor()
                    || address & BUS_ADDRESS_MASK & !1 >= 0x400)
            {
                return Err("unsupported CMPI normalization tag".into());
            }
            if mode == 3 {
                expected.set_address_register(reg, base.wrapping_add(increment));
            } else if mode == 4 {
                expected.set_address_register(reg, address);
            }
            u32::from(read(&mut steps, &map, address, initial, false)?)
        };
        let mask = [0xff, 0xffff, u32::MAX][usize::from(size)];
        let bound = [128i64, 32768, 2147483648][usize::from(size)];
        let operand = operand & mask;
        let immediate = immediate & mask;
        let difference = signed(operand, size) - signed(immediate, size);
        let result = difference as u32 & mask;
        let flags = (u16::from(signed(result, size) < 0) * 8)
            | (u16::from(result == 0) * 4)
            | (u16::from(difference < -bound || difference >= bound) * 2)
            | u16::from(operand < immediate);
        expected.set_status_register(initial.status_register() & !15 | flags);
        let fetched = read(&mut steps, &map, anchor.wrapping_add(2), expected, true)?;
        if mode == 0 && size == 2 {
            steps.push(Step::Idle(2));
        }
        expected.pc = anchor;
        let queue = [retained, fetched];
        if expected != case.expected.registers()?
            || queue != case.expected.prefetch
            || map != memory(&case.expected.ram)?
        {
            return Err("manual CMPI registers/queue/RAM differ from original".into());
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
            return Err("original CMPI phase count/clocks differ".into());
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
                return Err("original CMPI ordered phase differs".into());
            }
        }
        Ok(())
    }
}
