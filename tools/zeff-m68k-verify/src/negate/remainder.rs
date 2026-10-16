use super::plan::{Memory, Plan, Step, memory};
use crate::reference::{Case, Phase};
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};

pub(super) fn sample(map: &Memory, access: BusAccess) -> Result<u16, String> {
    let at = access.bus_address;
    let high = if access.lanes != ByteLanes::Lower {
        u16::from(*map.get(&at).ok_or("missing selected high byte")?) << 8
    } else {
        0
    };
    let low = if access.lanes != ByteLanes::Upper {
        u16::from(*map.get(&(at + 1)).ok_or("missing selected low byte")?)
    } else {
        0
    };
    Ok(high | low)
}

pub(super) fn deposit(map: &mut Memory, access: BusAccess, value: u16) {
    if access.lanes != ByteLanes::Lower {
        map.insert(access.bus_address, (value >> 8) as u8);
    }
    if access.lanes != ByteLanes::Upper {
        map.insert(access.bus_address + 1, value as u8);
    }
}

fn access(address: u32, bank: bool, data: bool) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & BUS_ADDRESS_MASK & !1,
        function_code: match (bank, data) {
            (false, false) => FunctionCode::UserProgram,
            (false, true) => FunctionCode::UserData,
            (true, false) => FunctionCode::SupervisorProgram,
            (true, true) => FunctionCode::SupervisorData,
        },
        kind: if data {
            AccessKind::Data
        } else {
            AccessKind::Prefetch
        },
        lanes: if !data {
            ByteLanes::Both
        } else if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
    }
}

fn read(steps: &mut Vec<Step>, map: &Memory, at: BusAccess) -> Result<u16, String> {
    let value = sample(map, at)?;
    steps.push(Step::Transfer {
        access: at,
        direction: Direction::Read,
        value,
        entry: false,
    });
    Ok(value)
}

fn arithmetic(sr: u16, operand: u32, bits: u32, extend: bool) -> (u32, u16) {
    let modulus = 1u64 << bits;
    let mask = modulus - 1;
    let value = u64::from(operand) & mask;
    let x = u64::from(extend && sr & 16 != 0);
    let result = modulus.wrapping_sub(value + x) & mask;
    let borrow = u16::from(value + x != 0);
    let flags = (borrow * 17)
        | (u16::from(result & (modulus / 2) != 0) << 3)
        | (u16::from(result == 0 && (!extend || sr & 4 != 0)) << 2)
        | (u16::from(value == modulus / 2 && x == 0) << 1);
    (result as u32, (sr & !31) | flags)
}

