use super::{Case, Phase, memory_map};
use std::collections::BTreeMap;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes};
use zeff_m68k::{Registers, SUPERVISOR, TRACE};

struct Permission {
    kind: AccessKind,
    write: bool,
    address: u32,
    function_code: u32,
}

pub(super) struct Contract {
    pub(super) exception: Option<u8>,
    pub(super) pending_trace: bool,
    permissions: Vec<Permission>,
}

impl Contract {
    pub(super) fn permits(&self, access: BusAccess, write: bool) -> bool {
        access.lanes == ByteLanes::Both
            && self.permissions.iter().any(|permission| {
                permission.kind == access.kind
                    && permission.write == write
                    && permission.address == access.bus_address
                    && permission.function_code == access.function_code as u32
            })
    }

    fn allow(&mut self, kind: AccessKind, write: bool, address: u32, function_code: u32) {
        self.permissions.push(Permission {
            kind,
            write,
            address: address & 0x00ff_ffff,
            function_code,
        });
    }
}

fn word(memory: &BTreeMap<u32, u8>, address: u32) -> Result<u16, String> {
    let address = address & 0x00ff_ffff;
    let byte = |address| {
        memory
            .get(&address)
            .copied()
            .ok_or_else(|| format!("unobserved stack/status read {address:06x}"))
    };
    Ok((u16::from(byte(address)?) << 8) | u16::from(byte(address.wrapping_add(1) & 0x00ff_ffff)?))
}

fn long(memory: &BTreeMap<u32, u8>, address: u32) -> Result<u32, String> {
    Ok((u32::from(word(memory, address)?) << 16)
        | u32::from(word(memory, address.wrapping_add(2))?))
}

fn operand(
    case: &Case,
    registers: &Registers,
    memory: &BTreeMap<u32, u8>,
) -> Result<(u16, Option<u32>, u32), String> {
    let op = case.initial.prefetch[0];
    let mode = (op >> 3) & 7;
    let register = usize::from(op & 7);
    let extension = case.initial.prefetch[1];
    let base = registers.pc.wrapping_add(2);
    if mode == 0 {
        return Ok((registers.d[register] as u16, None, base));
    }
    if mode == 7 && register == 4 {
        return Ok((extension, None, base.wrapping_add(2)));
    }
    let an = |index| registers.address_register(index).unwrap();
    let index = || {
        let number = usize::from((extension >> 12) & 7);
        let value = if extension & 0x8000 != 0 {
            an(number)
        } else {
            registers.d[number]
        };
        if extension & 0x0800 != 0 {
            value as i32
        } else {
            i32::from(value as i16)
        }
    };
    let (address, extensions) = match mode {
        2 | 3 => (an(register), 0),
        4 => (an(register).wrapping_sub(2), 0),
        5 => (
            an(register).wrapping_add_signed(i32::from(extension as i16)),
            1,
        ),
        6 => (
            an(register)
                .wrapping_add_signed(index())
                .wrapping_add_signed(i32::from(extension as u8 as i8)),
            1,
        ),
        7 => match register {
            0 => (i32::from(extension as i16) as u32, 1),
            1 => (
                (u32::from(extension) << 16)
                    | u32::from(word(memory, registers.pc.wrapping_add(4))?),
                2,
            ),
            2 => (base.wrapping_add_signed(i32::from(extension as i16)), 1),
            3 => (
                base.wrapping_add_signed(index())
                    .wrapping_add_signed(i32::from(extension as u8 as i8)),
                1,
            ),
            _ => return Err("invalid stack/status operand".into()),
        },
        _ => return Err("invalid stack/status operand".into()),
    };
    Ok((
        word(memory, address)?,
        Some(address & 0x00ff_ffff),
        base.wrapping_add(extensions * 2),
    ))
}

