use std::collections::HashMap;

use super::{Cpu, CpuBus, StatusFlags};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read(u32),
    DummyRead(u32),
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

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        self.accesses.push(Access::DummyRead(physical_addr));
        self.memory.get(&physical_addr).copied().unwrap_or(0xFF)
    }

    fn idle(&mut self) {
        self.accesses.push(Access::Idle);
    }
}

#[test]
fn jsr_pushes_the_return_address_before_fetching_the_high_operand() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(0, 0x10);
    cpu.set_mapping_register(1, 0x20);
    cpu.registers_mut().pc = 0x1FFE;
    cpu.registers_mut().sp = 0xFD;
    bus.memory.insert(0x021FFE, 0x20);
    bus.memory.insert(0x021FFF, 0x34);
    bus.memory.insert(0x040000, 0x12);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 7);
    assert_eq!(cpu.registers().pc, 0x1234);
    assert_eq!(cpu.registers().sp, 0xFB);
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0x021FFE),
            Access::Read(0x021FFF),
            Access::Idle,
            Access::Write(0x0401FD, 0x20),
            Access::Write(0x0401FC, 0x00),
            Access::Read(0x040000),
            Access::Idle,
        ]
    );
}

#[test]
fn brk_uses_mpr7_and_pushes_break_without_memory_operation() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(1, 0x20);
    cpu.set_mapping_register(2, 0x10);
    cpu.set_mapping_register(7, 0x30);
    cpu.registers_mut().pc = 0x4000;
    cpu.registers_mut().sp = 0x80;
    cpu.registers_mut().status =
        StatusFlags::CARRY | StatusFlags::DECIMAL | StatusFlags::MEMORY_OPERATION;
    bus.memory.insert(0x020000, 0x00);
    bus.memory.insert(0x020001, 0xA5);
    bus.memory.insert(0x061FF6, 0x34);
    bus.memory.insert(0x061FF7, 0x12);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 8);
    assert_eq!(cpu.registers().pc, 0x1234);
    assert_eq!(cpu.registers().sp, 0x7D);
    assert_eq!(
        cpu.registers().status,
        StatusFlags::CARRY | StatusFlags::INTERRUPT
    );
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0x020000),
            Access::Read(0x020001),
            Access::Write(0x040180, 0x40),
            Access::Write(0x04017F, 0x02),
            Access::Write(0x04017E, 0x19),
            Access::Read(0x061FF6),
            Access::Read(0x061FF7),
            Access::Idle,
        ]
    );
}

#[test]
fn plp_restores_memory_operation_but_strips_break() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(1, 1);
    cpu.registers_mut().sp = 0xFF;
    bus.memory.insert(0, 0x28);
    bus.memory.insert(1, 0xEA);
    bus.memory.insert(0x2100, 0xFF);

    let step = cpu.step(&mut bus).unwrap();

    assert_eq!(step.cycles, 4);
    assert_eq!(cpu.registers().sp, 0);
    assert_eq!(
        cpu.registers().status,
        StatusFlags::from_bits_retain(0xFF) & !StatusFlags::BREAK
    );
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0),
            Access::DummyRead(1),
            Access::Idle,
            Access::Read(0x2100),
        ]
    );
}

#[test]
fn bsr_and_rts_preserve_the_return_address_across_stack_wrap() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(1, 1);
    bus.memory.insert(0, 0x44);
    bus.memory.insert(1, 0x02);
    bus.memory.insert(4, 0x60);

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 8);
    assert_eq!(cpu.registers().pc, 4);
    assert_eq!(cpu.registers().sp, 0xFE);
    assert_eq!(bus.memory.get(&0x2100), Some(&0));
    assert_eq!(bus.memory.get(&0x21FF), Some(&1));

    bus.accesses.clear();
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 7);
    assert_eq!(cpu.registers().pc, 2);
    assert_eq!(cpu.registers().sp, 0);
    assert_eq!(
        bus.accesses,
        [
            Access::Read(4),
            Access::DummyRead(5),
            Access::Idle,
            Access::Read(0x21FF),
            Access::Read(0x2100),
            Access::Idle,
            Access::Idle,
        ]
    );
}

#[test]
fn rti_restores_memory_operation_for_the_following_instruction() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.registers_mut().a = 0xA5;
    cpu.registers_mut().x = 0x10;
    cpu.registers_mut().sp = 0xFC;
    bus.memory.insert(0, 0x40);
    bus.memory.insert(1, 0x69);
    bus.memory.insert(2, 0x02);
    bus.memory.insert(0x10, 0x01);
    bus.memory.insert(0x01FD, 0x30);
    bus.memory.insert(0x01FE, 0x01);
    bus.memory.insert(0x01FF, 0x00);

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 7);
    assert_eq!(cpu.registers().pc, 1);
    assert!(
        cpu.registers()
            .status
            .contains(StatusFlags::MEMORY_OPERATION)
    );
    assert!(!cpu.registers().status.contains(StatusFlags::BREAK));

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 5);
    assert_eq!(cpu.registers().a, 0xA5);
    assert_eq!(bus.memory.get(&0x10), Some(&0x03));
}

#[test]
fn conditional_branch_has_no_page_cross_penalty() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(0, 0x10);
    cpu.set_mapping_register(1, 0x20);
    cpu.registers_mut().pc = 0x1FFE;
    bus.memory.insert(0x021FFE, 0xD0);
    bus.memory.insert(0x021FFF, 0x7F);

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 4);
    assert_eq!(cpu.registers().pc, 0x207F);
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0x021FFE),
            Access::Read(0x021FFF),
            Access::DummyRead(0x040000),
            Access::Idle,
        ]
    );

    cpu.registers_mut().pc = 0x1FFE;
    cpu.registers_mut().status.insert(StatusFlags::ZERO);
    bus.accesses.clear();
    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 2);
    assert_eq!(cpu.registers().pc, 0x2000);
    assert_eq!(
        bus.accesses,
        [Access::Read(0x021FFE), Access::Read(0x021FFF)]
    );
}

#[test]
fn indirect_jump_wraps_its_pointer_at_ffff() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::default();
    cpu.set_mapping_register(7, 2);
    bus.memory.insert(0, 0x6C);
    bus.memory.insert(1, 0xFF);
    bus.memory.insert(2, 0xFF);
    bus.memory.insert(0x5FFF, 0x34);

    assert_eq!(cpu.step(&mut bus).unwrap().cycles, 7);
    assert_eq!(cpu.registers().pc, 0x6C34);
    assert_eq!(
        bus.accesses,
        [
            Access::Read(0),
            Access::Read(1),
            Access::Read(2),
            Access::Idle,
            Access::Read(0x5FFF),
            Access::Read(0),
            Access::Idle,
        ]
    );
}
