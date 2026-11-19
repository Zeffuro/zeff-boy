mod candidate_bus;
mod rollback_memory;
mod svp_reads;
use crate::cartridge::Cartridge;
use crate::constants::*;
use crate::hardware::audio::Audio;
use std::cell::Cell;
use zeff_m68k::bus::{BUS_ADDRESS_MASK, Direction};
use zeff_z80::{CpuTrap, Z80Bus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundFault {
    ConcurrentSchedulingEnabled,
    ExpansionUnavailable,
    InputBlocked(crate::InputFault),
    MainCpuRunning,
    MainInterruptPending,
    ResetAsserted,
    BusRequested,
    UnsupportedOpcode { pc: u16, opcode: u8 },
    UnsupportedAccess { address: u16, direction: Direction },
    UnsupportedPort { port: u8, direction: Direction },
    UnstableOpn2Read { pc: u16 },
    UnstableCartridgeRead { pc: u16 },
    CpuTrap(CpuTrap),
    VdpTiming(crate::VdpTimingError),
    ClockOverflow,
}

#[cfg(feature = "sound-shadow-observer")]
pub(crate) mod observer;
#[cfg(test)]
mod observer_contract_tests;
#[cfg(test)]
mod opn2_tests;
mod ram;
mod registers;
use registers::RegisterWrites;
mod sf001;
use sf001::Sf001Writes;
mod sf002;
use sf002::Sf002Writes;
pub(crate) mod schedule;
mod state;
use ram::SoundRam;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SoundMemory {
    pub ram: SoundRam,
    pub bank: u16,
    pub requested: bool,
    pub granted: bool,
    pub reset_asserted: bool,
    pub irq_asserted: bool,
}

impl Default for SoundMemory {
    fn default() -> Self {
        Self {
            ram: SoundRam::default(),
            bank: 0,
            requested: false,
            granted: false,
            reset_asserted: true,
            irq_asserted: false,
        }
    }
}

impl SoundMemory {
    fn snapshot(&self) -> Self {
        Self {
            ram: self.ram.snapshot(),
            bank: self.bank,
            requested: self.requested,
            granted: self.granted,
            reset_asserted: self.reset_asserted,
            irq_asserted: self.irq_asserted,
        }
    }

    pub fn complete_boundary(&mut self) {
        self.granted = self.requested && !self.reset_asserted;
    }

    pub fn request(&mut self, requested: bool) {
        self.requested = requested;
        if !requested {
            self.granted = false;
        }
    }

    pub fn set_reset(&mut self, asserted: bool) {
        self.reset_asserted = asserted;
        self.granted = false;
    }

    pub fn read(&self, address: u16) -> Option<u8> {
        match address {
            0..=SOUND_RAM_END => Some(self.ram[usize::from(address) & SOUND_RAM_MASK]),
            SOUND_BANK_START..=SOUND_BANK_END => Some(SOUND_DISCONNECTED_BYTE),
            SOUND_UNUSED_START..=SOUND_UNUSED_END => Some(SOUND_DISCONNECTED_BYTE),
            _ => None,
        }
    }

    pub fn write(&mut self, address: u16, value: u8) -> bool {
        match address {
            0..=SOUND_RAM_END => self.ram[usize::from(address) & SOUND_RAM_MASK] = value,
            SOUND_BANK_START..=SOUND_BANK_END => {
                self.bank = (self.bank >> 1) | (u16::from(value & 1) << SOUND_BANK_INPUT_SHIFT);
            }
            SOUND_UNUSED_START..=SOUND_UNUSED_END => {}
            _ => return false,
        }
        true
    }
}

pub(crate) struct CandidateBus<'a> {
    pub memory: &'a mut SoundMemory,
    pub fault: Cell<Option<SoundFault>>,
    pub cartridge: &'a Cartridge,
    pub writes: WorkWrites,
    pub register_writes: RegisterWrites,
    pub sf001_writes: Sf001Writes,
    pub sf002_writes: Sf002Writes,
    pub psg_writes: PsgWrites,
    pub opn2_writes: Opn2Writes,
    pub timed_read: Cell<bool>,
    pub cartridge_read: Cell<Option<u16>>,
    pub opn2_status: u8,
    pub accesses: Cell<u32>,
    pub bank_access: Cell<Option<BankAccess>>,
    svp_reads: std::cell::RefCell<svp_reads::RomReads>,
    historical_rom: Option<&'a [svp_reads::RomRead]>,
    historical_cursor: Cell<usize>,
    #[cfg(feature = "first-failure-diagnostic")]
    pub diagnostic_access: Cell<Option<crate::diagnostic::SoundCandidateAccess>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BankAccess {
    pub address: u32,
    pub direction: Direction,
    pub value: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkWrites([Option<(usize, u8)>; SOUND_WORK_WRITE_CAPACITY]);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PsgWrites([Option<u8>; SOUND_PSG_WRITE_CAPACITY]);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Opn2Writes([Option<(u8, u8)>; SOUND_OPN2_WRITE_CAPACITY]);

impl Opn2Writes {
    fn push(&mut self, port: u8, value: u8) -> bool {
        let Some(slot) = self.0.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *slot = Some((port, value));
        true
    }

    pub fn publish(self, audio: &mut Audio, at: u64) {
        for (port, value) in self.0.into_iter().flatten() {
            audio.write_opn2(at, port, value);
        }
    }
}

impl PsgWrites {
    fn push(&mut self, value: u8) -> bool {
        let Some(slot) = self.0.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *slot = Some(value);
        true
    }

    pub fn publish(self, audio: &mut Audio, at: u64) {
        for value in self.0.into_iter().flatten() {
            audio.write_psg(at, value);
        }
    }
}

impl WorkWrites {
    fn push(&mut self, address: u32, value: u8) -> bool {
        let Some(slot) = self.0.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *slot = Some((address as usize & WORK_RAM_MASK, value));
        true
    }

    pub fn publish(self, ram: &mut [u8]) {
        for (address, value) in self.0.into_iter().flatten() {
            ram[address] = value;
        }
    }
}

impl<'a> CandidateBus<'a> {
    pub fn new(memory: &'a mut SoundMemory, cartridge: &'a Cartridge) -> Self {
        CandidateBus {
            memory,
            cartridge,
            fault: Cell::new(None),
            writes: WorkWrites::default(),
            register_writes: RegisterWrites::default(),
            sf001_writes: Sf001Writes::default(),
            sf002_writes: Sf002Writes::default(),
            psg_writes: PsgWrites::default(),
            opn2_writes: Opn2Writes::default(),
            timed_read: Cell::new(false),
            cartridge_read: Cell::new(None),
            opn2_status: 0,
            accesses: Cell::new(0),
            bank_access: Cell::new(None),
            svp_reads: std::cell::RefCell::new(svp_reads::RomReads::default()),
            historical_rom: None,
            historical_cursor: Cell::new(0),
            #[cfg(feature = "first-failure-diagnostic")]
            diagnostic_access: Cell::new(None),
        }
    }

    fn record(&self) {
        self.accesses.set(self.accesses.get().saturating_add(1));
    }

    #[cfg(feature = "first-failure-diagnostic")]
    fn diagnostic_access(&self, address: u16, direction: Direction, write_value: Option<u8>) {
        if self.fault.get().is_none() && crate::diagnostic::enabled() {
            self.diagnostic_access
                .set(Some(crate::diagnostic::SoundCandidateAccess {
                    ordinal: self.accesses.get(),
                    local_address: address,
                    direction,
                    write_value,
                    bank: self.memory.bank,
                    translated_main: (address >= SOUND_BANK_WINDOW)
                        .then(|| self.bank_address(address)),
                }));
        }
    }

    fn read_opn2(&self, port: u8) -> u8 {
        self.timed_read.set(true);
        if port == 0 { self.opn2_status } else { 0 }
    }

    fn bank_read(&self, address: u32, value: u8) -> u8 {
        self.capture_bank(address, Direction::Read, value);
        value
    }

    fn capture_bank(&self, address: u32, direction: Direction, value: u8) {
        if self.bank_access.get().is_none() {
            self.bank_access.set(Some(BankAccess {
                address,
                direction,
                value,
            }));
        }
    }

    fn bank_address(&self, address: u16) -> u32 {
        (u32::from(self.memory.bank) << SOUND_BANK_SHIFT) | (u32::from(address) & SOUND_MAIN_MASK)
    }

    fn bank_opn2_port(address: u32) -> Option<u8> {
        let port = (address & SOUND_MAIN_MASK) as u16;
        ((SOUND_APERTURE_START..=SOUND_APERTURE_END).contains(&address)
            && (SOUND_OPN2_START..=SOUND_BANK_OPN2_END).contains(&port))
        .then_some((port & 3) as u8)
    }
    fn fail(&self, fault: SoundFault) {
        if self.fault.get().is_none() {
            self.fault.set(Some(fault));
        }
    }
}

fn svp_access_allowed(cartridge: &Cartridge, address: u32, direction: Direction) -> bool {
    cartridge.svp.is_none()
        || match direction {
            Direction::Read => address < 0x200000 && (address as usize) < cartridge.bytes.len(),
            Direction::Write => {
                address > CARTRIDGE_UNUSED_END
                    && !(0xa13000..0xa13100).contains(&address)
                    && !(0xa15000..0xa15100).contains(&address)
            }
        }
}
