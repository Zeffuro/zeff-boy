use crate::Region;
use crate::cartridge::Cartridge;
use crate::constants::*;
use crate::hardware::audio::Audio;
use crate::hardware::input::Input;
use crate::hardware::sound::SoundMemory;
use crate::hardware::vdp::{Vdp, VdpDmaSource, VdpPortError, VdpScheduleError};
use zeff_emu_common::time::{ClockRate, MasterTicks};
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, BusResponse, ByteLanes, Direction};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult, InterruptVector};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusError {
    UnsupportedAccess {
        address: u32,
        direction: Direction,
    },
    ClockOverflow,
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
    pub cartridge: Cartridge,
    pub ram: Box<[u8]>,
    pub now: MasterTicks,
    pub cpu_clock: CpuClocks,
    pub irq_level: u8,
    pub vdp: Vdp,
    pub reset_pulses: u64,
    pub reset_output: bool,
    pub clock_fault: bool,
    pub sound: SoundMemory,
    pub input: Input,
    pub prefetch_upper: Option<u8>,
    pub video: row_delivery::RowDelivery,
    pub audio: Audio,
}

mod audio;
mod dma;
mod input;
pub(crate) mod ownership;
mod row_delivery;
pub(crate) mod scheduled;
mod sound;
mod state;
mod vdp;

impl Bus {
    pub fn new(cartridge: Cartridge, region: Region) -> Self {
        Self {
            region,
            cartridge,
            ram: vec![0; WORK_RAM_SIZE].into_boxed_slice(),
            now: MasterTicks::ZERO,
            cpu_clock: CpuClocks(0),
            irq_level: 0,
            vdp: Vdp::bootstrap_mode5(),
            reset_pulses: 0,
            reset_output: false,
            clock_fault: false,
            sound: SoundMemory::default(),
            input: Input::default(),
            prefetch_upper: None,
            video: Default::default(),
            audio: Audio::new(region),
        }
    }

    pub fn rate(&self) -> ClockRate {
        ClockRate::from_hz(self.region.master_hz())
    }

