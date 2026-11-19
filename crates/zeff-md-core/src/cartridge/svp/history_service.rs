use super::{InstructionModel, model::Phase, service::Replayed, ssp1601::Fault};
impl InstructionModel {
    pub(super) fn validate_historical_services(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.has_rom_history(), "empty historical component");
        self.validate_rom_history()?;
        anyhow::ensure!(
            matches!(self.phase, Phase::Execute { .. } | Phase::Commit { .. }),
            "history outside execution"
        );
        let result = self.replay_services(true);
        match (&self.phase, result) {
            (
                Phase::Commit { core, ports, step },
                Ok(Replayed::Ready(next, controller, expected)),
            ) => {
                anyhow::ensure!(
                    core == &next && ports == &controller && step == &expected,
                    "historical pending semantic transaction differs"
                );
            }
            (Phase::Commit { .. }, _) => anyhow::bail!("historical commit frontier"),
            (_, Err(Fault::InvalidState(reason))) => anyhow::bail!("{reason}"),
            (_, Err(Fault::ServicePending)) => anyhow::bail!("unresolved historical frontier"),
            _ => {}
        }
        if let Phase::Execute { pc, opcode } = self.phase {
            let capture = &self.services.as_ref().unwrap()[0];
            anyhow::ensure!(
                capture.request.pc == pc && capture.response == opcode,
                "historical opcode identity"
            );
        }
        Ok(())
    }
}
