use super::MdBackend;
use super::debug::PcBreakpoints;
use crate::emu_thread::GuestCallRequest;
use crate::symbols::ExecMode;
use anyhow::{Result, anyhow, ensure};
use std::sync::atomic::{AtomicU64, Ordering};
use zeff_m68k::timing::TimedOutcome;

#[cfg(test)]
mod svp_history_tests;

static NEXT_UNDO: AtomicU64 = AtomicU64::new(1);

struct HostCheckpoint {
    debugger: PcBreakpoints,
    suspended: bool,
    runtime_fault: Option<String>,
    execution_faulted: bool,
    audio_generation_enabled: Option<bool>,
    rewind_audio_changed: bool,
}

impl HostCheckpoint {
    fn capture(backend: &MdBackend) -> Self {
        Self {
            debugger: backend.debugger.clone(),
            suspended: backend.suspended,
            runtime_fault: backend.runtime_fault.clone(),
            execution_faulted: backend.execution_faulted,
            audio_generation_enabled: backend.audio_generation_enabled,
            rewind_audio_changed: backend.rewind_audio_changed.get(),
        }
    }

    fn restore(self, backend: &mut MdBackend) {
        if backend.rollback_owned() {
            return;
        }
        backend.debugger = self.debugger;
        backend.suspended = self.suspended;
        backend.runtime_fault = self.runtime_fault;
        backend.execution_faulted = self.execution_faulted;
        backend.audio_generation_enabled = self.audio_generation_enabled;
        backend.rewind_audio_changed.set(self.rewind_audio_changed);
    }
}

pub(super) struct GuestCallUndo {
    token: [u8; 16],
    checkpoint: Vec<u8>,
    host: HostCheckpoint,
}

fn undo_token() -> Result<[u8; 16]> {
    let serial = NEXT_UNDO
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| anyhow!("guest-call undo identifiers are exhausted"))?;
    let mut token = [0; 16];
    token[..8].copy_from_slice(b"ZMDUNDO1");
    token[8..].copy_from_slice(&serial.to_le_bytes());
    Ok(token)
}

impl MdBackend {
    fn require_guest_call_ready(&self) -> Result<()> {
        self.require_normal()?;
        ensure!(self.suspended, "CPU must be suspended");
        ensure!(!self.execution_faulted, "Mega Drive execution is faulted");
        Ok(())
    }

    pub(in crate::emu_backend) fn execute_guest_call(
        &mut self,
        request: &GuestCallRequest,
    ) -> Result<(u64, Vec<u8>)> {
        self.require_guest_call_ready()?;
        ensure!(
            request.exec_mode == ExecMode::M68000,
            "expected M68000 code"
        );
        ensure!(
            request.instruction_budget > 0,
            "instruction budget must be positive"
        );
        ensure!(
            request.target != self.machine.core().registers().pc,
            "call target is the current PC"
        );
        self.machine
            .core()
            .check_debug_call(request.target)
            .map_err(|error| anyhow!("Mega Drive guest call rejected: {error:?}"))?;
        let checkpoint = self.machine.core().encode_debugger_checkpoint()?;
        let host = HostCheckpoint::capture(self);
        let token = undo_token()?;
        let transaction = self
            .sound_trace
            .begin_transaction(self.machine.normal_mut()?);
        let machine = transaction.is_none().then(|| self.machine.core().clone());
        match self.run_guest_call(request) {
            Ok(instructions) => {
                if let Some(transaction) = transaction {
                    self.sound_trace.accept_transaction(transaction);
                }
                self.guest_call_undo = Some(GuestCallUndo {
                    token,
                    checkpoint,
                    host,
                });
                Ok((instructions, token.to_vec()))
            }
            Err(error) => {
                if let Some(transaction) = transaction {
                    self.sound_trace
                        .abort_transaction(self.machine.normal_mut()?, transaction);
                } else {
                    *self.same_branch_core_mut()? = machine.expect("ordinary guest-call snapshot");
                }
                host.restore(self);
                Err(anyhow!("{error}; state restored"))
            }
        }
    }

    fn run_guest_call(&mut self, request: &GuestCallRequest) -> Result<u64> {
        self.require_normal()?;
        self.debugger.clear_stop();
        self.sound_trace.resume_watches();
        self.suspended = false;
        let dma_before = self.machine.core().debug_dma_activity_epoch();
        let core = self.machine.normal_mut()?;
        let context = match self.sound_trace.attachment() {
            Some(mut sound) => core.begin_debug_call_sound_traced(request.target, &mut sound),
            None => core.begin_debug_call(request.target),
        }
        .map_err(|error| anyhow!("Mega Drive guest call entry failed: {error:?}"))?;
        self.finish_sound_watch_boundary(false);
        ensure!(!self.suspended, "call hit a SOUND watch during entry");
        ensure!(
            !self.debugger.check_dma(dma_before, self.machine.core()),
            "call hit a debugger stop during entry"
        );
        let mut instructions = 0;
        loop {
            if self.machine.core().debug_call_returned(&context) {
                self.same_branch_core_mut()?
                    .finish_debug_call(context)
                    .map_err(|error| anyhow!("Mega Drive guest call return failed: {error:?}"))?;
                self.machine.core().encode_frontend_state()?;
                self.suspended = true;
                return Ok(instructions);
            }
            ensure!(
                instructions < request.instruction_budget,
                "call exceeded its instruction budget"
            );
            ensure!(!self.machine.core().main_cpu_stopped(), "call entered STOP");
            ensure!(
                self.machine.core().interrupt_level() != 7,
                "call encountered level 7 interrupt input"
            );
            ensure!(
                !self.debugger.should_stop(self.machine.core()),
                "call hit a debugger stop"
            );
            let step = self
                .step_debug_instruction()
                .map_err(|fault| anyhow!("Mega Drive guest call machine fault: {fault:?}"))?;
            if matches!(step.cpu, TimedOutcome::Instruction(_)) {
                instructions += 1;
            }
            ensure!(!self.suspended, "call hit a debugger stop");
            ensure!(!self.machine.core().main_cpu_stopped(), "call entered STOP");
        }
    }

    pub(in crate::emu_backend) fn undo_guest_call(&mut self, token: &[u8]) -> Result<()> {
        self.require_guest_call_ready()?;
        let undo = self
            .guest_call_undo
            .as_ref()
            .ok_or_else(|| anyhow!("no Mega Drive guest call is available to undo"))?;
        ensure!(
            token == undo.token,
            "guest-call undo token is stale or belongs to another backend"
        );
        let mut candidate = self.machine.core().clone();
        candidate.decode_debugger_checkpoint(&undo.checkpoint)?;

        let undo = self
            .guest_call_undo
            .take()
            .expect("validated guest-call undo");
        *self.same_branch_core_mut()? = candidate;
        undo.host.restore(self);
        self.rotate_trace_stream();
        self.replace_sound_unwitnessed(
            "Host undo restored the current machine; SOUND history was not restored",
        );
        Ok(())
    }
}