    pub fn peek(&self, logical: u32) -> Option<u8> {
        let address = logical & BUS_ADDRESS_MASK;
        match address {
            0..=CARTRIDGE_END => self.cartridge.read(address),
            WORK_RAM_START..=BUS_ADDRESS_MASK => Some(self.ram[address as usize & WORK_RAM_MASK]),
            SOUND_APERTURE_START..=SOUND_APERTURE_END => {
                let sound_address = (address & SOUND_MAIN_MASK) as u16;
                if (SOUND_OPN2_START..=SOUND_OPN2_END).contains(&sound_address) {
                    Some(if sound_address & 3 == 0 {
                        self.audio.opn2_status_at(self.now.get())
                    } else {
                        0
                    })
                } else {
                    self.sound.read(sound_address)
                }
            }
            _ => self.read_input(address),
        }
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

    fn advance_video_to(&mut self, next: MasterTicks) -> Result<(), BusError> {
        let row_due = next_row_deadline(self.vdp.now()).is_some_and(|at| at <= next);
        let capture_due = self.video.deadline().is_some_and(|at| at <= next);
        if row_due || capture_due {
            self.vdp
                .preflight_with_source(
                    next,
                    self.region,
                    VdpDmaSource::from_cartridge(&self.cartridge, &self.ram),
                )
                .map_err(BusError::Vdp)?;
            let period = self.region.frame_lines() * VDP_LINE_TICKS;
            let skip = if self.vdp.progressive_skip_allowed() {
                let start = self
                    .vdp
                    .field_position(self.region)
                    .map_err(BusError::Vdp)?
                    .start
                    .get();
                MasterTicks::new(
                    start
                        + ((next.get().saturating_sub(start) / period).saturating_sub(1)) * period,
                )
            } else {
                self.vdp.now()
            };
            if let Some(at) = self.video.deadline().filter(|&at| at < skip) {
                let captured = self
                    .vdp
                    .advance_with_line_from(
                        at,
                        at,
                        false,
                        self.region,
                        Some(VdpDmaSource::from_cartridge(&self.cartridge, &self.ram)),
                    )
                    .expect("validated video prefix");
                self.video.complete(captured);
            }
            if skip > self.vdp.now() {
                self.vdp
                    .advance_with_source(
                        skip,
                        self.region,
                        VdpDmaSource::from_cartridge(&self.cartridge, &self.ram),
                    )
                    .expect("validated skipped fields");
            }
            loop {
                let row = next_row_deadline(self.vdp.now());
                let capture = self.video.deadline();
                let deadline = match (row, capture) {
                    (Some(row), Some(capture)) => row.min(capture),
                    (Some(at), None) | (None, Some(at)) => at,
                    (None, None) => break,
                };
                if deadline >= next {
                    break;
                }
                if capture == Some(deadline) {
                    let captured = self
                        .vdp
                        .advance_with_line_from(
                            deadline,
                            deadline,
                            false,
                            self.region,
                            Some(VdpDmaSource::from_cartridge(&self.cartridge, &self.ram)),
                        )
                        .expect("validated video deadline");
                    self.video.complete(captured);
                } else {
                    self.vdp
                        .advance_with_source(
                            deadline,
                            self.region,
                            VdpDmaSource::from_cartridge(&self.cartridge, &self.ram),
                        )
                        .expect("validated video deadline");
                }
                if row == Some(deadline) {
                    self.publish_raster_row(deadline)?;
                }
            }
        }
        if self.video.deadline() == Some(next) {
            let captured = self
                .vdp
                .advance_with_line_from(
                    next,
                    next,
                    false,
                    self.region,
                    Some(VdpDmaSource::from_cartridge(&self.cartridge, &self.ram)),
                )
                .map_err(BusError::Vdp)?;
            self.video.complete(captured);
        } else {
            self.vdp
                .advance_with_source(
                    next,
                    self.region,
                    VdpDmaSource::from_cartridge(&self.cartridge, &self.ram),
                )
                .map_err(BusError::Vdp)?;
        }
        if next.get() != 0 && next.get().is_multiple_of(VDP_LINE_TICKS) {
            self.publish_raster_row(next)?;
        }
        Ok(())
    }

    fn publish_raster_row(&mut self, at: MasterTicks) -> Result<(), BusError> {
        let Some((row, field)) = self
            .vdp
            .completed_row(at, self.region)
            .map_err(BusError::Vdp)?
        else {
            return Ok(());
        };
        let row = usize::from(row);
        let height = self.vdp.registers().display_height(self.region);
        if row < height {
            self.video.publish_field_row(
                row,
                row_delivery::RasterField {
                    id: field.id,
                    height,
                    interlaced: field.active_interlaced,
                    odd: field.active_odd,
                },
                self.vdp.prepare_raster_line(row as u16, self.region),
            );
        }
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

    fn read(&mut self, access: BusAccess) -> Result<u16, BusError> {
        let result = self.read_mapped(access);
        self.prefetch_upper = match (access.kind, access.lanes, &result) {
            (AccessKind::Prefetch, ByteLanes::Both, Ok(value))
                if (access.bus_address <= CARTRIDGE_END
                    && (access.bus_address as usize + 1) < self.cartridge.bytes.len())
                    || (WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&access.bus_address) =>
            {
                Some((value >> 8) as u8)
            }
            _ => None,
        };
        result
    }

    fn read_mapped(&mut self, access: BusAccess) -> Result<u16, BusError> {
        if let Some(result) = self.read_vdp(access) {
            return result;
        }
        if let Some(value) = self.read_sound(access) {
            return Ok(value);
        }
        let address = access.bus_address;
        let failure = || BusError::UnsupportedAccess {
            address,
            direction: Direction::Read,
        };
        if (SOUND_APERTURE_START..=SOUND_APERTURE_END).contains(&address) {
            return Err(failure());
        }
        Ok(u16::from_be_bytes([
            self.peek(address).ok_or_else(failure)?,
            self.peek(address.wrapping_add(1)).ok_or_else(failure)?,
        ]))
    }

    fn write(&mut self, access: BusAccess, value: u16) -> Result<(), BusError> {
        self.prefetch_upper = None;
        if access.bus_address == crate::cartridge::ram::SAVE_CONTROL
            && let Some(save) = &mut self.cartridge.save
        {
            if access.lanes == ByteLanes::Lower && value as u8 == 1 {
                save.enabled = true;
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
        if let Some(result) = self.write_vdp(access, value) {
            return result;
        }
        if self.write_input(access, value) {
            return Ok(());
        }
        if self.write_sound(access, value) {
            return Ok(());
        }
        match access.bus_address {
            0..=CARTRIDGE_END => {
                if let Some(save) = &mut self.cartridge.save
                    && save.contains(access.bus_address)
                {
                    if !save.enabled {
                        return Err(BusError::UnsupportedAccess {
                            address: access.bus_address,
                            direction: Direction::Write,
                        });
                    }
                    if access.lanes != ByteLanes::Lower {
                        save.write(access.bus_address, (value >> 8) as u8);
                    }
                    if access.lanes != ByteLanes::Upper {
                        save.write(access.bus_address + 1, value as u8);
                    }
                }
                Ok(())
            }
            WORK_RAM_START..=BUS_ADDRESS_MASK => {
                let index = access.bus_address as usize & WORK_RAM_MASK;
                let [high, low] = value.to_be_bytes();
                if access.lanes != ByteLanes::Lower {
                    self.ram[index] = high;
                }
                if access.lanes != ByteLanes::Upper {
                    self.ram[(index + 1) & WORK_RAM_MASK] = low;
                }
                Ok(())
            }
            address => Err(BusError::UnsupportedAccess {
                address,
                direction: Direction::Write,
            }),
        }
    }
}

impl ClockedBus for Bus {
    type Error = BusError;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        let mut waits = 0u32;
        let result = (|| {
            self.check_main_clock(start)?;
            self.wait_external_dma(&mut waits)?;
            self.advance_main(self.cpu_clock, TRANSFER_CLOCKS)?;
            while let Some(clocks) = self.vdp_read_wait(access)? {
                let total = waits.checked_add(clocks).ok_or(BusError::ClockOverflow)?;
                self.advance_main(self.cpu_clock, clocks)?;
                waits = total;
            }
            self.read(access)
        })();
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        let mut waits = 0u32;
        let result = (|| {
            self.check_main_clock(start)?;
            self.wait_external_dma(&mut waits)?;
            self.advance_main(self.cpu_clock, TRANSFER_CLOCKS)?;
            self.prefetch_upper = None;
            while let Some(clocks) = self.vdp_data_wait(access)? {
                let total = waits.checked_add(clocks).ok_or(BusError::ClockOverflow)?;
                self.advance_main(self.cpu_clock, clocks)?;
                waits = total;
            }
            self.write(access, value)?;
            self.wait_external_dma(&mut waits)
        })();
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        if self.advance_main(start, clocks).is_err() {
            self.clock_fault = true;
        } else if clocks != 0 {
            self.prefetch_upper = None;
        }
    }
    fn acknowledge_cycle(
        &mut self,
        _: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, BusError> {
        let mut waits = 0;
        let result = (|| {
            self.check_main_clock(start)?;
            self.wait_external_dma(&mut waits)?;
            self.advance_main_cycle(self.cpu_clock, TRANSFER_CLOCKS, true)?;
            self.audio.advance_to(self.now.get());
            self.prefetch_upper = None;
            Ok(BusResponse::Complete(InterruptVector::Autovector))
        })();
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }
    fn begin_reset_output(&mut self, _: CpuClocks) -> Result<BusResponse<()>, BusError> {
        self.prefetch_upper = None;
        self.reset_output = true;
        Ok(BusResponse::Complete(()))
    }
    fn end_reset_output(&mut self, _: CpuClocks) {
        self.reset_output = false;
        self.reset_pulses = self.reset_pulses.wrapping_add(1);
    }
}

fn next_row_deadline(after: MasterTicks) -> Option<MasterTicks> {
    after
        .get()
        .checked_add(VDP_LINE_TICKS - after.get() % VDP_LINE_TICKS)
        .map(MasterTicks::new)
}
