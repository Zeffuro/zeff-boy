use super::{Case, Phase, expected_memory, memory_map};
use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes};

struct Permission {
    kind: AccessKind,
    write: bool,
    address: u32,
    fc: u32,
    lanes: ByteLanes,
}

pub(super) struct Contract {
    phases: Vec<Option<Permission>>,
}

impl Contract {
    pub(super) fn permits(&self, access: BusAccess, write: bool, cursor: usize) -> bool {
        self.phases
            .get(cursor)
            .and_then(Option::as_ref)
            .is_some_and(|p| {
                p.kind == access.kind
                    && p.write == write
                    && p.address == access.bus_address
                    && p.fc == access.function_code as u32
                    && p.lanes == access.lanes
            })
    }
}

struct Builder<'a> {
    case: &'a Case,
    memory: BTreeMap<u32, u8>,
    contract: Contract,
    clocks: u64,
}

impl Builder<'_> {
    fn word(&self, address: u32) -> Result<u16, String> {
        let byte = |a| {
            self.memory
                .get(&(a & 0xffffff))
                .copied()
                .ok_or_else(|| format!("unobserved transfer read {a:08x}"))
        };
        Ok((u16::from(byte(address)?) << 8) | u16::from(byte(address.wrapping_add(1))?))
    }

    fn idle(&mut self, clocks: u32) -> Result<(), String> {
        match self.case.transactions.get(self.contract.phases.len()) {
            Some(Phase::Idle { duration_cycles }) if *duration_cycles == clocks => {}
            _ => return Err("transfer idle boundary differs".into()),
        }
        self.clocks += u64::from(clocks);
        self.contract.phases.push(None);
        Ok(())
    }

    fn transfer(
        &mut self,
        kind: AccessKind,
        address: u32,
        fc: u32,
        lanes: ByteLanes,
        write: Option<u16>,
    ) -> Result<u16, String> {
        let address = address & 0xffffff;
        let (uds, lds) = match lanes {
            ByteLanes::Both => (1, 1),
            ByteLanes::Upper => (1, 0),
            ByteLanes::Lower => (0, 1),
        };
        let read_byte = |a| {
            self.memory
                .get(&a)
                .copied()
                .ok_or("unobserved transfer byte")
        };
        let data = if let Some(data) = write {
            data
        } else {
            (if uds == 1 {
                u16::from(read_byte(address)?) << 8
            } else {
                0
            }) | if lds == 1 {
                u16::from(read_byte((address + 1) & 0xffffff)?)
            } else {
                0
            }
        };
        let raw = match (
            self.case.transactions.get(self.contract.phases.len()),
            write.is_some(),
        ) {
            (Some(Phase::Read(t)), false) | (Some(Phase::Write(t)), true) => t,
            _ => return Err("transfer order differs".into()),
        };
        if raw.address_bus != address
            || raw.function_code != fc
            || raw.uds != uds
            || raw.lds != lds
            || raw.data_bus != u32::from(data)
            || raw.duration_cycles < 4
        {
            return Err("initial-state transfer fields differ".into());
        }
        self.clocks += u64::from(raw.duration_cycles);
        if write.is_some() {
            if uds == 1 {
                self.memory.insert(address, (data >> 8) as u8);
            }
            if lds == 1 {
                self.memory.insert((address + 1) & 0xffffff, data as u8);
            }
        }
        self.contract.phases.push(Some(Permission {
            kind,
            write: write.is_some(),
            address,
            fc,
            lanes,
        }));
        Ok(data)
    }

    fn fetch(&mut self, address: u32, fc: u32) -> Result<u16, String> {
        self.transfer(AccessKind::Prefetch, address, fc, ByteLanes::Both, None)
    }
}

fn value(registers: &Registers, index: usize) -> u32 {
    if index < 8 {
        registers.d[index]
    } else {
        registers.address_register(index - 8).unwrap()
    }
}

fn set(registers: &mut Registers, index: usize, value: u32) {
    if index < 8 {
        registers.d[index] = value;
    } else {
        registers.set_address_register(index - 8, value);
    }
}

fn address(builder: &Builder<'_>, registers: &Registers, op: u16) -> Result<(u32, u32), String> {
    let mode = (op >> 3) & 7;
    let r = usize::from(op & 7);
    let base = registers.pc.wrapping_add(4);
    if matches!(mode, 2..=4) {
        return Ok((registers.address_register(r).unwrap(), 0));
    }
    let extension = builder.word(base)?;
    let raw_index = value(registers, usize::from((extension >> 12) & 15));
    let index = if extension & 0x800 != 0 {
        raw_index as i32
    } else {
        i32::from(raw_index as i16)
    };
    let indexed = |a: u32| {
        a.wrapping_add_signed(index)
            .wrapping_add_signed(i32::from(extension as u8 as i8))
    };
    Ok(match (mode, r) {
        (5, _) => (
            registers
                .address_register(r)
                .unwrap()
                .wrapping_add_signed(i32::from(extension as i16)),
            1,
        ),
        (6, _) => (indexed(registers.address_register(r).unwrap()), 1),
        (7, 0) => (i32::from(extension as i16) as u32, 1),
        (7, 1) => (
            (u32::from(extension) << 16) | u32::from(builder.word(base.wrapping_add(2))?),
            2,
        ),
        (7, 2) => (base.wrapping_add_signed(i32::from(extension as i16)), 1),
        (7, 3) => (indexed(base), 1),
        _ => return Err("invalid MOVEM address".into()),
    })
}