pub(super) fn build(case: &Case) -> Result<Plan, String> {
    let initial = case.initial.registers()?;
    let word = case.initial.prefetch[0];
    let (size, mode, reg) = (word >> 6 & 3, word >> 3 & 7, usize::from(word & 7));
    if size != 0 && mode != 0 {
        return Err("unselected remaining NEG/NEGX route".into());
    }
    let bits = [8, 16, 32][usize::from(size)];
    let mask = ((1u64 << bits) - 1) as u32;
    let mut registers = initial;
    let mut map = memory(&case.initial.ram)?;
    let mut steps = Vec::new();
    let p = initial.pc;
    let bank = initial.supervisor();
    let extension = case.initial.prefetch[1];
    let count = if mode == 7 && reg == 1 {
        2
    } else {
        u32::from(mode >= 5)
    };
    let mut retained = extension;
    let mut low = 0;
    if matches!(mode, 4 | 6) {
        steps.push(Step::Idle(2));
    }
    for n in 0..count {
        retained = read(
            &mut steps,
            &map,
            access(p.wrapping_add(4 + n * 2), bank, false),
        )?;
        if n == 0 {
            low = retained;
        }
    }
    let mut operand = 0;
    let source = if mode == 0 {
        initial.d[reg] & mask
    } else {
        let base = initial.address_register(reg).ok_or("bad source An")?;
        let step = if reg == 7 { 2 } else { 1 };
        operand = match (mode, reg) {
            (2 | 3, _) => base,
            (4, _) => base.wrapping_sub(step),
            (5, _) => base.wrapping_add_signed(i32::from(extension as i16)),
            (6, _) => {
                let n = usize::from(extension >> 12 & 7);
                let index = if extension & 0x8000 == 0 {
                    initial.d[n]
                } else {
                    initial.address_register(n).ok_or("bad index An")?
                };
                let index = if extension & 0x800 == 0 {
                    index as i16 as i32 as u32
                } else {
                    index
                };
                base.wrapping_add(index)
                    .wrapping_add_signed(i32::from(extension as u8 as i8))
            }
            (7, 0) => extension as i16 as i32 as u32,
            (7, 1) => u32::from(extension) << 16 | u32::from(low),
            _ => return Err("unselected remaining Byte mode".into()),
        };
        if mode == 3 {
            registers.set_address_register(reg, operand.wrapping_add(step));
        } else if mode == 4 {
            registers.set_address_register(reg, operand);
        }
        let data = read(&mut steps, &map, access(operand, bank, true))?;
        u32::from(if operand & 1 == 0 {
            data >> 8
        } else {
            data & 255
        })
    };
    let (result, sr) = arithmetic(
        initial.status_register(),
        source,
        bits,
        word & 0xff00 == 0x4000,
    );
    registers.set_status_register(sr);
    let anchor = p.wrapping_add(2 + count * 2);
    let fetched = read(
        &mut steps,
        &map,
        access(anchor.wrapping_add(2), bank, false),
    )?;
    if mode == 0 {
        registers.d[reg] = initial.d[reg] & !mask | result;
        if size == 2 {
            steps.push(Step::Idle(2));
        }
    } else {
        let at = access(operand, bank, true);
        let value = if operand & 1 == 0 {
            (result as u16) << 8
        } else {
            result as u16
        };
        steps.push(Step::Transfer {
            access: at,
            direction: Direction::Write,
            value,
            entry: false,
        });
        deposit(&mut map, at, value);
    }
    registers.pc = anchor;
    let total = steps
        .iter()
        .map(|step| match step {
            Step::Idle(n) => u64::from(*n),
            _ => 4,
        })
        .sum();
    let plan = Plan {
        steps,
        initial,
        phase: registers,
        final_registers: registers,
        words: [retained, fetched],
        memory: map,
        odd: false,
        operand,
        capture_pc: anchor.wrapping_add(2),
        total,
    };
    validate(&plan, case)?;
    if plan.final_registers != case.expected.registers()?
        || plan.words != case.expected.prefetch
        || plan.memory != memory(&case.expected.ram)?
    {
        return Err("remaining full registers/queue/RAM differ from original result".into());
    }
    Ok(plan)
}

fn validate(plan: &Plan, case: &Case) -> Result<(), String> {
    if plan.steps.len() != case.transactions.len()
        || plan.total != u64::from(case.reference_total_cycles)
    {
        return Err("remaining phase count or clocks differ".into());
    }
    for (step, phase) in plan.steps.iter().zip(&case.transactions) {
        let good = match (step, phase) {
            (Step::Idle(clocks), Phase::Idle { duration_cycles }) => clocks == duration_cycles,
            (
                Step::Transfer {
                    access,
                    direction,
                    value,
                    entry,
                },
                raw,
            ) => {
                let data = match (direction, raw) {
                    (Direction::Read, Phase::Read(t)) | (Direction::Write, Phase::Write(t)) => {
                        Some(t)
                    }
                    _ => None,
                };
                let lanes = match access.lanes {
                    ByteLanes::Both => (1, 1),
                    ByteLanes::Upper => (1, 0),
                    ByteLanes::Lower => (0, 1),
                };
                !entry
                    && data.is_some_and(|t| {
                        t.duration_cycles == 4
                            && t.address_bus == access.bus_address
                            && t.function_code == access.function_code as u32
                            && t.data_bus == u32::from(*value)
                            && (t.uds, t.lds) == lanes
                    })
            }
            _ => false,
        };
        if !good {
            return Err(
                "remaining ordered access/data/lanes/duration differ from raw phase".into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::arithmetic;

    #[test]
    fn original_long_inputs_and_sticky_zero() {
        for (sr, value, bits, extend, result, flags) in [
            (0x2413, 0xfbfe0fbc, 32, false, 0x0401f044, 0x2411),
            (0x0216, 0xbbe0657a, 32, true, 0x441f9a85, 0x0211),
            (0x0102, 0x02e530b5, 32, true, 0xfd1acf4b, 0x0119),
            (0x0014, 255, 8, true, 0, 0x0015),
            (0x0010, 255, 8, true, 0, 0x0011),
            (0x0014, 0x8000, 16, true, 0x7fff, 0x0011),
        ] {
            assert_eq!(arithmetic(sr, value, bits, extend), (result, flags));
        }
    }
}

pub fn validate_payload(text: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != super::provenance::REMAINDER_COMPLETION_SHA256 {
        return Err(
            "remaining compact payload SHA256 differs from original-fingerprint extraction".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "remainder/tests.rs"]
mod lane_tests;
