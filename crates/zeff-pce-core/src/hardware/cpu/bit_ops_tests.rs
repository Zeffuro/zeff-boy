use std::collections::HashMap;

use super::{Cpu, CpuBus, StatusFlags};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read(u32),
    Write(u32, u8),
    Idle,
}

#[derive(Default)]
struct Bus {
    memory: HashMap<u32, u8>,
    accesses: Vec<Access>,
}

impl CpuBus for Bus {
    fn read(&mut self, physical_addr: u32) -> u8 {
        self.accesses.push(Access::Read(physical_addr));
        self.memory.get(&physical_addr).copied().unwrap_or(0xFF)
    }

    fn write(&mut self, physical_addr: u32, value: u8) {
        self.accesses.push(Access::Write(physical_addr, value));
        self.memory.insert(physical_addr, value);
    }

    fn idle(&mut self) {
        self.accesses.push(Access::Idle);
    }
}

#[test]
fn immediate_bit_sources_negative_and_overflow_from_the_operand() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.registers_mut().a = 0x0F;
    cpu.registers_mut().status =
        StatusFlags::CARRY | StatusFlags::NEGATIVE | StatusFlags::MEMORY_OPERATION;
    bus.memory.insert(0, 0x89);
    bus.memory.insert(1, 0x40);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 2);
    assert_eq!(
        cpu.registers().status,
        StatusFlags::CARRY | StatusFlags::ZERO | StatusFlags::OVERFLOW
    );
    assert_eq!(bus.accesses, [Access::Read(0), Access::Read(1)]);
}

#[test]
fn rmb_uses_mpr1_and_two_idle_cycles_before_its_only_write() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(1, 0x20);
    cpu.registers_mut().status =
        StatusFlags::CARRY | StatusFlags::NEGATIVE | StatusFlags::MEMORY_OPERATION;
    bus.memory.insert(0, 0x47);
    bus.memory.insert(1, 0xFF);
    bus.memory.insert(0x0400FF, 0xFF);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 7);
    assert_eq!(bus.memory.get(&0x0400FF), Some(&0xEF));
    assert_eq!(
        cpu.registers().status,
        StatusFlags::CARRY | StatusFlags::NEGATIVE
    );
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0),
            Access::Read(1),
            Access::Idle,
            Access::Read(0x0400FF),
            Access::Idle,
            Access::Idle,
            Access::Write(0x0400FF, 0xEF),
        ]
    );
}

#[test]
fn taken_bbs_wraps_instruction_fetch_and_relative_target() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(0, 0x10);
    cpu.set_mapping_register(1, 0x20);
    cpu.set_mapping_register(7, 0x30);
    cpu.registers_mut().pc = 0xFFFE;
    bus.memory.insert(0x061FFE, 0xFF);
    bus.memory.insert(0x061FFF, 0x80);
    bus.memory.insert(0x020000, 0xFE);
    bus.memory.insert(0x040080, 0x80);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 8);
    assert_eq!(cpu.registers().pc, 0xFFFF);
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0x061FFE),
            Access::Read(0x061FFF),
            Access::Idle,
            Access::Read(0x020000),
            Access::Idle,
            Access::Read(0x040080),
            Access::Idle,
            Access::Idle,
        ]
    );
}

#[test]
fn absolute_indexed_tst_uses_its_immediate_mask_and_wraps() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.registers_mut().a = 0;
    cpu.registers_mut().x = 1;
    cpu.registers_mut().status = StatusFlags::CARRY;
    bus.memory.insert(0, 0xB3);
    bus.memory.insert(1, 0x0F);
    bus.memory.insert(2, 0xFF);
    bus.memory.insert(3, 0xFF);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 8);
    assert_eq!(
        cpu.registers().status,
        StatusFlags::CARRY | StatusFlags::NEGATIVE
    );
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0),
            Access::Read(1),
            Access::Read(2),
            Access::Read(3),
            Access::Idle,
            Access::Idle,
            Access::Read(0),
            Access::Idle,
        ]
    );
}

#[test]
fn tsb_writes_even_when_the_value_is_unchanged_and_uses_old_flags() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(1, 1);
    cpu.registers_mut().a = 0;
    cpu.registers_mut().status = StatusFlags::CARRY;
    bus.memory.insert(0, 0x04);
    bus.memory.insert(1, 0x80);
    bus.memory.insert(0x2080, 0xC0);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 6);
    assert_eq!(
        cpu.registers().status,
        StatusFlags::CARRY | StatusFlags::ZERO | StatusFlags::OVERFLOW | StatusFlags::NEGATIVE
    );
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0),
            Access::Read(1),
            Access::Idle,
            Access::Read(0x2080),
            Access::Idle,
            Access::Write(0x2080, 0xC0),
        ]
    );
}

#[test]
fn bbr_not_taken_stops_after_the_direct_page_read() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(1, 1);
    bus.memory.insert(0, 0x0F);
    bus.memory.insert(1, 0x80);
    bus.memory.insert(2, 0x7F);
    bus.memory.insert(0x2080, 0x01);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 6);
    assert_eq!(cpu.registers().pc, 3);
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0),
            Access::Read(1),
            Access::Idle,
            Access::Read(2),
            Access::Idle,
            Access::Read(0x2080),
        ]
    );
}
