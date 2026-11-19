use std::collections::BTreeMap;

use crate::reference::{Case, Phase};
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};

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

pub(super) fn memory(bytes: &[(u32, u8)]) -> Result<Memory, String> {
    let mut map = Memory::new();
    for &(address, byte) in bytes {
        if address > 0x00ff_ffff || map.insert(address, byte).is_some() {
            return Err("duplicate or out-of-range RAM byte".into());
        }
    }
    Ok(map)
}

pub(super) fn sample(map: &Memory, address: u32) -> Result<u16, String> {
    let at = address & 0x00ff_fffe;
    let high = map.get(&at).ok_or("missing high RAM byte")?;
    let low = map.get(&(at + 1)).ok_or("missing low RAM byte")?;
    Ok(u16::from(*high) << 8 | u16::from(*low))
}

fn read(steps: &mut Vec<Step>, map: &Memory, address: u32, sr: u16) -> Result<u16, String> {
    let value = sample(map, address)?;
    let access = BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_fffe,
        function_code: if sr & 0x2000 == 0 {
            FunctionCode::UserProgram
        } else {
            FunctionCode::SupervisorProgram
        },
        kind: AccessKind::Prefetch,
        lanes: ByteLanes::Both,
    };
    steps.push(Step::Read(access, value));
    Ok(value)
}

impl Plan {
    pub fn new(case: &Case) -> Result<Self, String> {
        let word = case.initial.prefetch[0];
        let family = word & 0xffc0;
        let immediate = word & 63 == 60;
        if !matches!(family, 0x44c0 | 0x46c0)
            || !(word & 63 < 8 || immediate)
            || case.cohort
                != if family == 0x44c0 {
                    "MOVEtoCCR"
                } else {
                    "MOVEtoSR"
                }
            || case.name.is_empty()
        {
            return Err("unselected status source/cohort/identity".into());
        }
        let initial = case.initial.registers()?;
        if initial.status_register() & 0x8000 != 0 {
            return Err("initial trace excluded".into());
        }
        if family == 0x46c0 && !initial.supervisor() {
            return Err("user SR privilege excluded".into());
        }
        if initial.pc & 1 != 0 || case.normalization_exclusion.is_some() {
            return Err("unselected imported boundary".into());
        }
        let map = memory(&case.initial.ram)?;
        let value = if immediate {
            case.initial.prefetch[1]
        } else {
            initial.d[usize::from(word & 7)] as u16
        };
        let old_sr = initial.status_register();
        let sr = if family == 0x44c0 {
            old_sr & 0xffe0 | value & 31
        } else {
            value & 0xa71f
        };
        let mut steps = Vec::new();
        let anchor = initial.pc.wrapping_add(if immediate { 4 } else { 2 });
        if immediate {
            read(&mut steps, &map, anchor, old_sr)?;
        }
        steps.push(Step::Idle(2));
        steps.push(Step::Idle(2));
        let first = read(&mut steps, &map, anchor, sr)?;
        let second = read(&mut steps, &map, anchor.wrapping_add(2), sr)?;
        let mut expected = initial;
        expected.pc = anchor;
        expected.set_status_register(sr);
        if expected.status_register() != sr
            || expected != case.expected.registers()?
            || [first, second] != case.expected.prefetch
            || map != memory(&case.expected.ram)?
        {
            return Err("manual full registers/queue/RAM differ from original".into());
        }
        let clocks = if immediate { 16 } else { 12 };
        let plan = Self {
            initial,
            expected,
            queue: [first, second],
            memory: map,
            steps,
            clocks,
        };
        plan.validate_phases(case)?;
        Ok(plan)
    }

    fn validate_phases(&self, case: &Case) -> Result<(), String> {
        let mut flattened: Vec<Step> = Vec::new();
        for step in &self.steps {
            if let Step::Idle(clocks) = step
                && let Some(Step::Idle(last)) = flattened.last_mut()
            {
                *last += clocks;
            } else {
                flattened.push(step.clone());
            }
        }
        if self.clocks != u64::from(case.reference_total_cycles)
            || flattened.len() != case.transactions.len()
        {
            return Err("original phase count/clocks differ".into());
        }
        for (manual, original) in flattened.iter().zip(&case.transactions) {
            let matches = match (manual, original) {
                (Step::Idle(clocks), Phase::Idle { duration_cycles }) => clocks == duration_cycles,
                (Step::Read(access, value), Phase::Read(raw)) => {
                    raw.duration_cycles == 4
                        && raw.address_bus == access.bus_address
                        && raw.function_code == access.function_code as u32
                        && raw.data_bus == u32::from(*value)
                        && (raw.uds, raw.lds) == (1, 1)
                }
                _ => false,
            };
            if !matches {
                return Err("original ordered phase differs from manual status schedule".into());
            }
        }
        Ok(())
    }
}
