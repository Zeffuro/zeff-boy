mod controller;
mod input;
mod responder;
mod state;

use super::frontend::MemoryDrives;
use crate::hardware::vdp::control_core::PhaseLevels;
use anyhow::{Result, ensure};
use controller::Controller;
pub(in crate::hardware::vdp) use input::{
    BusInputs, ByteRead, ByteWrite, ControllerOutput, Inputs, Pins, Profile, Response, RowTransfer,
};
use responder::Responder;
pub(in crate::hardware::vdp) use responder::canonical_address;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Ram {
    controller: Controller,
    responder: Responder,
}

impl Ram {
    pub fn software_origin() -> Self {
        Self {
            controller: Controller::software_origin(),
            responder: Responder::software_origin(),
        }
    }

    pub fn pins(&self) -> Pins {
        self.controller.pins
    }

    pub fn clock(&mut self, input: Inputs) -> Result<ControllerOutput> {
        input.profile.validate()?;
        ensure!(
            input.frontend.address <= 0x1ffff && !(input.frontend.fill && input.frontend.copy),
            "invalid VDP VRAM controller input"
        );
        let mut candidate = self.controller;
        let returned_data = candidate.evaluate(input);
        let output = ControllerOutput {
            pins: candidate.pins,
            returned_data,
        };
        self.controller = candidate;
        Ok(output)
    }

    pub fn respond(
        &mut self,
        bus: BusInputs,
        read_byte: impl FnMut(u16) -> Result<u8>,
    ) -> Result<Response> {
        let mut candidate = self.responder;
        let response = candidate.evaluate(self.controller.pins, bus, read_byte)?;
        self.responder = candidate;
        Ok(response)
    }

    pub fn validate_frontier(&self) -> Result<()> {
        ensure!(
            self.controller.pins == self.responder.previous,
            "incomplete VRAM board-pass response"
        );
        Ok(())
    }
}
