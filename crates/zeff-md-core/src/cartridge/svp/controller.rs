use super::ssp1601::{Fault, Ports};
use std::sync::Arc;

mod dram;
mod memory;
mod service;
mod state;
use dram::Dram;

pub(crate) struct DramReadView {
    dram: Dram,
}

impl DramReadView {
    pub(crate) fn read(&self, address: u16) -> u16 {
        self.dram.read(address)
    }
}

const DRAM_WORDS: usize = 65536;
pub(super) const IRAM_WORDS: usize = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Programming {
    #[default]
    Address,
    Mode(u16),
    Ready {
        address: u16,
        mode: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Channel {
    address: u32,
    mode: u16,
}

impl Channel {
    fn controls_valid(mode: u16) -> bool {
        mode & !0xfc00 == 0 && (mode & 0x4000 == 0 || mode & 0xb800 == 0)
    }

    fn new(address: u16, mode: u16, writing: bool) -> Result<Self, Fault> {
        if !Self::controls_valid(mode & !0x1f) {
            return Err(Controller::failure(6, "unqualified PMC mode"));
        }
        let region = mode & 0x1f;
        if mode & 0x4400 != 0 && region != 0x18 {
            return Err(Controller::failure(6, "DRAM-only PMC mode"));
        }
        if !matches!(region, 0x18 | 0x1c) && (writing || region > 0x0f) {
            return Err(Controller::failure(6, "unqualified PMC destination"));
        }
        Ok(Self {
            address: (u32::from(region) << 16) | u32::from(address),
            mode: mode & 0xfc00,
        })
    }

    fn next(self) -> Self {
        if self.mode & 0x4000 != 0 {
            return Self {
                address: (self.address + if self.address & 1 == 0 { 1 } else { 31 }) & 0x1fffff,
                ..self
            };
        }
        let code = (self.mode >> 11) & 7;
        let step = match code {
            0 => 0,
            7 => 128,
            _ => 1_i32 << (code - 1),
        };
        let delta = if self.mode & 0x8000 != 0 { -step } else { step };
        Self {
            address: self.address.wrapping_add_signed(delta) & 0x1fffff,
            ..self
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Controller {
    pub iram: Arc<Vec<u16>>,
    dram: Dram,
    rom: Arc<[u16]>,
    programming: Programming,
    channels: [[Option<Channel>; 6]; 2],
    pmc_shadow: Option<Channel>,
    xst: u16,
    status: u16,
    cold_blind: bool,
    pub inputs: u16,
}

impl Controller {
    #[cfg(test)]
    pub fn synthetic() -> Self {
        Self::with_rom(Arc::from([]))
    }

    pub fn with_rom(rom: Arc<[u16]>) -> Self {
        Self {
            iram: Arc::new(vec![0; IRAM_WORDS]),
            dram: Dram::zeroed(),
            rom,
            programming: Programming::Address,
            channels: [[None; 6]; 2],
            pmc_shadow: None,
            xst: 0,
            status: 0,
            cold_blind: true,
            inputs: 0,
        }
    }

    pub fn publish_main(&mut self, value: u16) {
        self.cold_blind = false;
        self.xst = value;
        self.status |= 2;
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn consume_main_status(&mut self) -> u16 {
        self.cold_blind = false;
        let status = self.status;
        self.status &= !1;
        status
    }

    pub fn xst(&self) -> u16 {
        self.xst
    }

    pub fn dram(&self, address: u16) -> u16 {
        self.dram.read(address)
    }

    pub(super) fn dram_read_view(&self) -> DramReadView {
        DramReadView {
            dram: self.dram.clone(),
        }
    }

    fn failure(index: u8, reason: &'static str) -> Fault {
        Fault::Port { index, reason }
    }

    fn program(&mut self, index: u8, writing: bool) -> Result<(), Fault> {
        let Programming::Ready { address, mode } = self.programming else {
            return Err(Self::failure(index, "incomplete PMC programming"));
        };
        let channel = Channel::new(address, mode, writing)?;
        if !writing && channel.mode & 0x4400 != 0 {
            return Err(Self::failure(index, "unqualified DRAM read mode"));
        }
        self.channels[usize::from(writing)][usize::from(index)] = Some(channel);
        self.pmc_shadow = Some(channel);
        self.programming = Programming::Address;
        self.cold_blind = false;
        Ok(())
    }

    fn pmc_write(&mut self, value: u16) -> Result<(), Fault> {
        self.programming = match self.programming {
            Programming::Address | Programming::Ready { .. } => {
                if let Some(shadow) = &mut self.pmc_shadow {
                    shadow.address = (shadow.address & 0x1f0000) | u32::from(value);
                }
                Programming::Mode(value)
            }
            Programming::Mode(address) => {
                self.pmc_shadow = Some(Channel::new(address, value, false)?);
                Programming::Ready {
                    address,
                    mode: value,
                }
            }
        };
        self.cold_blind = false;
        Ok(())
    }

    fn pmc_blind_read(&mut self) -> Result<(), Fault> {
        let shadow = self
            .pmc_shadow
            .ok_or_else(|| Self::failure(6, "unknown PMC contents"))?;
        self.programming = match self.programming {
            Programming::Address | Programming::Ready { .. } => {
                Programming::Mode(shadow.address as u16)
            }
            Programming::Mode(address) => {
                let mode = (shadow.address >> 16) as u16 | shadow.mode;
                Channel::new(address, mode, false)?;
                Programming::Ready { address, mode }
            }
        };
        self.cold_blind = false;
        Ok(())
    }

    fn preflight(&self, index: u8, blind: bool) -> Result<(), Fault> {
        if index <= 5
            && self.programming != Programming::Address
            && !(blind && matches!(self.programming, Programming::Ready { .. }))
        {
            return Err(Self::failure(
                index,
                "PMC requires completed directional blind capture",
            ));
        }
        Ok(())
    }

    fn memory_port(index: u8, st: u16, writing: bool) -> bool {
        index == 4 || (st & 0x0020 != 0 && index <= 5) || (!writing && index == 1)
    }
}

impl Controller {
    pub(super) fn read_captured(
        &mut self,
        index: u8,
        st: u16,
        blind: bool,
        captured: Option<u16>,
    ) -> Result<u16, Fault> {
        self.preflight(index, blind)?;
        if index > 7 || st & 0x0060 == 0x0040 {
            return Err(Self::failure(index, "unqualified external port mode"));
        }
        // Only the discarded, pristine cold-start EXT5 transaction is admitted.
        if index == 5 && st == 0 && blind && self.cold_blind {
            self.cold_blind = false;
            return Ok(0xffff);
        }
        if index == 7 && blind {
            self.programming = Programming::Address;
            self.cold_blind = false;
            return Ok(0xffff);
        }
        if index == 6 {
            let address = self
                .pmc_shadow
                .ok_or_else(|| Self::failure(6, "unknown PMC contents"))?
                .address as u16;
            let value = if !blind && matches!(self.programming, Programming::Mode(_)) {
                // Admit only the agreement domain of the retained readback models.
                if address >> 12 != (address >> 4) & 15 {
                    return Err(Self::failure(6, "unqualified PMC readback"));
                }
                ((address & 0x0f0f) << 4) | ((address & 0xf0f0) >> 4)
            } else {
                address
            };
            self.pmc_blind_read()?;
            return Ok(if blind { 0xffff } else { value });
        }
        if Self::memory_port(index, st, false) && index != 5 {
            if blind {
                self.program(index, false)?;
                return Ok(0xffff);
            }
            return self.read_memory(index, captured);
        }
        if blind {
            return Err(Self::failure(index, "unqualified blind port read"));
        }
        let value = match (index, st & 0x0020 != 0) {
            (0, false) => {
                let value = self.status;
                self.status &= !2;
                value
            }
            (3, false) => self.xst,
            _ => return Err(Self::failure(index, "unqualified external port read")),
        };
        self.cold_blind = false;
        Ok(value)
    }
}

impl Ports for Controller {
    fn read(&mut self, index: u8, st: u16, blind: bool) -> Result<u16, Fault> {
        self.read_captured(index, st, blind, None)
    }

    fn write(&mut self, index: u8, st: u16, blind: bool, value: u16) -> Result<(), Fault> {
        self.write_captured(index, st, blind, value, None)
            .map(|_| ())
    }

    fn status_inputs(&mut self) -> Result<u16, Fault> {
        Ok(self.inputs)
    }
}

impl Controller {
    pub(super) fn write_captured(
        &mut self,
        index: u8,
        st: u16,
        blind: bool,
        value: u16,
        captured: Option<u16>,
    ) -> Result<u16, Fault> {
        self.preflight(index, blind)?;
        if index > 7 || st & 0x0060 == 0x0040 {
            return Err(Self::failure(index, "unqualified external port mode"));
        }
        if Self::memory_port(index, st, true) {
            return if blind {
                self.program(index, true).map(|_| 0)
            } else {
                self.write_memory(index, value, captured)
            };
        }
        if blind {
            return Err(Self::failure(index, "unqualified blind port write"));
        }
        match index {
            0 if st & 0x0020 == 0 => self.status = value,
            3 if st & 0x0020 == 0 => {
                self.xst = value;
                self.status |= 1;
            }
            6 => return self.pmc_write(value).map(|_| 0),
            _ => return Err(Self::failure(index, "unqualified external port write")),
        }
        self.cold_blind = false;
        Ok(0)
    }
}
