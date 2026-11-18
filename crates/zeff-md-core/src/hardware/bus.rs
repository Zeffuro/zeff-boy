mod rollback_memory;
use crate::Region;
use crate::cartridge::{Cartridge, SampledByte};
use crate::constants::*;
use crate::hardware::audio::Audio;
use crate::hardware::input::Input;
use crate::hardware::sound::SoundMemory;
use crate::hardware::vdp::{Vdp, VdpDmaSource, VdpPortError, VdpScheduleError, ZeroFillWrite};
use zeff_emu_common::time::{ClockRate, MasterTicks};
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, BusResponse, ByteLanes, Direction};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, InstructionInput, InterruptVector};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusError {
    UnsupportedAccess {
        address: u32,
        direction: Direction,
    },
    Svp {
        address: u32,
        direction: Direction,
        cause: crate::svp::Error,
    },
    UnqualifiedCartridgeReset {
        address: u32,
    },
    ClockOverflow,
    Owned(String),
    Vdp(VdpScheduleError),
    VdpPort(VdpPortError),
    ClockMismatch {
        expected: CpuClocks,
        actual: CpuClocks,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bus {
    pub region: Region,
    pub tmss: Option<crate::hardware::tmss::Tmss>,
    pub cartridge: Cartridge,
    pub ram: Box<[u8]>,
    pub now: MasterTicks,
    pub cpu_clock: CpuClocks,
    pub irq_level: u8,
    pub vdp: Vdp,
    pub reset_pulses: u64,
    pub reset_output: bool,
    pub(crate) rmw: Option<rmw::RamRmw>,
    pub clock_fault: bool,
    pub sound: SoundMemory,
    pub input: Input,
    pub prefetch: Prefetch,
    pub video: row_delivery::RowDelivery,
    pub audio: Audio,
    pub(crate) owned: Option<Box<owned::OwnedState>>,
}

mod audio;
mod bounded_odd;
mod bounded_switched;
mod clocked;
mod dma;
mod download;
mod extended_ssf;
mod input;
mod input_timed;
mod interrupt_sampling;
pub(crate) mod owned;
pub(crate) mod ownership;
mod parallel_adapter;
mod prefetch;
mod previous;
mod realtec;
mod registers;
mod rmw;
mod save_latch;
mod sf001;
mod sf002;
mod sf004;
pub(crate) use prefetch::Prefetch;
mod instruction_input;
mod raster;
mod read;
mod row_delivery;
pub(crate) mod scheduled;
mod sound;
mod state;
mod svp;
mod tmss;
mod unused_cartridge;
mod vdp;
use vdp::{VdpTransfer, VdpTransferRequest};

impl Bus {
    pub fn new(cartridge: Cartridge, region: Region) -> Self {
        Self {
            region,
            tmss: None,
            cartridge,
            ram: vec![0; WORK_RAM_SIZE].into_boxed_slice(),
            now: MasterTicks::ZERO,
            cpu_clock: CpuClocks(0),
            irq_level: 0,
            vdp: Vdp::bootstrap_mode5(),
            reset_pulses: 0,
            reset_output: false,
            rmw: None,
            clock_fault: false,
            sound: SoundMemory::default(),
            input: Input::cold(),
            prefetch: Prefetch::Empty,
            video: Default::default(),
            audio: Audio::new(region),
            owned: None,
        }
    }

    pub fn rate(&self) -> ClockRate {
        ClockRate::from_hz(self.region.master_hz())
    }

    pub fn advance_machine(&mut self, ticks: u64) -> Result<(), BusError> {
        self.advance_machine_cycle(ticks, false)?;
        self.audio.advance_to(self.now.get());
        Ok(())
    }

    pub fn interrupt_level(&self) -> u8 {
        if self.irq_level != 0 {
            self.irq_level
        } else {
            self.vdp.interrupt_level()
        }
    }

    fn advance_machine_cycle(&mut self, ticks: u64, acknowledge: bool) -> Result<(), BusError> {
        if self
            .cartridge
            .svp
            .as_ref()
            .is_some_and(|model| model.functional_timing().is_some())
        {
            return self.advance_functional_svp(ticks, acknowledge);
        }
        let next = MasterTicks::new(
            self.now
                .get()
                .checked_add(ticks)
                .ok_or(BusError::ClockOverflow)?,
        );
        self.advance_video_to(next)?;
        if acknowledge && self.irq_level == 0 {
            self.vdp.acknowledge_cpu();
        }
        self.now = next;
        Ok(())
    }

    fn advance_main(&mut self, start: CpuClocks, clocks: u32) -> Result<(), BusError> {
        self.advance_main_cycle(start, clocks, false)?;
        self.audio.advance_to(self.now.get());
        Ok(())
    }

    fn advance_main_cycle(
        &mut self,
        start: CpuClocks,
        clocks: u32,
        acknowledge: bool,
    ) -> Result<(), BusError> {
        self.check_main_clock(start)?;
        let next = self
            .cpu_clock
            .0
            .checked_add(u64::from(clocks))
            .ok_or(BusError::ClockOverflow)?;
        self.advance_machine_cycle(u64::from(clocks) * MAIN_CPU_DIVISOR, acknowledge)?;
        self.cpu_clock = CpuClocks(next);
        Ok(())
    }

    fn check_main_clock(&self, start: CpuClocks) -> Result<(), BusError> {
        if self.clock_fault {
            return Err(BusError::ClockOverflow);
        }
        if start != self.cpu_clock {
            return Err(BusError::ClockMismatch {
                expected: self.cpu_clock,
                actual: start,
            });
        }
        Ok(())
    }

    fn write(&mut self, access: BusAccess, value: u16) -> Result<(), BusError> {
        self.write_owned(access, value, None, false)
    }

    fn write_owned(
        &mut self,
        access: BusAccess,
        value: u16,
        input_begin: Option<u64>,
        main_data: bool,
    ) -> Result<(), BusError> {
        self.write_owned_capturing(access, value, input_begin, main_data, &mut ())
    }

    fn write_owned_capturing<S: previous::Capture>(
        &mut self,
        access: BusAccess,
        value: u16,
        input_begin: Option<u64>,
        main_data: bool,
        previous: &mut S,
    ) -> Result<(), BusError> {
        self.preflight_svp_write(access, value)?;
        self.preflight_tmss(access, Some(value))?;
        if let Some(result) = self.write_svp(access, value) {
            if result.is_ok() {
                self.prefetch = Prefetch::Empty;
            }
            return result;
        }
        if let Some(result) = self.write_tmss(access, value) {
            if result.is_ok() {
                self.prefetch = Prefetch::Empty;
            }
            return result;
        }
        self.preflight_bounded_odd(access, Direction::Write)?;
        self.preflight_bounded_switched(access, Some(value))?;
        self.preflight_save_latch(access, Direction::Write)?;
        self.preflight_download(access, Direction::Write)?;
        #[cfg(feature = "first-failure-diagnostic")]
        crate::diagnostic::service_entry(self, access, crate::diagnostic::Callback::Write);
        self.preflight_banked_register_write(access)?;
        self.preflight_protection_write(access)?;
        self.preflight_parallel_adapter_write(access, value)?;
        self.preflight_sf002_write(access, value)?;
        self.preflight_sf004_write(access, value)?;
        self.preflight_extended_ssf_write(access)?;
        self.preflight_realtec_write(access)?;
        self.prefetch = Prefetch::Empty;
        if let Some(accepted) = self
            .cartridge
            .parallel_adapter
            .as_mut()
            .and_then(|board| board.write(access, value))
        {
            return if accepted {
                Ok(())
            } else {
                Err(BusError::UnsupportedAccess {
                    address: access.bus_address,
                    direction: Direction::Write,
                })
            };
        }
        if let Some(result) = self.write_realtec(access, value) {
            return result;
        }
        if let Some(result) = self.write_extended_ssf(access, value) {
            return result;
        }
        if let Some(board) = &mut self.cartridge.sf004
            && crate::cartridge::sf004::Sf004::domain(access.bus_address)
        {
            assert!(board.write(access.bus_address + 1, value as u8));
            return Ok(());
        }
        if self.cartridge.bounded_split_hole(access.bus_address)
            || self.cartridge.linear_rom.is_some() && access.bus_address <= CARTRIDGE_UNUSED_END
        {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Write,
            });
        }
        if let Some(accepted) =
            self.cartridge
                .write_sf002(access, value, self.now.get(), self.region.master_hz())
        {
            return if accepted {
                Ok(())
            } else {
                Err(BusError::UnsupportedAccess {
                    address: access.bus_address,
                    direction: Direction::Write,
                })
            };
        }
        if (0xa130f0..=0xa130fe).contains(&access.bus_address)
            && let Some(board) = &mut self.cartridge.ssf2
        {
            return if board.control(access, value) {
                Ok(())
            } else {
                Err(BusError::UnsupportedAccess {
                    address: access.bus_address,
                    direction: Direction::Write,
                })
            };
        }
        if self.write_sf001(access, value) {
            return Ok(());
        }
        if let Some(result) = self.write_latched_save(access, value) {
            return result;
        }
        if let Some(result) = self.write_download(access, value) {
            return result;
        }
        if access.bus_address == MEMORY_MODE_CONTROL
            && access.lanes != ByteLanes::Lower
            && value & MEMORY_MODE_DRAM == 0
        {
            return Ok(());
        }
        if (crate::cartridge::ram::SAVE_CONTROL..=crate::cartridge::ram::SAVE_CONTROL_END)
            .contains(&access.bus_address)
            && let Some(save) = &mut self.cartridge.save
            && save.accepts_control_address(access.bus_address)
        {
            if save.bounded_split() {
                return Ok(());
            }
            let accepted = if access.lanes == ByteLanes::Upper {
                access.logical_address == crate::cartridge::ram::SAVE_CONTROL
                    && save.unchanged_upper_control((value >> 8) as u8)
            } else {
                save.control(value as u8, access.lanes == ByteLanes::Both)
            };
            if accepted {
                return Ok(());
            }
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Write,
            });
        }
        if self.write_psg(access, value) {
            return Ok(());
        }
        if let Some(result) = self.write_vdp(access, value, main_data) {
            return result;
        }
        let input_accepted = if let Some(begin) = input_begin {
            self.write_input_cycle(access, value, begin)
        } else {
            self.write_input(access, value)
        };
        if input_accepted {
            return Ok(());
        }
        if self.write_sound(access, value) {
            return Ok(());
        }
        if let Some(result) = self.write_lock_on_save(access, value) {
            return result;
        }
        if self.write_registers(access, value) {
            return Ok(());
        }
        if self.cartridge.expansion_save_contains(access.bus_address) {
            let save = self.cartridge.save.as_mut().unwrap();
            if access.lanes != ByteLanes::Lower {
                save.write(
                    access.bus_address,
                    (value >> 8) as u8,
                    self.now.get(),
                    self.region.master_hz(),
                );
            }
            if access.lanes != ByteLanes::Upper {
                save.write(
                    access.bus_address + 1,
                    value as u8,
                    self.now.get(),
                    self.region.master_hz(),
                );
            }
            return Ok(());
        }
        match access.bus_address {
            0..=CARTRIDGE_END => {
                if let Some(save) = &mut self.cartridge.save
                    && save.contains(access.bus_address)
                {
                    if save.serial_write(
                        value,
                        access.lanes != ByteLanes::Lower,
                        access.lanes != ByteLanes::Upper,
                        self.now.get(),
                        self.region.master_hz(),
                    ) {
                        return Ok(());
                    }
                    if !save.enabled {
                        if save.rom_visible() {
                            return Ok(());
                        }
                        return Err(BusError::UnsupportedAccess {
                            address: access.bus_address,
                            direction: Direction::Write,
                        });
                    }
                    if access.lanes != ByteLanes::Lower {
                        save.write(
                            access.bus_address,
                            (value >> 8) as u8,
                            self.now.get(),
                            self.region.master_hz(),
                        );
                    }
                    if access.lanes != ByteLanes::Upper {
                        save.write(
                            access.bus_address + 1,
                            value as u8,
                            self.now.get(),
                            self.region.master_hz(),
                        );
                    }
                }
                Ok(())
            }
            WORK_RAM_START..=BUS_ADDRESS_MASK => {
                if self.suppress_rmw_write(access) {
                    return Ok(());
                }
                let index = access.bus_address as usize & WORK_RAM_MASK;
                let [high, low] = value.to_be_bytes();
                previous.capture(&self.ram, index, access.lanes);
                if access.lanes != ByteLanes::Lower {
                    self.ram[index] = high;
                }
                if access.lanes != ByteLanes::Upper {
                    self.ram[(index + 1) & WORK_RAM_MASK] = low;
                }
                Ok(())
            }
            address if address > CARTRIDGE_END && address <= CARTRIDGE_UNUSED_END => Ok(()),
            address => Err(BusError::UnsupportedAccess {
                address,
                direction: Direction::Write,
            }),
        }
    }
}