pub(super) fn derive(case: &Case) -> Result<Option<Contract>, String> {
    let family = case.cohort.as_str();
    if !matches!(
        family,
        "LINK"
            | "UNLINK"
            | "RTR"
            | "TRAP"
            | "MOVEfromUSP"
            | "MOVEtoUSP"
            | "MOVEfromSR"
            | "MOVEtoSR"
            | "MOVEtoCCR"
    ) {
        return Ok(None);
    }
    let op = case.initial.prefetch[0];
    let matches = match family {
        "LINK" => op & 0xfff8 == 0x4e50,
        "UNLINK" => op & 0xfff8 == 0x4e58,
        "RTR" => op == 0x4e77,
        "TRAP" => op & 0xfff0 == 0x4e40,
        "MOVEfromUSP" => op & 0xfff8 == 0x4e68,
        "MOVEtoUSP" => op & 0xfff8 == 0x4e60,
        "MOVEfromSR" => op & 0xffc0 == 0x40c0,
        "MOVEtoSR" => op & 0xffc0 == 0x46c0,
        "MOVEtoCCR" => op & 0xffc0 == 0x44c0,
        _ => false,
    };
    if !matches {
        return Err("stack/status opcode contradicts cohort".into());
    }
    let registers = case.initial.registers()?;
    let mut expected = registers;
    expected.pc = expected.pc.wrapping_add(2);
    let mut memory = memory_map(&case.initial.ram)?;
    let privilege =
        matches!(family, "MOVEfromUSP" | "MOVEtoUSP" | "MOVEtoSR") && !registers.supervisor();
    let exception = if privilege {
        Some(8)
    } else if family == "TRAP" {
        Some(32 + (op & 15) as u8)
    } else {
        None
    };
    let mut contract = Contract {
        exception,
        pending_trace: registers.status_register() & TRACE != 0 && !privilege,
        permissions: Vec::new(),
    };
    let mut writes = Vec::new();
    let index = usize::from(op & 7);
    let data_fc = if registers.supervisor() { 5 } else { 1 };
    if let Some(vector) = exception {
        let pc = if privilege {
            registers.pc
        } else {
            registers.pc.wrapping_add(2)
        };
        let sp = registers.ssp;
        writes.extend([
            (sp.wrapping_sub(2) & 0x00ff_ffff, pc as u16, 5),
            (
                sp.wrapping_sub(6) & 0x00ff_ffff,
                registers.status_register(),
                5,
            ),
            (sp.wrapping_sub(4) & 0x00ff_ffff, (pc >> 16) as u16, 5),
        ]);
        for &(address, data, fc) in &writes {
            contract.allow(AccessKind::Stack, true, address, fc);
            memory.insert(address, (data >> 8) as u8);
            memory.insert(address.wrapping_add(1) & 0x00ff_ffff, data as u8);
        }
        let vector_address = u32::from(vector) * 4;
        contract.allow(AccessKind::Vector, false, vector_address, 5);
        contract.allow(AccessKind::Vector, false, vector_address + 2, 5);
        expected.ssp = sp.wrapping_sub(6);
        expected.set_status_register((registers.status_register() | SUPERVISOR) & !TRACE);
        expected.pc = long(&memory, vector_address)?;
        if case.expected.prefetch
            != [
                word(&memory, expected.pc)?,
                word(&memory, expected.pc.wrapping_add(2))?,
            ]
        {
            return Err("stack/status exception queue contradicts initial state".into());
        }
    } else {
        match family {
            "LINK" => {
                let value = registers.address_register(index).unwrap();
                let frame = registers.stack_pointer().wrapping_sub(4);
                writes.extend([
                    (frame & 0x00ff_ffff, (value >> 16) as u16, data_fc),
                    (frame.wrapping_add(2) & 0x00ff_ffff, value as u16, data_fc),
                ]);
                expected.set_address_register(index, frame);
                expected.set_address_register(
                    7,
                    frame.wrapping_add_signed(i32::from(case.initial.prefetch[1] as i16)),
                );
                expected.pc = expected.pc.wrapping_add(2);
                for &(address, _, fc) in &writes {
                    contract.allow(AccessKind::Stack, true, address, fc);
                }
            }
            "UNLINK" => {
                let frame = registers.address_register(index).unwrap();
                let value = long(&memory, frame)?;
                for offset in [0, 2] {
                    contract.allow(
                        AccessKind::Stack,
                        false,
                        frame.wrapping_add(offset),
                        data_fc,
                    );
                }
                expected.set_address_register(7, frame.wrapping_add(4));
                expected.set_address_register(index, value);
            }
            "RTR" => {
                let sp = registers.stack_pointer();
                let ccr = word(&memory, sp)?;
                expected.pc = long(&memory, sp.wrapping_add(2))?;
                expected.set_address_register(7, sp.wrapping_add(6));
                expected.set_status_register((registers.status_register() & !31) | (ccr & 31));
                for offset in [0, 2, 4] {
                    contract.allow(AccessKind::Stack, false, sp.wrapping_add(offset), data_fc);
                }
            }
            "MOVEfromUSP" => {
                expected.set_address_register(index, registers.usp);
            }
            "MOVEtoUSP" => {
                expected.usp = registers.address_register(index).unwrap();
            }
            _ => {
                let mode = (op >> 3) & 7;
                let (value, address, cursor) = operand(case, &registers, &memory)?;
                expected.pc = cursor;
                if let Some(address) = address {
                    let fc = if family != "MOVEfromSR" && mode == 7 && matches!(index, 2 | 3) {
                        data_fc + 1
                    } else {
                        data_fc
                    };
                    contract.allow(AccessKind::Data, false, address, fc);
                    if family == "MOVEfromSR" {
                        writes.push((address, registers.status_register(), data_fc));
                        contract.allow(AccessKind::Data, true, address, data_fc);
                    }
                }
                if mode == 3 {
                    expected.set_address_register(
                        index,
                        registers.address_register(index).unwrap().wrapping_add(2),
                    );
                }
                if mode == 4 {
                    expected.set_address_register(
                        index,
                        registers.address_register(index).unwrap().wrapping_sub(2),
                    );
                }
                match family {
                    "MOVEfromSR" if mode == 0 => {
                        expected.d[index] = (registers.d[index] & 0xffff0000)
                            | u32::from(registers.status_register())
                    }
                    "MOVEfromSR" => {}
                    "MOVEtoCCR" => expected
                        .set_status_register((registers.status_register() & !31) | (value & 31)),
                    "MOVEtoSR" => expected.set_status_register(value),
                    _ => unreachable!(),
                }
            }
        }
    }
    let actual: Vec<_> = case
        .transactions
        .iter()
        .filter_map(|phase| {
            if let Phase::Write(transfer) = phase {
                Some(transfer)
            } else {
                None
            }
        })
        .collect();
    if actual.len() != writes.len()
        || actual
            .iter()
            .zip(writes)
            .any(|(transfer, (address, value, fc))| {
                transfer.address_bus != address
                    || transfer.data_bus != u32::from(value)
                    || transfer.function_code != fc
                    || transfer.uds != 1
                    || transfer.lds != 1
            })
    {
        return Err("stack/status writes contradict initial state".into());
    }
    if case.expected.registers()? != expected {
        return Err("stack/status registers contradict initial state".into());
    }
    let program_fc = data_fc + 1;
    if exception.is_some() {
        for offset in [0, 2] {
            contract.allow(
                AccessKind::Prefetch,
                false,
                expected.pc.wrapping_add(offset),
                6,
            );
        }
    } else {
        match family {
            "LINK" => {
                for offset in [4, 6] {
                    contract.allow(
                        AccessKind::Prefetch,
                        false,
                        registers.pc.wrapping_add(offset),
                        program_fc,
                    );
                }
            }
            "UNLINK" | "MOVEfromUSP" | "MOVEtoUSP" => contract.allow(
                AccessKind::Prefetch,
                false,
                registers.pc.wrapping_add(4),
                program_fc,
            ),
            "RTR" => {
                for offset in [0, 2] {
                    contract.allow(
                        AccessKind::Prefetch,
                        false,
                        expected.pc.wrapping_add(offset),
                        program_fc,
                    );
                }
            }
            _ => {
                let extensions = expected.pc.wrapping_sub(registers.pc.wrapping_add(2)) / 2;
                for index in 0..extensions {
                    contract.allow(
                        AccessKind::Prefetch,
                        false,
                        registers.pc.wrapping_add(4 + index * 2),
                        program_fc,
                    );
                }
                if family == "MOVEfromSR" {
                    contract.allow(
                        AccessKind::Prefetch,
                        false,
                        expected.pc.wrapping_add(2),
                        program_fc,
                    );
                } else {
                    let final_fc = if expected.supervisor() { 6 } else { 2 };
                    for offset in [0, 2] {
                        contract.allow(
                            AccessKind::Prefetch,
                            false,
                            expected.pc.wrapping_add(offset),
                            final_fc,
                        );
                    }
                }
            }
        }
    }
    Ok(Some(contract))
}
