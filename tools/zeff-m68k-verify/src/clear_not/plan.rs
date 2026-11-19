use std::collections::BTreeMap;

use crate::reference::{Case, Phase};
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};

pub(super) type Memory = BTreeMap<u32, u8>;

#[derive(Clone, Debug)]
pub(super) enum Step {
    Idle(u32),
    Transfer {
        access: BusAccess,
        direction: Direction,
        value: u16,
        entry: bool,
    },
    Detect(BusAccess),
}

pub(super) struct Plan {
    pub steps: Vec<Step>,
    pub initial: Registers,
    pub phase: Registers,
    pub final_registers: Registers,
    pub words: [u16; 2],
    pub memory: Memory,
    pub odd: bool,
    pub operand: u32,
    pub capture_pc: u32,
    pub total: u64,
}

pub(super) fn memory(bytes: &[(u32, u8)]) -> Result<Memory, String> {
    let mut map = Memory::new();
    for &(address, value) in bytes {
        if address > 0x00ff_ffff || map.insert(address, value).is_some() {
            return Err("duplicate or out-of-range RAM byte".into());
        }
    }
    Ok(map)
}

pub(super) fn sample(map: &Memory, address: u32) -> Result<u16, String> {
    let address = address & 0x00ff_fffe;
    let high = map.get(&address).ok_or("missing actual RAM high byte")?;
    let low = map
        .get(&(address + 1))
        .ok_or("missing actual RAM low byte")?;
    Ok(u16::from(*high) << 8 | u16::from(*low))
}

pub(super) fn deposit(map: &mut Memory, address: u32, value: u16) {
    let address = address & 0x00ff_fffe;
    map.insert(address, (value >> 8) as u8);
    map.insert(address + 1, value as u8);
}

fn access(address: u32, fc: FunctionCode, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_fffe,
        function_code: fc,
        kind,
        lanes: ByteLanes::Both,
    }
}

fn read(
    steps: &mut Vec<Step>,
    map: &Memory,
    at: u32,
    space: (FunctionCode, AccessKind, bool),
) -> Result<u16, String> {
    let (fc, kind, entry) = space;
    let value = sample(map, at)?;
    steps.push(Step::Transfer {
        access: access(at, fc, kind),
        direction: Direction::Read,
        value,
        entry,
    });
    Ok(value)
}

fn write(
    steps: &mut Vec<Step>,
    map: &mut Memory,
    at: u32,
    value: u16,
    space: (FunctionCode, AccessKind, bool),
) {
    let (fc, kind, entry) = space;
    steps.push(Step::Transfer {
        access: access(at, fc, kind),
        direction: Direction::Write,
        value,
        entry,
    });
    deposit(map, at, value);
}

fn logic(sr: u16, value: u32, long: bool) -> u16 {
    let value = if long { value } else { value & 65535 };
    (sr & !15)
        | if value == 0 { 4 } else { 0 }
        | if value & if long { 0x8000_0000 } else { 0x8000 } != 0 {
            8
        } else {
            0
        }
}

