mod external;
mod state;

use super::frontend::{ExternalFeedback, Output as FrontendOutput};
use crate::hardware::vdp::control_core::{
    HandshakeFeedback, MainRequests, PhaseLevels, RegisterStage,
};
use anyhow::{Result, ensure};
use zeff_m68k::bus::FunctionCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BoardPins {
    pub address: u32,
    pub data: u16,
    pub function_code: FunctionCode,
    pub address_strobe: bool,
    pub read: bool,
    pub upper_strobe: bool,
    pub lower_strobe: bool,
    pub interrupt_acknowledge: bool,
    pub sound_vdp_request: bool,
    pub test_mode: bool,
}

impl BoardPins {
    fn origin() -> Self {
        Self {
            address: 0,
            data: 0,
            function_code: FunctionCode::SupervisorProgram,
            address_strobe: false,
            read: true,
            upper_strobe: false,
            lower_strobe: false,
            interrupt_acknowledge: false,
            sound_vdp_request: false,
            test_mode: false,
        }
    }

    fn validate(self) -> Result<()> {
        ensure!(
            self.address <= 0xffffff && self.address & 1 == 0,
            "invalid resolved MAIN address"
        );
        ensure!(!self.sound_vdp_request, "SOUND VDP request is not owned");
        ensure!(!self.test_mode, "VDP test wiring is not owned");
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Inputs {
    pub phases: PhaseLevels,
    pub cpu_clock1: bool,
    pub refresh: bool,
    pub memory_beat: bool,
    pub fifo_full: bool,
    pub old_external: ExternalFeedback,
    pub handshake: HandshakeFeedback,
    pub register11_before: u8,
    pub reset_comb: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Early {
    #[cfg(test)]
    pub dma_pressure: Option<u32>,
    pub main: MainRequests,
    pub upper_strobe: bool,
    pub lower_strobe: bool,
    pub hv_read: bool,
    pub status_read: bool,
    pub external_address_cycle: bool,
    access: external::Access,
    pub address: u32,
    pub psg_acknowledge: bool,
    pub psg_write: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PinLevels {
    pub acknowledge: bool,
    pub output_enable: bool,
    pub column_strobe: bool,
    pub row_strobe: bool,
    pub upper_write: bool,
    pub lower_write: bool,
    pub ram_address: u8,
    pub address22: bool,
    pub data_read: bool,
    pub status_read: bool,
    pub hv_read: bool,
    pub psg_write: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Drives {
    pub address: Option<u32>,
    pub levels: PinLevels,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Wiring {
    external: external::External,
    pins: BoardPins,
    address: u32,
    held: PinLevels,
}

impl Wiring {
    pub fn software_origin() -> Self {
        Self {
            external: external::External::default(),
            pins: BoardPins::origin(),
            address: 0,
            held: PinLevels {
                output_enable: true,
                column_strobe: true,
                row_strobe: true,
                upper_write: true,
                lower_write: true,
                ..PinLevels::default()
            },
        }
    }

    // Capture once per board pass, not once per repeated asynchronous primitive.
    pub fn capture_bus(
        &mut self,
        pins: BoardPins,
        old_external: ExternalFeedback,
        source_word_address: u32,
    ) -> Result<()> {
        self.capture_bus_physical(pins, old_external, source_word_address, &mut ())
    }
    pub fn capture_bus_physical<C: crate::sound_trace::physical::vdp::Capture>(
        &mut self,
        pins: BoardPins,
        old_external: ExternalFeedback,
        source_word_address: u32,
        physical: &mut C,
    ) -> Result<()> {
        pins.validate()?;
        ensure!(
            source_word_address <= 0x3fffff,
            "invalid external DMA source"
        );
        self.address = if old_external.source_enabled {
            self.address & 0x400000 | source_word_address
        } else {
            (pins.address >> 1) & 0x73ffff
        };
        self.pins = pins;
        physical.vdp(crate::sound_trace::physical::vdp::Event::Capture {
            dma: old_external.source_enabled,
        });
        #[cfg(test)]
        physical.vdp_raw(crate::sound_trace::physical::vdp::raw::Event::Capture {
            dma: old_external.source_enabled,
            address: self.address,
            pins: crate::sound_trace::physical::vdp::raw::Pins {
                address: pins.address,
                word: pins.data,
                controls: [
                    pins.read,
                    pins.address_strobe,
                    pins.upper_strobe,
                    pins.lower_strobe,
                ],
            },
        });
        Ok(())
    }

    pub fn early(&mut self, input: Inputs) -> Result<Early> {
        ensure!(
            input.register11_before & 128 == 0,
            "VDP color address wiring is not owned"
        );
        let p = self.pins;
        let selected =
            self.address & 0x738070 == 0x600000 && p.address_strobe && input.handshake.request_gate;
        let port = self.address & 12;
        ensure!(!selected || port != 12, "VDP test port wiring is not owned");
        let lanes = p.upper_strobe || p.lower_strobe;
        let main = MainRequests {
            write: selected && !p.read && port == 0 && lanes,
            read: selected && p.read && port == 0 && lanes,
            control_select: self.address & 2 != 0,
            data_bus: p.data,
        };
        let access = self.external.early(input, p, self.address);
        let psg_acknowledge = selected && !p.read && port == 8;
        Ok(Early {
            #[cfg(test)]
            dma_pressure: access
                .full_pressure
                .then_some((self.address & 0x3fffff) << 1),
            main,
            upper_strobe: p.upper_strobe,
            lower_strobe: p.lower_strobe,
            hv_read: selected && p.read && port == 4,
            status_read: main.read && main.control_select,
            external_address_cycle: access.external_address_cycle,
            access,
            address: self.address,
            psg_acknowledge,
            psg_write: psg_acknowledge && p.lower_strobe,
        })
    }

    pub fn finish(&mut self, early: Early, frontend: &FrontendOutput) -> Result<Drives> {
        let registers = frontend.control.registers.at(RegisterStage::Control);
        ensure!(
            registers[11] & 128 == 0,
            "VDP color address wiring is not owned"
        );
        let (mux, column, row) = self.external.ram_pins(
            early.access,
            frontend.dma.external_byte,
            registers[11] & 64 != 0,
        );
        let ram_address = if mux {
            [14, 8, 9, 10, 11, 12, 13, 15].into_iter().enumerate().fold(
                0u8,
                |value, (destination, source)| {
                    value | (((early.address >> source) & 1) as u8) << destination
                },
            )
        } else {
            early.address as u8
        };
        self.held = PinLevels {
            acknowledge: early.access.acknowledge || early.psg_acknowledge || frontend.acknowledge,
            output_enable: early.access.first_recovery
                || early.access.read_enable
                || self.external.selected,
            column_strobe: column,
            row_strobe: row,
            upper_write: early.access.write_enable && early.upper_strobe,
            lower_write: early.access.write_enable && early.lower_strobe,
            ram_address,
            address22: self.external.address22(frontend.dma.external_word),
            data_read: early.main.read && !early.main.control_select,
            status_read: early.status_read,
            hv_read: early.hv_read,
            psg_write: early.psg_write,
        };
        let dma = frontend.control.registers.after().dma();
        self.drives(
            u32::from(dma.source_low()) | u32::from(dma.source_high()) << 16,
            ExternalFeedback {
                source_enabled: frontend.dma.external_word || frontend.dma.external_byte,
                bus_grant_acknowledge: frontend.dma.bus_grant_acknowledge,
            },
        )
    }

    pub fn drives(
        &self,
        source_word_address: u32,
        old_external: ExternalFeedback,
    ) -> Result<Drives> {
        ensure!(
            source_word_address <= 0x3fffff,
            "invalid external DMA source"
        );
        Ok(Drives {
            address: old_external
                .source_enabled
                .then_some((source_word_address | u32::from(self.held.address22) << 22) << 1),
            levels: self.held,
        })
    }
}
