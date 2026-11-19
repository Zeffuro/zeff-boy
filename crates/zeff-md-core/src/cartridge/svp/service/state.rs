use super::{
    Capture, InstructionModel, MAX_CAPTURES, ServiceRequest, ServiceResource, ServiceRole,
};
use crate::svp::{model::Phase, ssp1601::Fault};
use zeff_emu_common::save_state::{StateReader, StateWriter};

impl InstructionModel {
    pub(in super::super) fn write_services(&self, w: &mut StateWriter) {
        let Some(captures) = &self.services else {
            return;
        };
        w.write_bytes(b"ZSVPSVC1");
        w.write_u8(captures.len() as u8);
        for capture in captures {
            let request = capture.request;
            let (role, index) = match request.role {
                ServiceRole::ResetVector => (0, 0),
                ServiceRole::Opcode => (1, 0),
                ServiceRole::InstructionWord => (2, 0),
                ServiceRole::ProgramOperand => (3, 0),
                ServiceRole::PortRead(index) => (4, index),
                ServiceRole::PortWrite(index) => (5, index),
                ServiceRole::StatusInputs => (6, 0),
            };
            w.write_u64(request.instruction);
            w.write_u16(request.pc);
            w.write_u8(request.ordinal);
            w.write_u8(role);
            w.write_u8(index);
            w.write_u8(match request.resource {
                ServiceResource::ProgramIram => 0,
                ServiceResource::ProgramIrom => 1,
                ServiceResource::SharedRom => 2,
                ServiceResource::SharedDram => 3,
                ServiceResource::ExternalIram => 4,
                ServiceResource::Controller => 5,
            });
            w.write_u32(request.address);
            w.write_u16(request.mode);
            w.write_u16(request.status);
            w.write_bool(request.blind);
            w.write_u16(request.value);
            w.write_u16(capture.response);
        }
    }

    pub(in super::super) fn read_services(
        &self,
        r: &mut StateReader<'_>,
    ) -> anyhow::Result<Option<Vec<Capture>>> {
        if self.services.is_none() {
            return Ok(None);
        }
        let mut tag = [0; 8];
        r.read_exact(&mut tag)?;
        anyhow::ensure!(&tag == b"ZSVPSVC1", "missing service owner");
        let count = usize::from(r.read_u8()?);
        anyhow::ensure!(count <= MAX_CAPTURES, "service capture bound");
        let mut captures = Vec::with_capacity(count);
        for _ in 0..count {
            let instruction = r.read_u64()?;
            let pc = r.read_u16()?;
            let ordinal = r.read_u8()?;
            let role = r.read_u8()?;
            let index = r.read_u8()?;
            let role = match (role, index) {
                (0, 0) => ServiceRole::ResetVector,
                (1, 0) => ServiceRole::Opcode,
                (2, 0) => ServiceRole::InstructionWord,
                (3, 0) => ServiceRole::ProgramOperand,
                (4, 0..=7) => ServiceRole::PortRead(index),
                (5, 0..=7) => ServiceRole::PortWrite(index),
                (6, 0) => ServiceRole::StatusInputs,
                _ => anyhow::bail!("service role"),
            };
            let resource = match r.read_u8()? {
                0 => ServiceResource::ProgramIram,
                1 => ServiceResource::ProgramIrom,
                2 => ServiceResource::SharedRom,
                3 => ServiceResource::SharedDram,
                4 => ServiceResource::ExternalIram,
                5 => ServiceResource::Controller,
                _ => anyhow::bail!("service resource"),
            };
            let request = ServiceRequest {
                instruction,
                pc,
                ordinal,
                role,
                resource,
                address: r.read_u32()?,
                mode: r.read_u16()?,
                status: r.read_u16()?,
                blind: r.read_bool()?,
                value: r.read_u16()?,
            };
            captures.push(Capture {
                request,
                response: r.read_u16()?,
            });
        }
        Ok(Some(captures))
    }

    pub(in super::super) fn validate_services(&self) -> anyhow::Result<()> {
        let Some(captures) = &self.services else {
            return Ok(());
        };
        if !matches!(self.phase, Phase::Execute { .. }) {
            anyhow::ensure!(
                captures.is_empty(),
                "captures outside instruction execution"
            );
            return Ok(());
        }
        anyhow::ensure!(!captures.is_empty(), "missing captured opcode");
        match self.replay_services(true) {
            Err(Fault::InvalidState(reason)) => anyhow::bail!("{reason}"),
            Err(Fault::ServicePending) => anyhow::bail!("unresolved service frontier"),
            // Semantic faults retain the successfully captured prefix for rollback/retry.
            _ => Ok(()),
        }
    }
}
