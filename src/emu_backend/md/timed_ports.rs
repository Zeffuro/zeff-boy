use crate::emu_backend::EmuBackend;
use crate::emu_thread::{MdTimedPortFailure, MdTimedPortFailureKind, MdTimedPortOutcome};
use zeff_md_core::emulator::{DebugPortError, DebugPortWrite};

impl EmuBackend {
    pub(crate) fn md_timed_port_queued_pcm(&self) -> Option<usize> {
        let Self::Md(md) = self else {
            return None;
        };
        Some(md.core().clone().drain_audio_samples().len())
    }

    pub(crate) fn execute_md_timed_ports(
        &mut self,
        writes: &[DebugPortWrite],
    ) -> Result<MdTimedPortOutcome, MdTimedPortFailure> {
        let Self::Md(md) = self else {
            return Err(MdTimedPortFailure::new(
                MdTimedPortFailureKind::Context,
                "timed ports require the Mega Drive backend",
            ));
        };
        if md.rollback_owned()
            || !md.suspended
            || md.execution_faulted
            || md.runtime_fault.is_some()
        {
            return Err(MdTimedPortFailure::new(
                MdTimedPortFailureKind::Lifecycle,
                "pause a healthy Mega Drive CPU before writing timed ports",
            ));
        }
        let fields = md.core().frame_count();
        let core = md.machine.normal_mut().map_err(|error| {
            MdTimedPortFailure::new(MdTimedPortFailureKind::Lifecycle, error.to_string())
        })?;
        let report = match md.sound_trace.attachment() {
            Some(mut sound) => core.debug_write_ports_sound_traced(writes, &mut sound),
            None => core.debug_write_ports(writes),
        }
        .map_err(failure)?;
        if report.writes != 0 {
            md.sound_trace.accept_forward_transaction();
        }
        md.finish_sound_watch_boundary(false);
        Ok(MdTimedPortOutcome {
            writes: report.writes,
            cpu_clocks: report.cpu_clocks.0,
            master_ticks: report.elapsed.get(),
            instructions: report.instructions,
            fields_crossed: md.core().frame_count() - fields,
        })
    }
}

fn failure(error: DebugPortError) -> MdTimedPortFailure {
    match error {
        DebugPortError::UnsupportedPort { index } => MdTimedPortFailure {
            kind: MdTimedPortFailureKind::Port,
            index: Some(index),
            reason: "unsupported timed port address or width".into(),
        },
        DebugPortError::Rejected(reason) => {
            MdTimedPortFailure::new(MdTimedPortFailureKind::Lifecycle, reason)
        }
        DebugPortError::Execution { index, fault } => MdTimedPortFailure {
            kind: MdTimedPortFailureKind::Execution,
            index: Some(index),
            reason: format!("{fault:?}"),
        },
        DebugPortError::Checkpoint(reason) => {
            MdTimedPortFailure::new(MdTimedPortFailureKind::Checkpoint, reason)
        }
    }
}