pub(super) fn derive(case: &Case) -> Result<Option<Contract>, String> {
    if !matches!(
        case.cohort.as_str(),
        "MOVEM.w" | "MOVEM.l" | "MOVEP.w" | "MOVEP.l"
    ) {
        return Ok(None);
    }
    if !super::format::transfer_metadata(case) {
        return Err("transfer cohort/opcode differs".into());
    }
    let original = case.initial.registers()?;
    let mut expected = original;
    let initial = memory_map(&case.initial.ram)?;
    let final_memory = expected_memory(case, &initial)?;
    let mut b = Builder {
        case,
        memory: initial,
        contract: Contract { phases: Vec::new() },
        clocks: 0,
    };
    let op = case.initial.prefetch[0];
    let dfc = if original.supervisor() { 5 } else { 1 };
    let pfc = dfc + 1;
    let p = original.pc;
    let width = if op & 64 != 0 { 4 } else { 2 };
    let queue = if case.cohort.starts_with("MOVEP") {
        let dn = usize::from((op >> 9) & 7);
        let base = original
            .address_register(usize::from(op & 7))
            .unwrap()
            .wrapping_add_signed(i32::from(case.initial.prefetch[1] as i16));
        let first = b.fetch(p.wrapping_add(4), pfc)?;
        let store = op & 128 != 0;
        let mut result = 0;
        for n in 0..width {
            let a = base.wrapping_add(n * 2);
            let upper = a & 1 == 0;
            let lanes = if upper {
                ByteLanes::Upper
            } else {
                ByteLanes::Lower
            };
            let shift = if upper { 8 } else { 0 };
            let byte = ((original.d[dn] >> (8 * (width - 1 - n))) & 255) as u16;
            let data = b.transfer(
                AccessKind::Data,
                a & !1,
                dfc,
                lanes,
                store.then_some(byte << shift),
            )?;
            result = (result << 8) | u32::from(data >> shift);
        }
        if !store {
            expected.d[dn] = if width == 2 {
                original.d[dn] & 0xffff0000 | result
            } else {
                result
            };
        }
        expected.pc = p.wrapping_add(4);
        [first, b.fetch(p.wrapping_add(6), pfc)?]
    } else {
        let mode = (op >> 3) & 7;
        let r = usize::from(op & 7);
        let load = op & 1024 != 0;
        let legal = if load {
            matches!(mode, 2 | 3 | 5 | 6) || mode == 7 && r <= 3
        } else {
            matches!(mode, 2 | 4 | 5 | 6) || mode == 7 && r <= 1
        };
        if !legal {
            return Err("invalid MOVEM operand".into());
        }
        let (mut a, extensions) = address(&b, &original, op)?;
        let mut first = b.fetch(p.wrapping_add(4), pfc)?;
        if mode == 6 || mode == 7 && r == 3 {
            b.idle(2)?;
        }
        for n in 0..extensions {
            first = b.fetch(p.wrapping_add(6 + n * 2), pfc)?;
        }
        expected.pc = p.wrapping_add(4 + extensions * 2);
        let fc = if mode == 7 && matches!(r, 2 | 3) {
            pfc
        } else {
            dfc
        };
        for bit in 0..16 {
            if case.initial.prefetch[1] & (1 << bit) == 0 {
                continue;
            }
            let index = if mode == 4 { 15 - bit } else { bit };
            if load {
                let high = b.transfer(AccessKind::Data, a, fc, ByteLanes::Both, None)?;
                let data = if width == 4 {
                    (u32::from(high) << 16)
                        | u32::from(b.transfer(
                            AccessKind::Data,
                            a.wrapping_add(2),
                            fc,
                            ByteLanes::Both,
                            None,
                        )?)
                } else {
                    i32::from(high as i16) as u32
                };
                set(&mut expected, index, data);
                a = a.wrapping_add(width);
            } else {
                let data = value(&original, index);
                if mode == 4 {
                    a = a.wrapping_sub(width);
                    if width == 4 {
                        b.transfer(
                            AccessKind::Data,
                            a.wrapping_add(2),
                            fc,
                            ByteLanes::Both,
                            Some(data as u16),
                        )?;
                    }
                    b.transfer(
                        AccessKind::Data,
                        a,
                        fc,
                        ByteLanes::Both,
                        Some(if width == 4 {
                            (data >> 16) as u16
                        } else {
                            data as u16
                        }),
                    )?;
                } else {
                    b.transfer(
                        AccessKind::Data,
                        a,
                        fc,
                        ByteLanes::Both,
                        Some(if width == 4 {
                            (data >> 16) as u16
                        } else {
                            data as u16
                        }),
                    )?;
                    if width == 4 {
                        b.transfer(
                            AccessKind::Data,
                            a.wrapping_add(2),
                            fc,
                            ByteLanes::Both,
                            Some(data as u16),
                        )?;
                    }
                    a = a.wrapping_add(width);
                }
            }
        }
        if load {
            b.transfer(AccessKind::Discard, a, fc, ByteLanes::Both, None)?;
        }
        if mode == 3 || mode == 4 {
            expected.set_address_register(r, a);
        }
        [first, b.fetch(expected.pc.wrapping_add(2), pfc)?]
    };
    if expected != case.expected.registers()?
        || queue != case.expected.prefetch
        || b.memory != final_memory
        || b.contract.phases.len() != case.transactions.len()
        || b.clocks != u64::from(case.reference_total_cycles)
    {
        return Err("derived transfer completion differs".into());
    }
    Ok(Some(b.contract))
}
