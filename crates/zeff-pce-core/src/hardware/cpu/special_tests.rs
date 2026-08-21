use std::collections::HashMap;

use super::{Cpu, CpuBus, StatusFlags, VdcPort};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read(u32),
    DummyRead(u32),
    Write(u32, u8),
    VdcWrite(VdcPort, u8),
    Idle,
}

struct Bus {
    memory: HashMap<u32, u8>,
    accesses: Vec<Access>,
    trace_limit: usize,
    cycles: u32,
    writes: u32,
    last_write: Option<(u32, u8)>,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            memory: HashMap::new(),
            accesses: Vec::new(),
            trace_limit: usize::MAX,
            cycles: 0,
            writes: 0,
            last_write: None,
        }
    }
}

impl Bus {
    fn record(&mut self, access: Access) {
        if self.accesses.len() < self.trace_limit {
            self.accesses.push(access);
        }
        self.cycles += 1;
    }
}

impl CpuBus for Bus {
    fn read(&mut self, physical_addr: u32) -> u8 {
        let value = self
            .memory
            .get(&physical_addr)
            .copied()
            .unwrap_or(physical_addr as u8);
        self.record(Access::Read(physical_addr));
        value
    }

    fn write(&mut self, physical_addr: u32, value: u8) {
        self.memory.insert(physical_addr, value);
        self.writes += 1;
        self.last_write = Some((physical_addr, value));
        self.record(Access::Write(physical_addr, value));
    }

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        let value = self
            .memory
            .get(&physical_addr)
            .copied()
            .unwrap_or(physical_addr as u8);
        self.record(Access::DummyRead(physical_addr));
        value
    }

    fn idle(&mut self) {
        self.record(Access::Idle);
    }

    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        self.record(Access::VdcWrite(port, value));
    }
}

#[test]
fn immediate_vdc_stores_use_the_dedicated_ports_on_the_final_cycle() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    bus.memory.extend([
        (0, 0x03),
        (1, 0xA5),
        (2, 0x13),
        (3, 0xB6),
        (4, 0x23),
        (5, 0xC7),
    ]);

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 4);
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 4);
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 4);

    assert_eq!(
        bus.accesses,
        [
            Access::Read(0),
            Access::Read(1),
            Access::Idle,
            Access::VdcWrite(VdcPort::SelectOrStatus, 0xA5),
            Access::Read(2),
            Access::Read(3),
            Access::Idle,
            Access::VdcWrite(VdcPort::DataLow, 0xB6),
            Access::Read(4),
            Access::Read(5),
            Access::Idle,
            Access::VdcWrite(VdcPort::DataHigh, 0xC7),
        ]
    );
}

#[test]
fn one_byte_tii_matches_the_stack_parameter_and_mapped_transfer_pattern() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(1, 0x10);
    cpu.set_mapping_register(2, 0x20);
    cpu.registers_mut().a = 0xAA;
    cpu.registers_mut().x = 0xBB;
    cpu.registers_mut().y = 0xCC;
    cpu.registers_mut().sp = 0x80;
    cpu.registers_mut().status = StatusFlags::MEMORY_OPERATION;
    bus.memory.extend([
        (0, 0x73),
        (1, 0x01),
        (2, 0x20),
        (3, 0x02),
        (4, 0x40),
        (5, 0x01),
        (6, 0x00),
        (0x020001, 0x5A),
    ]);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 23);
    assert_eq!(cpu.registers().pc, 7);
    assert_eq!(cpu.registers().sp, 0x80);
    assert_eq!(
        (cpu.registers().a, cpu.registers().x, cpu.registers().y),
        (0xAA, 0xBB, 0xCC)
    );
    assert!(cpu.registers().status.is_empty());
    assert_eq!(bus.memory.get(&0x040002), Some(&0x5A));
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0),
            Access::DummyRead(1),
            Access::Idle,
            Access::Write(0x020180, 0xCC),
            Access::Write(0x02017F, 0xAA),
            Access::Write(0x02017E, 0xBB),
            Access::Read(1),
            Access::Read(2),
            Access::Read(3),
            Access::Read(4),
            Access::Read(5),
            Access::Read(6),
            Access::Idle,
            Access::Idle,
            Access::Read(0x020001),
            Access::Idle,
            Access::Write(0x040002, 0x5A),
            Access::Idle,
            Access::Idle,
            Access::Idle,
            Access::Read(0x02017E),
            Access::Read(0x02017F),
            Access::Read(0x020180),
        ]
    );
}

#[test]
fn zero_block_length_transfers_65536_bytes_and_wraps_both_addresses() {
    let mut cpu = Cpu::new();
    let mut bus = Bus {
        trace_limit: 0,
        ..Bus::default()
    };
    for page in 0..8 {
        cpu.set_mapping_register(page, 0x10 + page as u8);
    }
    cpu.registers_mut().pc = 0x8000;
    cpu.registers_mut().sp = 0x80;
    let code_base = 0x14 << 13;
    bus.memory.extend([
        (code_base, 0x73),
        (code_base + 1, 0x00),
        (code_base + 2, 0x00),
        (code_base + 3, 0x00),
        (code_base + 4, 0x20),
        (code_base + 5, 0x00),
        (code_base + 6, 0x00),
    ]);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 393_233);
    assert_eq!(bus.cycles, 393_233);
    assert_eq!(bus.writes, 65_539);
    assert_eq!(bus.last_write, Some((0x021FFF, 0xFF)));
    assert_eq!(cpu.registers().pc, 0x8007);
    assert_eq!(cpu.registers().sp, 0x80);
}

