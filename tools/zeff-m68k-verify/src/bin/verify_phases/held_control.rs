use super::{Case, Phase, expected_memory, memory_map};
use std::collections::BTreeMap;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes};
use zeff_m68k::{SUPERVISOR, TRACE};

struct Permission {
    kind: AccessKind,
    write: bool,
    address: u32,
    fc: u32,
    lanes: ByteLanes,
}

pub(super) fn derive(case: &Case) -> Result<Option<Contract>, String> {
    if !matches!(
        case.cohort.as_str(),
        "RESET" | "RTE" | "RTS" | "ILLEGAL_LINEA" | "ILLEGAL_LINEF"
    ) {
        return Ok(None);
    }
    let op = case.initial.prefetch[0];
    let valid = match case.cohort.as_str() {
        "RESET" => op == 0x4e70,
        "RTE" => op == 0x4e73,
        "RTS" => op == 0x4e75,
        "ILLEGAL_LINEA" => op >> 12 == 10,
        "ILLEGAL_LINEF" => op >> 12 == 15,
        _ => false,
    };
    if !valid {
        return Err("held-control cohort/opcode differs".into());
    }
    let original = case.initial.registers()?;
    let mut expected = original;
    let exception = match case.cohort.as_str() {
        "RESET" | "RTE" if !original.supervisor() => Some(8),
        "ILLEGAL_LINEA" => Some(10),
        "ILLEGAL_LINEF" => Some(11),
        _ => None,
    };
    let initial = memory_map(&case.initial.ram)?;
    let final_memory = expected_memory(case, &initial)?;
    let mut b = Builder {
        case,
        memory: initial,
        clocks: 0,
        contract: Contract {
            phases: Vec::new(),
            exception,
            pending_trace: original.status_register() & TRACE != 0 && exception.is_none(),
            reset_pulse: op == 0x4e70 && original.supervisor(),
        },
    };
    let queue = if let Some(vector) = exception {
        let s = original.ssp;
        b.idle(4)?;
        b.transfer(
            AccessKind::Stack,
            s.wrapping_sub(2),
            5,
            ByteLanes::Both,
            Some(original.pc as u16),
        )?;
        b.transfer(
            AccessKind::Stack,
            s.wrapping_sub(6),
            5,
            ByteLanes::Both,
            Some(original.status_register()),
        )?;
        b.transfer(
            AccessKind::Stack,
            s.wrapping_sub(4),
            5,
            ByteLanes::Both,
            Some((original.pc >> 16) as u16),
        )?;
        let high = b.transfer(
            AccessKind::Vector,
            u32::from(vector) * 4,
            5,
            ByteLanes::Both,
            None,
        )?;
        let low = b.transfer(
            AccessKind::Vector,
            u32::from(vector) * 4 + 2,
            5,
            ByteLanes::Both,
            None,
        )?;
        expected.pc = (u32::from(high) << 16) | u32::from(low);
        expected.ssp = s.wrapping_sub(6);
        expected.set_status_register((original.status_register() | SUPERVISOR) & !TRACE);
        let first = b.fetch(expected.pc, 6)?;
        b.idle(2)?;
        [first, b.fetch(expected.pc.wrapping_add(2), 6)?]
    } else if op == 0x4e70 {
        b.idle(128)?;
        expected.pc = original.pc.wrapping_add(2);
        [
            case.initial.prefetch[1],
            b.fetch(
                original.pc.wrapping_add(4),
                if original.supervisor() { 6 } else { 2 },
            )?,
        ]
    } else {
        let rte = op == 0x4e73;
        let sp = original.address_register(7).unwrap();
        let fc = if original.supervisor() { 5 } else { 1 };
        let offset = if rte {
            let sr = b.transfer(AccessKind::Stack, sp, 5, ByteLanes::Both, None)?;
            expected.set_status_register(sr & 0xa71f);
            2
        } else {
            0
        };
        let high = b.transfer(
            AccessKind::Stack,
            sp.wrapping_add(offset),
            fc,
            ByteLanes::Both,
            None,
        )?;
        let low = b.transfer(
            AccessKind::Stack,
            sp.wrapping_add(offset + 2),
            fc,
            ByteLanes::Both,
            None,
        )?;
        expected.pc = (u32::from(high) << 16) | u32::from(low);
        if rte {
            expected.ssp = sp.wrapping_add(6);
        } else {
            expected.set_address_register(7, sp.wrapping_add(4));
        }
        let pfc = if expected.supervisor() { 6 } else { 2 };
        [
            b.fetch(expected.pc, pfc)?,
            b.fetch(expected.pc.wrapping_add(2), pfc)?,
        ]
    };
    if expected != case.expected.registers()?
        || queue != case.expected.prefetch
        || b.memory != final_memory
        || b.contract.phases.len() != case.transactions.len()
        || b.clocks != u64::from(case.reference_total_cycles)
    {
        return Err("derived held-control completion differs".into());
    }
    Ok(Some(b.contract))
}

pub(super) struct Contract {
    pub(super) exception: Option<u8>,
    pub(super) pending_trace: bool,
    pub(super) reset_pulse: bool,
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
    fn idle(&mut self, clocks: u32) -> Result<(), String> {
        match self.case.transactions.get(self.contract.phases.len()) {
            Some(Phase::Idle { duration_cycles }) if *duration_cycles == clocks => {}
            _ => return Err("held-control idle boundary differs".into()),
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
                .ok_or("unobserved held-control byte")
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
            _ => return Err("held-control order differs".into()),
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
