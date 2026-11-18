use super::ssp1601::Fault;
use super::{InstructionModel, ServiceRequest, ServiceResource, ServiceRole};
use crate::cartridge::rom_overlay::RomOverlay;
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[derive(Clone, Copy)]
pub(crate) struct CurrentRom<'a> {
    bytes: &'a [u8],
    overlay: &'a RomOverlay,
}

impl<'a> CurrentRom<'a> {
    pub(crate) fn new(bytes: &'a [u8], overlay: &'a RomOverlay) -> Self {
        Self { bytes, overlay }
    }

    fn word(self, address: u32) -> Option<u16> {
        let offset = usize::try_from(address).ok()?.checked_mul(2)?;
        Some(u16::from_be_bytes([
            self.overlay.read(offset, *self.bytes.get(offset)?),
            self.overlay.read(offset + 1, *self.bytes.get(offset + 1)?),
        ]))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WordInput {
    pub ordinal: u8,
    pub address: u32,
    pub original: u16,
    pub effective: u16,
}

impl InstructionModel {
    pub(crate) fn shared_rom_history_owner(&self) -> bool {
        self.services.is_some() && self.clock.is_some() && self.boundary_control.is_some()
    }

    pub(crate) fn has_rom_history(&self) -> bool {
        !self.rom_history.is_empty()
    }

    pub(super) fn historical_word(&self, request: ServiceRequest) -> Result<u16, Fault> {
        let original = self
            .rom
            .get(request.address as usize)
            .copied()
            .ok_or(Fault::InvalidState("historical ROM bounds"))?;
        if let Some(input) = self
            .rom_history
            .iter()
            .find(|input| input.ordinal == request.ordinal)
        {
            if !eligible(request)
                || input.address != request.address
                || input.original != original
                || input.effective == original
            {
                return Err(Fault::InvalidState("historical ROM owner"));
            }
            Ok(input.effective)
        } else {
            Ok(original)
        }
    }

    pub(super) fn current_word(
        &mut self,
        request: ServiceRequest,
        rom: Option<CurrentRom<'_>>,
    ) -> Result<u16, Fault> {
        let original = self
            .rom
            .get(request.address as usize)
            .copied()
            .ok_or(Fault::InvalidState("current ROM bounds"))?;
        let effective = match rom {
            Some(rom) => rom
                .word(request.address)
                .ok_or(Fault::InvalidState("current ROM bounds"))?,
            None => original,
        };
        if effective != original {
            if !eligible(request) || self.rom_history.len() >= super::service::MAX_CAPTURES {
                return Err(Fault::InvalidState("current ROM owner/bound"));
            }
            self.rom_history.push(WordInput {
                ordinal: request.ordinal,
                address: request.address,
                original,
                effective,
            });
        }
        Ok(effective)
    }

    pub(super) fn write_rom_history(&self, w: &mut StateWriter) {
        w.write_bytes(b"ZSVPROM2");
        w.write_u8(self.rom_history.len() as u8);
        for input in &self.rom_history {
            w.write_u8(input.ordinal);
            w.write_u32(input.address);
            for (original, effective) in input
                .original
                .to_be_bytes()
                .into_iter()
                .zip(input.effective.to_be_bytes())
            {
                w.write_u8(original);
                w.write_bool(original != effective);
                if original != effective {
                    w.write_u8(effective);
                }
            }
        }
    }

    pub(super) fn read_rom_history(r: &mut StateReader<'_>) -> anyhow::Result<Vec<WordInput>> {
        let mut tag = [0; 8];
        r.read_exact(&mut tag)?;
        anyhow::ensure!(&tag == b"ZSVPROM2", "historical ROM schema");
        let count = usize::from(r.read_u8()?);
        anyhow::ensure!(
            (1..=super::service::MAX_CAPTURES).contains(&count),
            "historical ROM count"
        );
        let mut inputs = Vec::with_capacity(count);
        for _ in 0..count {
            let ordinal = r.read_u8()?;
            let address = r.read_u32()?;
            let mut original = [0; 2];
            let mut effective = [0; 2];
            for lane in 0..2 {
                original[lane] = r.read_u8()?;
                effective[lane] = if r.read_bool()? {
                    let value = r.read_u8()?;
                    anyhow::ensure!(value != original[lane], "redundant ROM lane");
                    value
                } else {
                    original[lane]
                };
            }
            anyhow::ensure!(
                original != effective
                    && inputs
                        .last()
                        .is_none_or(|input: &WordInput| input.ordinal < ordinal),
                "historical ROM order/canonicality"
            );
            inputs.push(WordInput {
                ordinal,
                address,
                original: u16::from_be_bytes(original),
                effective: u16::from_be_bytes(effective),
            });
        }
        Ok(inputs)
    }

    pub(super) fn validate_rom_history(&self) -> anyhow::Result<()> {
        let captures = self
            .services
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("history without services"))?;
        for input in &self.rom_history {
            let capture = captures
                .get(usize::from(input.ordinal))
                .ok_or_else(|| anyhow::anyhow!("unused ROM history"))?;
            anyhow::ensure!(
                eligible(capture.request)
                    && capture.request.ordinal == input.ordinal
                    && self.historical_word(capture.request)? == capture.response,
                "historical ROM capture identity"
            );
        }
        Ok(())
    }
}

fn eligible(request: ServiceRequest) -> bool {
    request.resource == ServiceResource::SharedRom
        && matches!(
            request.role,
            ServiceRole::Opcode
                | ServiceRole::InstructionWord
                | ServiceRole::ProgramOperand
                | ServiceRole::PortRead(_)
        )
}