#[test]
fn alternating_block_modes_toggle_one_side_and_increment_the_other() {
    let mut tia_cpu = Cpu::new();
    let mut tia_bus = Bus::default();
    tia_cpu.set_mapping_register(1, 1);
    tia_cpu.set_mapping_register(2, 2);
    load_block_case(&mut tia_bus, 0xE3, 0x3000, 0x5000, 4);
    tia_bus
        .memory
        .extend([(0x3000, 1), (0x3001, 2), (0x3002, 3), (0x3003, 4)]);

    tia_cpu.step(&mut tia_bus).unwrap();

    assert_eq!(tia_bus.memory.get(&0x5000), Some(&3));
    assert_eq!(tia_bus.memory.get(&0x5001), Some(&4));

    let mut tai_cpu = Cpu::new();
    let mut tai_bus = Bus::default();
    tai_cpu.set_mapping_register(1, 1);
    tai_cpu.set_mapping_register(2, 2);
    load_block_case(&mut tai_bus, 0xF3, 0x3000, 0x5000, 4);
    tai_bus.memory.extend([(0x3000, 0xA5), (0x3001, 0x5A)]);

    tai_cpu.step(&mut tai_bus).unwrap();

    assert_eq!(tai_bus.memory.get(&0x5000), Some(&0xA5));
    assert_eq!(tai_bus.memory.get(&0x5001), Some(&0x5A));
    assert_eq!(tai_bus.memory.get(&0x5002), Some(&0xA5));
    assert_eq!(tai_bus.memory.get(&0x5003), Some(&0x5A));
}

#[test]
fn block_transfer_can_overwrite_a_saved_register_on_the_stack() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.registers_mut().a = 0xAA;
    cpu.registers_mut().x = 0xBB;
    cpu.registers_mut().y = 0xCC;
    cpu.registers_mut().sp = 0x80;
    load_block_case(&mut bus, 0x73, 0x3000, 0x217E, 1);
    bus.memory.insert(0x1000, 0x5A);

    cpu.step(&mut bus).unwrap();

    assert_eq!(cpu.registers().x, 0x5A);
    assert_eq!(cpu.registers().a, 0xAA);
    assert_eq!(cpu.registers().y, 0xCC);
    assert_eq!(cpu.registers().sp, 0x80);
}

#[test]
fn register_swaps_and_clears_preserve_status() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.registers_mut().a = 0x12;
    cpu.registers_mut().x = 0x34;
    cpu.registers_mut().y = 0x56;
    cpu.registers_mut().status =
        StatusFlags::CARRY | StatusFlags::NEGATIVE | StatusFlags::MEMORY_OPERATION;
    bus.memory
        .extend([(0, 0x02), (1, 0x62), (2, 0x82), (3, 0xC2)]);

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 3);
    assert_eq!((cpu.registers().x, cpu.registers().y), (0x56, 0x34));
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 2);
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 2);
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 2);
    assert_eq!(
        (cpu.registers().a, cpu.registers().x, cpu.registers().y),
        (0, 0, 0)
    );
    assert_eq!(
        cpu.registers().status,
        StatusFlags::CARRY | StatusFlags::NEGATIVE
    );
}

#[test]
fn tdd_and_tin_apply_their_distinct_address_updates() {
    let mut tdd_cpu = Cpu::new();
    let mut tdd_bus = Bus::default();
    tdd_cpu.set_mapping_register(1, 1);
    tdd_cpu.set_mapping_register(2, 2);
    load_block_case(&mut tdd_bus, 0xC3, 0x3001, 0x5001, 2);
    tdd_bus.memory.extend([(0x3000, 0xA0), (0x3001, 0xA1)]);

    assert_eq!(tdd_cpu.step(&mut tdd_bus).unwrap().cycles, 29);
    assert_eq!(tdd_bus.memory.get(&0x5000), Some(&0xA0));
    assert_eq!(tdd_bus.memory.get(&0x5001), Some(&0xA1));

    let mut tin_cpu = Cpu::new();
    let mut tin_bus = Bus::default();
    tin_cpu.set_mapping_register(1, 1);
    tin_cpu.set_mapping_register(2, 2);
    load_block_case(&mut tin_bus, 0xD3, 0x3000, 0x5000, 2);
    tin_bus.memory.extend([(0x3000, 0xB0), (0x3001, 0xB1)]);

    assert_eq!(tin_cpu.step(&mut tin_bus).unwrap().cycles, 29);
    assert_eq!(tin_bus.memory.get(&0x5000), Some(&0xB1));
}

fn load_block_case(bus: &mut Bus, opcode: u8, source: u16, destination: u16, length: u16) {
    bus.memory.insert(0, opcode);
    for (offset, value) in [source, destination, length]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .enumerate()
    {
        bus.memory.insert(offset as u32 + 1, value);
    }
}
