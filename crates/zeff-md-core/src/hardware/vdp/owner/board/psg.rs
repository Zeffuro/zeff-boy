use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PsgBinding {
    pub address: u16,
    pub bank: u16,
    pub half: u64,
    pub pass: u8,
}

impl PsgBinding {
    fn main_address(self) -> Result<u32> {
        ensure!(
            self.bank < 512 && self.pass < 2,
            "invalid PSG source binding"
        );
        ensure!(
            self.address >= 0x8000 || self.address & 0xff00 == 0x7f00,
            "PSG source is not a shared SOUND cycle"
        );
        let address = if self.address & 0xff00 == 0x7f00 {
            0xc00000 | u32::from(self.address & 255)
        } else {
            u32::from(self.bank) << 15 | u32::from(self.address & 0x7fff)
        };
        ensure!(
            address & 0xe700e0 == 0xc00000 && (address >> 1) & 12 == 8 && address & 1 != 0,
            "PSG source does not select the lower byte"
        );
        Ok(address)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Capture {
    binding: PsgBinding,
    value: u8,
    known: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PsgWrite {
    pub binding: PsgBinding,
    pub sequence: u64,
    pub value: u8,
    pub half: u64,
    pub pass: u8,
}

impl PsgWrite {
    pub fn validate(self) -> Result<()> {
        self.binding.main_address()?;
        ensure!(
            self.sequence != 0
                && self.pass < 2
                && (self.binding.half, self.binding.pass) < (self.half, self.pass),
            "invalid PSG write receipt"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Psg {
    level: bool,
    capture: Option<Capture>,
    sequence: u64,
    pending: Option<PsgWrite>,
}

impl Psg {
    pub const fn origin() -> Self {
        Self {
            level: false,
            capture: None,
            sequence: 0,
            pending: None,
        }
    }

    pub fn write_state(self, writer: &mut StateWriter) {
        writer.write_bool(self.level);
        writer.write_u64(self.sequence);
        writer.write_bool(self.capture.is_some());
        if let Some(capture) = self.capture {
            writer.write_u16(capture.binding.address);
            writer.write_u16(capture.binding.bank);
            writer.write_u64(capture.binding.half);
            writer.write_u8(capture.binding.pass);
            writer.write_u8(capture.value);
            writer.write_u8(capture.known);
        }
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let level = reader.read_bool()?;
        let sequence = reader.read_u64()?;
        let capture = if reader.read_bool()? {
            Some(Capture {
                binding: PsgBinding {
                    address: reader.read_u16()?,
                    bank: reader.read_u16()?,
                    half: reader.read_u64()?,
                    pass: reader.read_u8()?,
                },
                value: reader.read_u8()?,
                known: reader.read_u8()?,
            })
        } else {
            None
        };
        Ok(Self {
            level,
            capture,
            sequence,
            pending: None,
        })
    }
}

impl Board {
    pub fn psg_level(&self) -> bool {
        self.psg.level
    }

    pub fn take_psg_write(&mut self) -> Result<Option<PsgWrite>> {
        ensure!(
            self.stage.is_none(),
            "PSG receipt consumed inside board resolve"
        );
        Ok(self.psg.pending.take())
    }

    pub(super) fn observe_psg(&mut self, stage: Stage, view: BoardView) -> Result<()> {
        ensure!(self.psg.pending.is_none(), "unretired SOUND PSG write");
        let level = view.drives.levels.psg_write;
        if level && !self.psg.level {
            let sound_owner = self.grant.tristate() && !stage.pins.address_strobe;
            if sound_owner {
                let source = stage.companion;
                ensure!(
                    source.pins.shared() && source.pins.write,
                    "PSG pulse lacks a SOUND write source"
                );
                let binding = PsgBinding {
                    address: source
                        .pins
                        .address
                        .ok_or_else(|| anyhow::anyhow!("PSG source is undriven"))?,
                    bank: source.bank,
                    half: view.half,
                    pass: view.pass,
                };
                let address = binding.main_address()?;
                ensure!(
                    source.main_address()? == address
                        && source.pins.data == Some(stage.arbiter.data as u8),
                    "PSG source byte differs from resolved MAIN data"
                );
                self.psg.capture = Some(Capture {
                    binding,
                    value: stage.arbiter.data as u8,
                    known: stage.data_mask as u8,
                });
            } else {
                self.validate_main_psg()?;
            }
        }
        if level && let Some(capture) = self.psg.capture {
            self.validate_psg_drive(capture, stage.arbiter, stage.data_mask)?;
        }
        if !level
            && self.psg.level
            && let Some(capture) = self.psg.capture.take()
        {
            self.psg.sequence = self
                .psg
                .sequence
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("PSG receipt overflow"))?;
            let receipt = PsgWrite {
                binding: capture.binding,
                sequence: self.psg.sequence,
                value: capture.value,
                half: view.half,
                pass: view.pass,
            };
            receipt.validate()?;
            self.psg.pending = Some(receipt);
        }
        self.psg.level = level;
        Ok(())
    }

    fn validate_psg_drive(
        &self,
        capture: Capture,
        input: arbiter::Inputs,
        mask: u16,
    ) -> Result<()> {
        let address = capture.binding.main_address()?;
        ensure!(
            self.grant.tristate()
                && !input.address_strobe
                && !input.read
                && input.upper
                && !input.lower
                && input.address == address >> 1,
            "SOUND PSG physical grant/address/lane differs"
        );
        ensure!(
            capture.known == 255 && mask & 255 == 255 && input.data as u8 == capture.value,
            "SOUND PSG byte is unknown or changed during pulse"
        );
        Ok(())
    }

    fn validate_main_psg(&self) -> Result<()> {
        ensure!(
            self.protocol_request()
                .is_some_and(|binding| binding.target == ProtocolTarget::Psg
                    && matches!(binding.request.operation, main_bus::Operation::Write(_))
                    && binding.request.access.lanes != ByteLanes::Upper),
            "PSG pulse lacks its MAIN protocol owner"
        );
        Ok(())
    }

    pub(crate) fn validate_psg(&self) -> Result<()> {
        ensure!(
            self.psg.pending.is_none(),
            "unconsumed SOUND PSG receipt in state"
        );
        if let Some(capture) = self.psg.capture {
            ensure!(
                self.psg.level
                    && capture.binding.half != 0
                    && (capture.binding.half, capture.binding.pass) <= (self.half, 1),
                "invalid active SOUND PSG capture"
            );
            self.validate_psg_drive(capture, self.arbiter.held_inputs(), self.sound.main_mask)?;
        } else if self.psg.level {
            self.validate_main_psg()?;
        }
        Ok(())
    }
}
