use super::super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use zeff_m68k::bus::{BUS_ADDRESS_MASK, FunctionCode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SoundEvent {
    pub request: main_bus::Request,
    pub target: ProtocolTarget,
    pub source_half: u64,
    pub tick: u64,
}

impl SoundEvent {
    pub(crate) fn validate(&self) -> Result<()> {
        let access = self.request.access;
        let main_bus::Operation::Write(word) = self.request.operation else {
            anyhow::bail!("SOUND event is not a MAIN write");
        };
        let logical = access.logical_address & BUS_ADDRESS_MASK;
        ensure!(
            access.bus_address == logical & !1
                && (logical & 1 != 0) == (access.lanes == ByteLanes::Lower)
                && access.kind != AccessKind::InterruptAcknowledge
                && access.function_code != FunctionCode::CpuSpace
                && (access.lanes == ByteLanes::Both || word as u8 == (word >> 8) as u8),
            "invalid SOUND event request"
        );
        let address = access.bus_address;
        let selected = match self.target {
            ProtocolTarget::SoundControl => address & 0xfffc00 == 0xa11000,
            ProtocolTarget::SoundRam => address & 0xff0000 == 0xa00000,
            ProtocolTarget::Psg => address & 0xe700e0 == 0xc00000 && (address >> 1) & 12 == 8,
            _ => false,
        };
        ensure!(
            selected && self.tick == main_bus::ceil_tick(self.source_half),
            "SOUND event target or timestamp differs"
        );
        Ok(())
    }

    pub(crate) fn write_state(self, w: &mut StateWriter) {
        w.write_u8(1);
        w.write_u64(self.source_half);
        w.write_u64(self.tick);
        let access = self.request.access;
        w.write_u32(access.logical_address);
        w.write_u32(access.bus_address);
        w.write_u8(access.function_code as u8);
        w.write_u8(match access.kind {
            AccessKind::Opcode => 0,
            AccessKind::Extension => 1,
            AccessKind::ResetVector => 2,
            AccessKind::Vector => 3,
            AccessKind::Stack => 4,
            AccessKind::Data => 5,
            AccessKind::Discard => 6,
            AccessKind::Prefetch => 7,
            AccessKind::InterruptAcknowledge => 8,
        });
        w.write_u8(match access.lanes {
            ByteLanes::Upper => 0,
            ByteLanes::Lower => 1,
            ByteLanes::Both => 2,
        });
        let main_bus::Operation::Write(word) = self.request.operation else {
            unreachable!("validated SOUND event write");
        };
        w.write_u16(word);
        w.write_u8(self.target as u8);
    }

    pub(crate) fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        ensure!(
            r.read_u8()? == 1,
            "unsupported deferred SOUND event version"
        );
        let source_half = r.read_u64()?;
        let tick = r.read_u64()?;
        let logical_address = r.read_u32()?;
        let bus_address = r.read_u32()?;
        let function_code = match r.read_u8()? {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            _ => anyhow::bail!("invalid SOUND event function code"),
        };
        let kind = match r.read_u8()? {
            0 => AccessKind::Opcode,
            1 => AccessKind::Extension,
            2 => AccessKind::ResetVector,
            3 => AccessKind::Vector,
            4 => AccessKind::Stack,
            5 => AccessKind::Data,
            6 => AccessKind::Discard,
            7 => AccessKind::Prefetch,
            _ => anyhow::bail!("invalid SOUND event access kind"),
        };
        let lanes = match r.read_u8()? {
            0 => ByteLanes::Upper,
            1 => ByteLanes::Lower,
            2 => ByteLanes::Both,
            _ => anyhow::bail!("invalid SOUND event lanes"),
        };
        let word = r.read_u16()?;
        let target = match r.read_u8()? {
            1 => ProtocolTarget::SoundRam,
            2 => ProtocolTarget::SoundControl,
            4 => ProtocolTarget::Psg,
            _ => anyhow::bail!("invalid deferred SOUND target"),
        };
        let event = Self {
            source_half,
            tick,
            target,
            request: main_bus::Request {
                access: BusAccess {
                    logical_address,
                    bus_address,
                    function_code,
                    kind,
                    lanes,
                },
                operation: main_bus::Operation::Write(word),
            },
        };
        event.validate()?;
        Ok(event)
    }
}