impl Plan {
    pub fn new(case: &Case) -> Result<Self, String> {
        super::intake::validate(case)?;
        let initial = case.initial.registers()?;
        let mut phase = initial;
        let mut map = memory(&case.initial.ram)?;
        let mut steps = Vec::new();
        let opcode = case.initial.prefetch[0];
        let mode = opcode >> 3 & 7;
        let reg = usize::from(opcode & 7);
        let long = opcode >> 6 & 3 == 2;
        let stride = if long { 4 } else { 2 };
        let p = initial.pc;
        let extension = case.initial.prefetch[1];
        let data_fc = if initial.supervisor() {
            FunctionCode::SupervisorData
        } else {
            FunctionCode::UserData
        };
        let code_fc = if initial.supervisor() {
            FunctionCode::SupervisorProgram
        } else {
            FunctionCode::UserProgram
        };
        if matches!(mode, 4 | 6) {
            steps.push(Step::Idle(2));
        }
        let extensions = if mode == 7 && reg == 1 {
            2
        } else {
            u32::from(mode >= 5)
        };
        let mut next = extension;
        let mut low = 0;
        if extensions != 0 {
            next = read(
                &mut steps,
                &map,
                p.wrapping_add(4),
                (code_fc, AccessKind::Prefetch, false),
            )?;
            low = next;
        }
        if extensions == 2 {
            next = read(
                &mut steps,
                &map,
                p.wrapping_add(6),
                (code_fc, AccessKind::Prefetch, false),
            )?;
        }
        let base = initial.address_register(reg).ok_or("bad source An")?;
        let operand = match (mode, reg) {
            (2 | 3, _) => base,
            (4, _) => base.wrapping_sub(stride),
            (5, _) => base.wrapping_add_signed(i32::from(extension as i16)),
            (6, _) => {
                let selected = usize::from(extension >> 12 & 7);
                let index = if extension & 0x8000 == 0 {
                    initial.d[selected]
                } else {
                    initial.address_register(selected).ok_or("bad index An")?
                };
                let index = if extension & 0x0800 == 0 {
                    i32::from(index as i16) as u32
                } else {
                    index
                };
                base.wrapping_add(index)
                    .wrapping_add_signed(i32::from(extension as u8 as i8))
            }
            (7, 0) => i32::from(extension as i16) as u32,
            (7, 1) => u32::from(extension) << 16 | u32::from(low),
            _ => return Err("unselected RMW mode".into()),
        };
        if mode == 4 || mode == 3 && !long {
            phase.set_address_register(
                reg,
                if mode == 4 {
                    operand
                } else {
                    base.wrapping_add(stride)
                },
            );
        }
        let anchor = p.wrapping_add(2 + 2 * extensions);
        let odd = operand & 1 != 0;
        let capture_pc = p.wrapping_add(match (mode, reg, long) {
            (7, 1, _) => 6,
            (7, 0, _) | (4, _, false) => 4,
            _ => 2,
        });
        let mut final_registers = phase;
        let words;
        if odd {
            steps.push(Step::Detect(access(operand, data_fc, AccessKind::Data)));
            steps.push(Step::Idle(4));
            let ssw = (opcode & 0xffe0) | 16 | data_fc as u16;
            for (offset, value) in [
                (2, capture_pc as u16),
                (6, initial.status_register()),
                (4, (capture_pc >> 16) as u16),
                (8, opcode),
                (10, operand as u16),
                (14, ssw),
                (12, (operand >> 16) as u16),
            ] {
                write(
                    &mut steps,
                    &mut map,
                    phase.ssp.wrapping_sub(offset),
                    value,
                    (FunctionCode::SupervisorData, AccessKind::Stack, true),
                );
            }
            let high = read(
                &mut steps,
                &map,
                12,
                (FunctionCode::SupervisorData, AccessKind::Vector, true),
            )?;
            let low = read(
                &mut steps,
                &map,
                14,
                (FunctionCode::SupervisorData, AccessKind::Vector, true),
            )?;
            let handler = u32::from(high) << 16 | u32::from(low);
            let first = read(
                &mut steps,
                &map,
                handler,
                (FunctionCode::SupervisorProgram, AccessKind::Prefetch, true),
            )?;
            steps.push(Step::Idle(2));
            let second = read(
                &mut steps,
                &map,
                handler.wrapping_add(2),
                (FunctionCode::SupervisorProgram, AccessKind::Prefetch, true),
            )?;
            words = [first, second];
            final_registers.ssp = phase.ssp.wrapping_sub(14);
            final_registers.set_status_register((initial.status_register() | 0x2000) & !0x8000);
            final_registers.pc = handler;
        } else {
            let high = read(
                &mut steps,
                &map,
                operand,
                (data_fc, AccessKind::Data, false),
            )?;
            let value = if long {
                if mode == 3 {
                    phase.set_address_register(reg, base.wrapping_add(stride));
                }
                let low = read(
                    &mut steps,
                    &map,
                    operand.wrapping_add(2),
                    (data_fc, AccessKind::Data, false),
                )?;
                u32::from(high) << 16 | u32::from(low)
            } else {
                u32::from(high)
            };
            let result = if opcode & 0xff00 == 0x4600 {
                !value & if long { u32::MAX } else { 65535 }
            } else {
                0
            };
            let fetched = read(
                &mut steps,
                &map,
                anchor.wrapping_add(2),
                (code_fc, AccessKind::Prefetch, false),
            )?;
            if long {
                write(
                    &mut steps,
                    &mut map,
                    operand.wrapping_add(2),
                    result as u16,
                    (data_fc, AccessKind::Data, false),
                );
            }
            write(
                &mut steps,
                &mut map,
                operand,
                if long {
                    (result >> 16) as u16
                } else {
                    result as u16
                },
                (data_fc, AccessKind::Data, false),
            );
            final_registers = phase;
            final_registers.set_status_register(logic(initial.status_register(), result, long));
            final_registers.pc = anchor;
            words = [next, fetched];
        }
        let total = steps
            .iter()
            .map(|s| match s {
                Step::Idle(n) => u64::from(*n),
                Step::Detect(_) => 8,
                Step::Transfer { .. } => 4,
            })
            .sum();
        let result = Self {
            steps,
            initial,
            phase,
            final_registers,
            words,
            memory: map,
            odd,
            operand,
            capture_pc,
            total,
        };
        result.validate_raw(case)?;
        if result.final_registers != case.expected.registers()?
            || result.words != case.expected.prefetch
            || result.memory != memory(&case.expected.ram)?
        {
            return Err(
                "manual full registers/queue/shadow RAM differ from original result".into(),
            );
        }
        Ok(result)
    }

    fn validate_raw(&self, case: &Case) -> Result<(), String> {
        if self.steps.len() != case.transactions.len()
            || self.total != u64::from(case.reference_total_cycles)
        {
            return Err("manual phase count or clock total differs".into());
        }
        let mut after_detector = false;
        for (step, raw) in self.steps.iter().zip(&case.transactions) {
            let valid = match (step, raw) {
                (Step::Idle(n), Phase::Idle { duration_cycles }) => {
                    let expected = if after_detector {
                        after_detector = false;
                        n + 4
                    } else {
                        *n
                    };
                    expected == *duration_cycles
                }
                (Step::Detect(a), Phase::ReadAddressError(t)) => {
                    after_detector = true;
                    t.duration_cycles == 4
                        && t.address_bus == a.bus_address
                        && t.function_code == a.function_code as u32
                        && (t.uds, t.lds) == (1, 1)
                }
                (
                    Step::Transfer {
                        access: a,
                        direction,
                        value,
                        ..
                    },
                    raw,
                ) => {
                    let transfer = match (direction, raw) {
                        (Direction::Read, Phase::Read(t)) | (Direction::Write, Phase::Write(t)) => {
                            Some(t)
                        }
                        _ => None,
                    };
                    transfer.is_some_and(|t| {
                        t.duration_cycles == 4
                            && t.address_bus == a.bus_address
                            && t.function_code == a.function_code as u32
                            && (t.uds, t.lds) == (1, 1)
                            && t.data_bus == u32::from(*value)
                    })
                }
                _ => false,
            };
            if !valid {
                return Err("manual ordered access/data/duration differs from raw phase".into());
            }
        }
        Ok(())
    }
}
