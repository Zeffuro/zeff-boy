mod addressing;
mod alu;
mod bit_ops;
mod control_flow;
mod instructions;
mod modify;
mod on_chip;
mod registers;
mod special;

pub use on_chip::{
    HuC6280, InterruptSource, InterruptStep, LineLevel, OnChipIo,
    PROVISIONAL_INTERRUPT_ENTRY_CYCLES, TIMER_MASTER_TICKS, UNINITIALIZED_TIMER_COUNTER_READ,
};
pub use registers::{Registers, StatusFlags};

const LOGICAL_PAGE_SHIFT: u32 = 13;
const LOGICAL_PAGE_MASK: u16 = (1 << LOGICAL_PAGE_SHIFT) - 1;
const MPR_COUNT: usize = 8;
const RESERVED_COMPATIBILITY_NOP_OPCODES: [u8; 22] = [
    0x0B, 0x1B, 0x2B, 0x33, 0x3B, 0x4B, 0x5B, 0x5C, 0x63, 0x6B, 0x7B, 0x8B, 0x9B, 0xAB, 0xBB, 0xCB,
    0xDB, 0xDC, 0xE2, 0xEB, 0xFB, 0xFC,
];

pub const PHYSICAL_ADDRESS_BITS: u8 = 21;
pub const PHYSICAL_ADDRESS_MASK: u32 = (1 << PHYSICAL_ADDRESS_BITS) - 1;
pub const RESET_VECTOR_LOW: u32 = 0x0000_1FFE;
pub const RESET_VECTOR_HIGH: u32 = 0x0000_1FFF;

