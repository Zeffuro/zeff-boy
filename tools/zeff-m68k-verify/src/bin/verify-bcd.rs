use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs::File;
use std::io::Read;

use zeff_m68k::bus::{BusAccess, M68kBus};
use zeff_m68k::{CARRY, Cpu, EXTEND, StepOutcome, ZERO};

#[path = "verify_bcd/timing.rs"]
mod verify_bcd_timing;

struct OpcodeBus(u16);

impl M68kBus for OpcodeBus {
    type Error = ();

    fn read(&mut self, access: BusAccess) -> Result<u16, Self::Error> {
        if access.bus_address == 0 {
            Ok(self.0)
        } else {
            Err(())
        }
    }

    fn write(&mut self, _access: BusAccess, _value: u16) -> Result<(), Self::Error> {
        Err(())
    }
}

fn witness(opcode: u16, source: u8, destination: u8, x: bool, z: bool, expected: &[u8]) -> bool {
    let mut cpu = Cpu::new();
    for index in 0..8 {
        cpu.registers.d[index] = 0xabcd_1200 + index as u32 * 0x100;
    }
    cpu.registers.d[0] = 0xabcd_1200 | u32::from(source);
    cpu.registers.d[1] = 0xabcd_1300 | u32::from(destination);
    cpu.registers.ssp = 0x4000;
    cpu.registers.usp = 0x5000;
    cpu.registers.set_status_register(
        0x2500 | if x { CARRY | EXTEND } else { 0 } | if z { ZERO } else { 0 },
    );
    let mut registers = cpu.registers;
    let target = usize::from(opcode != 0x4800);
    registers.d[target] = (registers.d[target] & !255) | u32::from(expected[1]);
    registers.set_status_register(0x2500 | u16::from(expected[0]));
    registers.pc = 2;
    let outcome = cpu.step(&mut OpcodeBus(opcode));
    outcome
        == StepOutcome::Executed {
            opcode,
            nominal_cycles: 6,
        }
        && cpu.registers == registers
        && !cpu.stopped
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: verify-bcd <pinned-bcd-table.bin> [--timed-memory]")?;
    let timed = match args.next().as_deref() {
        None => false,
        Some("--timed-memory") => true,
        _ => return Err("unknown BCD verifier option".into()),
    };
    if args.next().is_some() {
        return Err("extra BCD verifier argument".into());
    }
    let mut table = Vec::new();
    File::open(path)?.take(1_050_625).read_to_end(&mut table)?;
    if table.len() != 1_050_624 || table.as_chunks::<2>().0.iter().any(|pair| pair[0] > 31) {
        return Err("invalid exhaustive BCD table size or CCR".into());
    }
    if Sha256::digest(&table)[..]
        != [
            0x84, 0x32, 0x86, 0x8c, 0x9a, 0xa9, 0x3c, 0x92, 0x57, 0x4b, 0xae, 0x48, 0xbe, 0xbd,
            0x4e, 0xfb, 0x28, 0x34, 0xe2, 0x98, 0x34, 0x0e, 0xb0, 0x95, 0x30, 0x62, 0x53, 0x65,
            0xb8, 0x01, 0x47, 0xe5,
        ]
    {
        return Err("BCD table provenance hash mismatch".into());
    }
    let mut offset = 0;
    let mut total = 0;
    let mut failures = 0;
    for opcode in [0xc300, 0x8300, 0x4800] {
        let count = if opcode == 0x4800 { 1 } else { 256 };
        let start = failures;
        for source in 0..=255 {
            for destination in 0..count {
                for x in [false, true] {
                    for z in [false, true] {
                        total += 1;
                        let check = if timed {
                            verify_bcd_timing::witness
                        } else {
                            witness
                        };
                        if !check(
                            opcode,
                            source,
                            destination as u8,
                            x,
                            z,
                            &table[offset..offset + 2],
                        ) {
                            failures += 1;
                            if failures <= 20 {
                                eprintln!(
                                    "BCD {opcode:04x}: S={source:02x} D={destination:02x} X={x} Z={z}"
                                );
                            }
                        }
                        offset += 2;
                    }
                }
            }
        }
        println!(
            "opcode {opcode:04x}: {} witnesses; {} failures",
            256 * count * 4,
            failures - start
        );
    }
    assert_eq!(offset, table.len());
    println!("total: {total} complete BCD witnesses; {failures} failures");
    if failures != 0 {
        return Err("BCD hardware-backed reference mismatch".into());
    }
    Ok(())
}
