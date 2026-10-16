use super::{Case, Phase, expected_memory, memory_map};
use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes};

#[path = "ordinary_register.rs"]
mod register;
#[path = "ordinary_unary.rs"]
mod unary;

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

pub(super) struct Builder<'a> {
    case: &'a Case,
    memory: BTreeMap<u32, u8>,
    contract: Contract,
    clocks: u64,
}

impl Builder<'_> {
    fn new(case: &Case) -> Result<Builder<'_>, String> {
        Ok(Builder {
            case,
            memory: memory_map(&case.initial.ram)?,
            contract: Contract { phases: Vec::new() },
            clocks: 0,
        })
    }

    fn finish(self, expected: Registers, queue: [u16; 2]) -> Result<Contract, String> {
        let initial = memory_map(&self.case.initial.ram)?;
        if expected != self.case.expected.registers()?
            || queue != self.case.expected.prefetch
            || self.memory != expected_memory(self.case, &initial)?
            || self.contract.phases.len() != self.case.transactions.len()
            || self.clocks != u64::from(self.case.reference_total_cycles)
        {
            return Err("derived ordinary completion differs".into());
        }
        Ok(self.contract)
    }
    pub(super) fn word(&self, address: u32) -> Result<u16, String> {
        let byte = |a| {
            self.memory
                .get(&(a & 0xffffff))
                .copied()
                .ok_or_else(|| format!("unobserved ordinary read {a:08x}"))
        };
        Ok((u16::from(byte(address)?) << 8) | u16::from(byte(address.wrapping_add(1))?))
    }

    pub(super) fn idle(&mut self, clocks: u32) -> Result<(), String> {
        let mut remaining = clocks;
        while remaining != 0 {
            match self.case.transactions.get(self.contract.phases.len()) {
                Some(Phase::Idle { duration_cycles })
                    if *duration_cycles > 0 && *duration_cycles <= remaining =>
                {
                    remaining -= *duration_cycles;
                    self.contract.phases.push(None);
                }
                _ => return Err("ordinary idle boundary differs".into()),
            }
        }
        self.clocks += u64::from(clocks);
        Ok(())
    }

    pub(super) fn transfer(
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
                .ok_or("unobserved ordinary byte")
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
            _ => return Err("ordinary order differs".into()),
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

    pub(super) fn fetch(&mut self, address: u32, fc: u32) -> Result<u16, String> {
        self.transfer(AccessKind::Prefetch, address, fc, ByteLanes::Both, None)
    }
}

pub(super) fn derive(case: &Case) -> Result<Option<Contract>, String> {
    let family = case.cohort.split('.').next().unwrap();
    let admitted = matches!(
        case.cohort.as_str(),
        "NEG.b"
            | "NEG.w"
            | "NEG.l"
            | "NEGX.b"
            | "NEGX.w"
            | "NOT.b"
            | "NOT.w"
            | "NOT.l"
            | "ASL.l"
            | "ASR.b"
            | "ASR.l"
            | "LSL.b"
            | "LSL.l"
            | "LSR.b"
            | "LSR.l"
            | "ROL.b"
            | "ROL.l"
            | "ROR.b"
            | "ROXL.b"
            | "ROXL.l"
            | "ROXR.b"
            | "ROXR.l"
            | "EXT.w"
            | "EXT.l"
            | "SWAP"
    );
    if !admitted {
        return Ok(None);
    }
    if !super::format::transfer_metadata(case) {
        return Err("ordinary cohort/opcode differs".into());
    }
    let mut builder = Builder::new(case)?;
    let (expected, queue) = if matches!(family, "NEG" | "NEGX" | "NOT") {
        unary::derive(case, &mut builder)?
    } else {
        register::derive(case, &mut builder)?
    };
    Ok(Some(builder.finish(expected, queue)?))
}

pub(super) fn flags(sr: u16, result: u32, sign: u32, xcvz: (bool, bool, bool, bool)) -> u16 {
    let (x, c, v, z) = xcvz;
    (sr & !31)
        | (u16::from(x) << 4)
        | (u16::from(result & sign != 0) << 3)
        | (u16::from(z) << 2)
        | (u16::from(v) << 1)
        | u16::from(c)
}
