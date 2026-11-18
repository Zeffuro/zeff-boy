use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ProtocolTarget {
    Io,
    SoundRam,
    SoundControl,
    Time,
    Psg,
}

impl ProtocolTarget {
    fn validate(self, request: main_bus::Request) -> Result<()> {
        let address = request.access.bus_address;
        let selected = match self {
            Self::Io => address & 0xffff00 == 0xa10000,
            Self::SoundRam => address & 0xff0000 == 0xa00000,
            Self::SoundControl => address & 0xfffc00 == 0xa11000,
            Self::Time => address & 0xffff00 == 0xa13000,
            Self::Psg => {
                address & 0xe700e0 == 0xc00000
                    && (address >> 1) & 12 == 8
                    && matches!(request.operation, main_bus::Operation::Write(_))
            }
        };
        ensure!(
            selected && request.operation != main_bus::Operation::InterruptAcknowledge,
            "MAIN protocol target does not match request"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProtocolRequest {
    pub request: main_bus::Request,
    pub start_half: u64,
    pub target: ProtocolTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProtocolDrive {
    pub binding: ProtocolRequest,
    pub half: u64,
    pub pass: u8,
    pub word: u16,
    pub mask: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProtocolCompletion {
    Read(ProtocolRequest),
    Write { binding: ProtocolRequest, word: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Completion {
    pub main: main_bus::Receipt,
    pub protocol: Option<ProtocolCompletion>,
    pub origin: [InstructionByteOrigin; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub target: ProtocolTarget,
    pub start_half: u64,
}

impl Board {
    pub fn offer_protocol(
        &mut self,
        request: main_bus::Request,
        target: ProtocolTarget,
    ) -> Result<()> {
        ensure!(
            self.ready_tick().is_some() && self.protocol.is_none(),
            "MAIN protocol offer is not ready"
        );
        target.validate(request)?;
        let main = self
            .main
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("MAIN clock prefix is incomplete"))?;
        main.offer(request)
            .map_err(|error| anyhow::anyhow!("invalid MAIN protocol request: {error:?}"))?;
        self.protocol = Some(Binding {
            target,
            start_half: main.snapshot().last_edge_half,
        });
        self.protocol_seen = false;
        self.instruction_origin.0.clear_sample();
        Ok(())
    }

    pub fn protocol_request(&self) -> Option<ProtocolRequest> {
        let binding = self.protocol?;
        let snapshot = self.main.as_ref()?.snapshot();
        let request = snapshot
            .pending
            .map(|pending| pending.request)
            .or_else(|| snapshot.completed.map(|completed| completed.request))?;
        Some(ProtocolRequest {
            request,
            start_half: binding.start_half,
            target: binding.target,
        })
    }

    pub fn qualify_protocol_drive(&mut self, drive: ProtocolDrive) -> Result<()> {
        ensure!(self.stage.is_none(), "MAIN protocol drive already pending");
        ensure!(
            drive.pass == self.next_pass
                && Some(drive.half)
                    == (if self.next_pass == 0 {
                        self.half.checked_add(1)
                    } else {
                        Some(self.half)
                    })
                && self.protocol_request() == Some(drive.binding),
            "stale MAIN protocol drive"
        );
        if let Some(previous) = self.protocol_drive {
            ensure!(
                self.next_pass == 1
                    && previous.pass == 0
                    && previous.half == drive.half
                    && previous.binding == drive.binding,
                "MAIN protocol drive was not replaced chronologically"
            );
        }
        ensure!(
            self.main
                .as_ref()
                .is_some_and(|main| main.snapshot().pending.is_some())
                && drive.binding.request.operation == main_bus::Operation::Read,
            "MAIN protocol drive is not a pending read"
        );
        ensure!(
            drive.word & !drive.mask == 0,
            "MAIN protocol data exceeds drive mask"
        );
        self.protocol_drive = Some(drive);
        Ok(())
    }

    pub(super) fn apply_protocol_drive(&mut self, stage: Stage, mask: &mut u16) -> Result<u16> {
        let Some(drive) = self.protocol_drive else {
            return Ok(0);
        };
        ensure!(
            drive.half == stage.preview.half
                && drive.pass == stage.preview.pass
                && self.protocol_request() == Some(drive.binding),
            "stale MAIN protocol source"
        );
        if !stage.arbiter.address_strobe
            && stage.arbiter.read
            && (!stage.arbiter.upper || !stage.arbiter.lower)
        {
            ensure!(
                self.address == drive.binding.request.access.bus_address,
                "MAIN protocol address drive mismatch"
            );
            self.instruction_origin.0.clear_latch();
            self.data = (self.data & !drive.mask) | drive.word;
            *mask |= drive.mask;
            self.protocol_seen = true;
            return Ok(drive.mask);
        }
        Ok(0)
    }

    pub(crate) fn validate_protocol(&self) -> Result<()> {
        if let Some(binding) = self.protocol {
            let source = self
                .protocol_request()
                .ok_or_else(|| anyhow::anyhow!("MAIN protocol has no request"))?;
            binding.target.validate(source.request)?;
            let snapshot = self
                .main
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("MAIN protocol lacks clock"))?
                .snapshot();
            let start = snapshot
                .pending
                .map(|pending| pending.start_half)
                .or_else(|| snapshot.completed.map(|completed| completed.start_half));
            ensure!(
                start == Some(binding.start_half),
                "MAIN protocol start does not match cycle"
            );
            if source.request.operation != main_bus::Operation::Read {
                ensure!(
                    !self.protocol_seen && self.protocol_drive.is_none(),
                    "MAIN protocol write has read drive history"
                );
            } else if snapshot.completed.is_some() || snapshot.phase == main_bus::Phase::S7 {
                ensure!(
                    self.protocol_seen,
                    "MAIN sampled protocol read has no drive history"
                );
            }
        } else {
            ensure!(
                !self.protocol_seen && self.protocol_drive.is_none(),
                "unowned MAIN protocol drive history"
            );
        }
        if let Some(drive) = self.protocol_drive {
            ensure!(
                drive.pass == 0
                    && Some(drive.half) == self.half.checked_add(1)
                    && self.protocol_request() == Some(drive.binding)
                    && drive.binding.request.operation == main_bus::Operation::Read
                    && drive.word & !drive.mask == 0,
                "invalid future MAIN protocol drive"
            );
            ensure!(
                self.main
                    .as_ref()
                    .is_some_and(|main| main.snapshot().pending.is_some()),
                "MAIN protocol drive follows completion"
            );
        }
        Ok(())
    }

    pub(crate) fn write_protocol(&self, writer: &mut StateWriter) {
        writer.write_bool(self.protocol.is_some());
        if let Some(binding) = self.protocol {
            writer.write_u8(binding.target as u8);
            writer.write_u64(binding.start_half);
        }
        writer.write_bool(self.protocol_seen);
        writer.write_bool(self.protocol_drive.is_some());
        if let Some(drive) = self.protocol_drive {
            writer.write_u64(drive.half);
            writer.write_u8(drive.pass);
            writer.write_u16(drive.word);
            writer.write_u16(drive.mask);
        }
    }

    pub(crate) fn read_protocol(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        self.protocol = if reader.read_bool()? {
            let target = match reader.read_u8()? {
                0 => ProtocolTarget::Io,
                1 => ProtocolTarget::SoundRam,
                2 => ProtocolTarget::SoundControl,
                3 => ProtocolTarget::Time,
                4 => ProtocolTarget::Psg,
                _ => anyhow::bail!("invalid MAIN protocol target"),
            };
            Some(Binding {
                target,
                start_half: reader.read_u64()?,
            })
        } else {
            None
        };
        self.protocol_seen = reader.read_bool()?;
        self.protocol_drive = if reader.read_bool()? {
            Some(ProtocolDrive {
                binding: self
                    .protocol_request()
                    .ok_or_else(|| anyhow::anyhow!("MAIN protocol drive has no request"))?,
                half: reader.read_u64()?,
                pass: reader.read_u8()?,
                word: reader.read_u16()?,
                mask: reader.read_u16()?,
            })
        } else {
            None
        };
        self.validate_protocol()
    }
}