#[inline]
pub const fn physical_address_for_page(logical_addr: u16, physical_page: u8) -> u32 {
    (physical_page as u32) << LOGICAL_PAGE_SHIFT | (logical_addr & LOGICAL_PAGE_MASK) as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VdcPort {
    SelectOrStatus = 0,
    Unused = 1,
    DataLow = 2,
    DataHigh = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerPort {
    CounterReload,
    Control,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrqPort {
    Disable,
    Request,
}

impl VdcPort {
    #[inline]
    pub const fn offset(self) -> u8 {
        self as u8
    }
}

pub trait CpuBus {
    fn read(&mut self, physical_addr: u32) -> u8;
    fn write(&mut self, physical_addr: u32, value: u8);

    fn observe_logical_read(
        &mut self,
        _logical_addr: u16,
        _physical_addr: u32,
        _value: u8,
        _dummy: bool,
    ) {
    }

    fn observe_logical_write(
        &mut self,
        _logical_addr: u16,
        _physical_addr: u32,
        _value: u8,
        _dummy: bool,
    ) {
    }

    fn observe_instruction_byte(&mut self, _logical_addr: u16, _physical_addr: u32, _value: u8) {}

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        self.read(physical_addr)
    }

    fn dummy_write(&mut self, physical_addr: u32, value: u8) {
        self.write(physical_addr, value);
    }

    fn write_vdc(&mut self, _port: VdcPort, _value: u8) {
        self.idle();
    }

    fn advance_internal_access(&mut self, _physical_addr: u32, _write: bool) -> bool {
        true
    }

    fn take_elapsed_master_ticks(&mut self) -> u64 {
        0
    }

    fn observe_internal_read(&mut self, _physical_addr: u32, _value: u8, _dummy: bool) {}

    fn observe_internal_write(&mut self, _physical_addr: u32, _value: u8, _dummy: bool) {}

    fn idle(&mut self) {}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeedMode {
    #[default]
    Low,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuStep {
    pub pc: u16,
    pub opcode: u8,
    pub cycles: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuTrap {
    UnsupportedOpcode { pc: u16, opcode: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu {
    registers: Registers,
    mpr: [u8; MPR_COUNT],
    speed_mode: SpeedMode,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            registers: Registers::default(),
            mpr: [0; MPR_COUNT],
            speed_mode: SpeedMode::Low,
        }
    }

    pub fn reset<B: CpuBus>(&mut self, bus: &mut B) {
        self.mpr[7] = 0;
        self.registers.status.insert(StatusFlags::INTERRUPT);
        self.registers
            .status
            .remove(StatusFlags::DECIMAL | StatusFlags::MEMORY_OPERATION);
        self.speed_mode = SpeedMode::Low;

        let low = bus.read(RESET_VECTOR_LOW);
        let high = bus.read(RESET_VECTOR_HIGH);
        self.registers.pc = u16::from_le_bytes([low, high]);
    }

    pub fn step<B: CpuBus>(&mut self, bus: &mut B) -> Result<CpuStep, CpuTrap> {
        let pc = self.registers.pc;
        let memory_operation = self
            .registers
            .status
            .contains(StatusFlags::MEMORY_OPERATION);
        let opcode = self.fetch(bus);

        self.registers.status.remove(StatusFlags::MEMORY_OPERATION);

        let cycles = match opcode {
            0x43 => self.execute_tma(bus),
            0x53 => self.execute_tam(bus),
            0x54 => {
                self.speed_mode = SpeedMode::Low;
                self.dummy_fetch(bus);
                bus.idle();
                3
            }
            0xD4 => {
                self.speed_mode = SpeedMode::High;
                self.dummy_fetch(bus);
                bus.idle();
                3
            }
            0xEA => {
                self.dummy_fetch(bus);
                2
            }
            opcode if RESERVED_COMPATIBILITY_NOP_OPCODES.contains(&opcode) => {
                self.dummy_fetch(bus);
                2
            }
            0xF4 => {
                self.registers.status.insert(StatusFlags::MEMORY_OPERATION);
                self.dummy_fetch(bus);
                2
            }
            _ => {
                if let Some(cycles) = self.execute_special(bus, opcode) {
                    cycles
                } else if let Some(cycles) = self.execute_bit_operations(bus, opcode) {
                    cycles
                } else if let Some(cycles) = self.execute_control(bus, opcode) {
                    cycles
                } else if let Some(cycles) = self.execute_baseline(bus, opcode) {
                    cycles
                } else if let Some(cycles) = self.execute_modify(bus, opcode) {
                    cycles
                } else if let Some(cycles) = self.execute_alu(bus, opcode, memory_operation) {
                    cycles
                } else {
                    return Err(CpuTrap::UnsupportedOpcode { pc, opcode });
                }
            }
        };

        Ok(CpuStep { pc, opcode, cycles })
    }

    #[inline]
    pub fn registers(&self) -> Registers {
        self.registers
    }

    #[inline]
    pub fn registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }

    #[inline]
    pub fn mapping_registers(&self) -> [u8; MPR_COUNT] {
        self.mpr
    }

    #[inline]
    pub fn mapping_register(&self, index: usize) -> u8 {
        self.mpr[index]
    }

    #[inline]
    pub fn set_mapping_register(&mut self, index: usize, value: u8) {
        self.mpr[index] = value;
    }

    #[inline]
    pub fn speed_mode(&self) -> SpeedMode {
        self.speed_mode
    }

    #[inline]
    pub fn set_speed_mode(&mut self, speed_mode: SpeedMode) {
        self.speed_mode = speed_mode;
    }

    #[inline]
    pub fn logical_to_physical(&self, logical_addr: u16) -> u32 {
        let page = usize::from(logical_addr >> LOGICAL_PAGE_SHIFT);
        physical_address_for_page(logical_addr, self.mpr[page])
    }

    #[inline]
    pub fn read<B: CpuBus>(&self, bus: &mut B, logical_addr: u16) -> u8 {
        let physical_addr = self.logical_to_physical(logical_addr);
        let value = bus.read(physical_addr);
        bus.observe_logical_read(logical_addr, physical_addr, value, false);
        value
    }

    #[inline]
    pub fn write<B: CpuBus>(&self, bus: &mut B, logical_addr: u16, value: u8) {
        let physical_addr = self.logical_to_physical(logical_addr);
        bus.write(physical_addr, value);
        bus.observe_logical_write(logical_addr, physical_addr, value, false);
    }

    fn execute_tma<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        let selection = self.fetch(bus);
        bus.idle();
        bus.idle();
        if selection != 0 {
            self.registers.a = self
                .mpr
                .iter()
                .enumerate()
                .filter(|(index, _)| selection & (1 << index) != 0)
                .fold(0, |combined, (_, value)| combined | value);
        }
        4
    }

    fn execute_tam<B: CpuBus>(&mut self, bus: &mut B) -> u32 {
        let selection = self.fetch(bus);
        bus.idle();
        bus.idle();
        bus.idle();
        for index in 0..MPR_COUNT {
            if selection & (1 << index) != 0 {
                self.mpr[index] = self.registers.a;
            }
        }
        5
    }

    #[inline]
    pub(super) fn fetch<B: CpuBus>(&mut self, bus: &mut B) -> u8 {
        let logical_addr = self.registers.pc;
        let physical_addr = self.logical_to_physical(logical_addr);
        let value = bus.read(physical_addr);
        bus.observe_logical_read(logical_addr, physical_addr, value, false);
        bus.observe_instruction_byte(logical_addr, physical_addr, value);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        value
    }

    #[inline]
    pub(super) fn dummy_fetch<B: CpuBus>(&self, bus: &mut B) {
        self.dummy_read(bus, self.registers.pc);
    }

    #[inline]
    pub(super) fn dummy_read<B: CpuBus>(&self, bus: &mut B, logical_addr: u16) {
        let physical_addr = self.logical_to_physical(logical_addr);
        let value = bus.dummy_read(physical_addr);
        bus.observe_logical_read(logical_addr, physical_addr, value, true);
    }
}

#[cfg(test)]
mod bit_ops_tests;
#[cfg(test)]
mod compatibility_tests;
#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
mod control_flow_tests;
#[cfg(test)]
mod on_chip_tests;
#[cfg(test)]
mod special_tests;
#[cfg(test)]
mod tests;
