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
    r: &Registers,
    size: u16,
    program: bool,
) -> Result<u16, String> {
    let value = sample(map, address)?;
    if !program && size != 0 && address & 1 != 0 {
        return Err("odd Word/Long operand excluded".into());
    }
    let lanes = if program || size != 0 {
        ByteLanes::Both
    } else if address & 1 == 0 {
        ByteLanes::Upper
    } else {
        ByteLanes::Lower
    };
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
        lanes,
    };
    steps.push(Step::Read(access, value));
    Ok(if program || size != 0 {
        value
    } else if address & 1 == 0 {
        value >> 8
    } else {
        value & 0xff
    })
}

impl Plan {
    pub fn new(case: &Case) -> Result<Self, String> {
        if !super::selected(case) || case.name.is_empty() {
            return Err("unselected TST source/identity".into());
        }
        let opcode = case.initial.prefetch[0];
        let size = opcode >> 6 & 3;
        let mode = opcode >> 3 & 7;
        let register = usize::from(opcode & 7);
        if case.cohort != ["TST.b", "TST.w", "TST.l"][usize::from(size)] {
            return Err("TST cohort differs".into());
        }
        let initial = case.initial.registers()?;
        if initial.status_register() & 0x8000 != 0 || initial.pc & 1 != 0 {
            return Err("trace or odd initial PC excluded".into());
        }
        let map = memory(&case.initial.ram)?;
        let mut expected = initial;
        let mut steps = Vec::new();
        let mut anchor = initial.pc.wrapping_add(2);
        let mut retained = case.initial.prefetch[1];
        let value = if mode == 0 {
            initial.d[register]
        } else {
            let base = initial
                .address_register(register)
                .ok_or("address register")?;
            let increment = if size == 0 && register == 7 {
                2
            } else {
                1 << size
            };
            let address = match mode {
                2 | 3 => base,
                4 => {
                    steps.push(Step::Idle(2));
                    base.wrapping_sub(increment)
                }
                5 => {
                    let extension = retained;
                    anchor = anchor.wrapping_add(2);
                    retained = read(&mut steps, &map, anchor, &initial, size, true)?;
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
                    retained = read(&mut steps, &map, anchor, &initial, size, true)?;
                    base.wrapping_add(index)
                        .wrapping_add_signed(i32::from(extension as u8 as i8))
                }
                7 if register == 0 => {
                    let address = (retained as i16 as i32) as u32;
                    anchor = anchor.wrapping_add(2);
                    retained = read(&mut steps, &map, anchor, &initial, size, true)?;
                    address
                }
                7 => {
                    let high = retained;
                    anchor = anchor.wrapping_add(2);
                    let low = read(&mut steps, &map, anchor, &initial, size, true)?;
                    anchor = anchor.wrapping_add(2);
                    retained = read(&mut steps, &map, anchor, &initial, size, true)?;
                    u32::from(high) << 16 | u32::from(low)
                }
                _ => return Err("unselected EA".into()),
            };
            if let Some(tag) = case.normalization_exclusion.as_deref()
                && (tag != "exception_or_low_supervisor_data"
                    || size != 0
                    || !initial.supervisor()
                    || address >= 0x1000)
            {
                return Err("unsupported normalization tag".into());
            }
            if mode == 4 {
                expected.set_address_register(register, address);
            }
            if mode == 3 {
                expected.set_address_register(register, base.wrapping_add(increment));
            }
            let high = u32::from(read(&mut steps, &map, address, &initial, size, false)?);
            if size == 2 {
                high << 16
                    | u32::from(read(
                        &mut steps,
                        &map,
                        address.wrapping_add(2),
                        &initial,
                        size,
                        false,
                    )?)
            } else {
                high
            }
        };
        if mode == 0 && case.normalization_exclusion.is_some() {
            return Err("unexpected Dn normalization tag".into());
        }
        let masks = [0xff, 0xffff, u32::MAX];
        let signs = [0x80, 0x8000, 0x8000_0000];
        let value = value & masks[usize::from(size)];
        let nz = if value == 0 {
            4
        } else if value & signs[usize::from(size)] != 0 {
            8
        } else {
            0
        };
        expected.set_status_register(initial.status_register() & !15 | nz);
        expected.pc = anchor;
        let fetched = read(
            &mut steps,
            &map,
            anchor.wrapping_add(2),
            &expected,
            size,
            true,
        )?;
        let queue = [retained, fetched];
        if expected != case.expected.registers()?
            || queue != case.expected.prefetch
            || map != memory(&case.expected.ram)?
        {
            return Err("manual TST registers/queue/RAM differ from original".into());
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
            return Err("original TST phase count/clocks differ".into());
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
                return Err("original TST ordered phase differs".into());
            }
        }
        Ok(())
    }
}
