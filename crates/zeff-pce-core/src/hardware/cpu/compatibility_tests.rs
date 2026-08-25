use std::collections::HashMap;

use super::{Cpu, CpuBus, CpuStep, RESERVED_COMPATIBILITY_NOP_OPCODES, Registers, StatusFlags};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BusAccess {
    Read(u32),
    DummyRead(u32),
}

#[derive(Default)]
struct TestBus {
    values: HashMap<u32, u8>,
    accesses: Vec<BusAccess>,
}

impl CpuBus for TestBus {
    fn read(&mut self, physical_addr: u32) -> u8 {
        self.accesses.push(BusAccess::Read(physical_addr));
        self.values.get(&physical_addr).copied().unwrap_or(0xFF)
    }

    fn write(&mut self, _physical_addr: u32, _value: u8) {
        panic!("reserved compatibility NOP wrote to the bus");
    }

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        self.accesses.push(BusAccess::DummyRead(physical_addr));
        self.values.get(&physical_addr).copied().unwrap_or(0xFF)
    }
}

#[test]
fn reserved_compatibility_nops_share_the_two_cycle_trace() {
    for opcode in RESERVED_COMPATIBILITY_NOP_OPCODES {
        let mut cpu = Cpu::new();
        let initial = Registers {
            a: 0x12,
            x: 0x34,
            y: 0x56,
            sp: 0x78,
            pc: 0x4000,
            status: StatusFlags::from_bits_retain(0xFF),
        };
        *cpu.registers_mut() = initial;
        for (index, value) in [0x80, 0x81, 0x12, 0x83, 0x84, 0x85, 0x86, 0x87]
            .into_iter()
            .enumerate()
        {
            cpu.set_mapping_register(index, value);
        }
        let mappings = cpu.mapping_registers();
        let mut bus = TestBus::default();
        bus.values.insert(0x024000, opcode);
        bus.values.insert(0x024001, 0xA5);

        assert_eq!(
            cpu.step(&mut bus),
            Ok(CpuStep {
                pc: 0x4000,
                physical_pc: 0x024000,
                opcode,
                cycles: 2,
            })
        );
        assert_eq!(
            cpu.registers(),
            Registers {
                pc: 0x4001,
                status: initial.status - StatusFlags::MEMORY_OPERATION,
                ..initial
            }
        );
        assert_eq!(cpu.mapping_registers(), mappings);
        assert_eq!(
            bus.accesses,
            [BusAccess::Read(0x024000), BusAccess::DummyRead(0x024001)]
        );
    }
}
