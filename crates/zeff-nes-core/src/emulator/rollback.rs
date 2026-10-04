use std::sync::{Arc, Weak};

use anyhow::{Result, ensure};

use super::Emulator;
use crate::hardware::{apu::Apu, bus::StateLoadRollback, cpu::Cpu, ppu::Ppu};

/// A local execution lease. Dropping it retires every snapshot it issued.
#[derive(Debug)]
pub struct NesRollbackSession {
    owner: Arc<()>,
}

/// An opaque runtime checkpoint; never a portable or network state format.
pub struct NesRollbackSnapshot {
    owner: Weak<()>,
    state: Vec<u8>,
    cpu: Cpu,
    ppu: Ppu,
    apu: Apu,
    bus: StateLoadRollback,
    cpu_odd_cycle: bool,
    mapper_runtime: Vec<u8>,
}

impl NesRollbackSnapshot {
    /// Encoded bytes only; runtime copies and their allocations are additional.
    pub fn encoded_native_bytes(&self) -> usize {
        self.state.len()
    }

    /// Inline storage plus owned allocation capacities, excluding allocator and Arc overhead.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.state.capacity()
            + self.mapper_runtime.capacity()
            + self.ppu.framebuffer.len()
            + self.apu.retained_sample_bytes()
    }

    pub fn frame(&self) -> u64 {
        self.ppu.frame_count + u64::from(self.ppu.frame_ready)
    }
}

impl Emulator {
    /// Execution transients hashed alongside native state for local checkpoints.
    pub fn encode_rollback_runtime_state(&self) -> Vec<u8> {
        self.bus.cartridge.capture_rollback_runtime_state()
    }

    /// Hardware eligible for source-pinned cross-platform rollback qualification.
    pub fn has_portable_rollback_hardware(&self) -> bool {
        self.has_standard_console_hardware()
            && self.has_standard_controller_topology()
            && self.bus.cartridge.has_fixed_rollback_hardware()
            && self.bus.cartridge.has_portable_rollback_execution()
    }

    pub fn begin_rollback_session(&mut self) -> Result<NesRollbackSession> {
        ensure!(
            self.rollback_owner.upgrade().is_none(),
            "rollback session already active"
        );
        validate_boundary(self)?;
        let owner = Arc::new(());
        self.rollback_owner = Arc::downgrade(&owner);
        Ok(NesRollbackSession { owner })
    }

    pub(crate) fn invalidate_rollback_session(&mut self) {
        self.rollback_owner = Weak::new();
    }

    pub(crate) fn invalidate_rollback_execution(&mut self) {
        self.invalidate_rollback_session();
        self.rollback_frame_boundary = false;
    }
}

impl NesRollbackSession {
    fn validate(&self, core: &Emulator) -> Result<()> {
        ensure!(
            core.rollback_owner.ptr_eq(&Arc::downgrade(&self.owner)),
            "stale or foreign rollback session"
        );
        validate_boundary(core)
    }

    pub fn capture(&self, core: &Emulator) -> Result<NesRollbackSnapshot> {
        self.validate(core)?;
        Ok(NesRollbackSnapshot {
            owner: Arc::downgrade(&self.owner),
            state: core.encode_state()?,
            cpu: core.cpu.clone(),
            ppu: core.bus.ppu.clone(),
            apu: core.bus.apu.clone(),
            bus: core.bus.capture_state_load_rollback(),
            cpu_odd_cycle: core.bus.cpu_odd_cycle,
            mapper_runtime: core.bus.cartridge.capture_rollback_runtime_state(),
        })
    }

    pub fn restore(&self, core: &mut Emulator, snapshot: &NesRollbackSnapshot) -> Result<()> {
        self.validate(core)?;
        ensure!(
            snapshot.owner.ptr_eq(&Arc::downgrade(&self.owner)),
            "stale or foreign rollback snapshot"
        );
        let cpu = snapshot.cpu.clone();
        let ppu = snapshot.ppu.clone();
        let apu = snapshot.apu.clone();
        // Public load is transactional on failure; all later operations are infallible.
        core.load_state(&snapshot.state)?;
        core.cpu = cpu;
        core.bus.ppu = ppu;
        core.bus.apu = apu;
        core.bus.restore_state_load_rollback(snapshot.bus);
        core.bus.cpu_odd_cycle = snapshot.cpu_odd_cycle;
        core.bus
            .cartridge
            .restore_rollback_runtime_state(&snapshot.mapper_runtime);
        core.rollback_frame_boundary = true;
        core.rollback_owner = Arc::downgrade(&self.owner);
        Ok(())
    }

    /// Advances one frame and returns its interleaved stereo PCM without publication.
    pub fn advance_frame(&self, core: &mut Emulator, ports: [u8; 2]) -> Result<Vec<f32>> {
        self.validate(core)?;
        let next_frame = core
            .frame_count()
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("rollback frame overflow"))?;
        core.invalidate_rollback_session();
        core.set_input_p1_raw(ports[0]);
        core.set_input_p2_raw(ports[1]);
        core.step_frame();
        if !core.rollback_frame_boundary || core.frame_count() != next_frame {
            core.invalidate_rollback_execution();
            anyhow::bail!("rollback frame did not complete exactly once; session retired");
        }
        let mut audio = Vec::new();
        core.drain_audio_into_stereo(&mut audio);
        core.rollback_owner = Arc::downgrade(&self.owner);
        Ok(audio)
    }
}

fn validate_boundary(core: &Emulator) -> Result<()> {
    ensure!(
        core.has_standard_console_hardware() && core.bus.cartridge.has_fixed_rollback_hardware(),
        "rollback requires fixed standard-console hardware"
    );
    ensure!(
        core.has_standard_controller_topology(),
        "rollback requires standard controllers"
    );
    ensure!(
        core.has_full_audio_output_at_rate(48_000),
        "rollback requires full 48000 Hz audio"
    );
    ensure!(
        core.has_default_video_palette(),
        "rollback requires the default palette"
    );
    ensure!(
        !core.is_cpu_suspended()
            && !core.debug.any_active()
            && !core.opcode_log.enabled
            && !core.instruction_trace.is_enabled()
            && !core.bus.audio_trace.is_enabled()
            && core.bus.game_genie.patches.is_empty(),
        "rollback excludes debugger, traces and cheats"
    );
    ensure!(
        core.rollback_frame_boundary,
        "rollback requires a fresh or completed frame"
    );
    ensure!(
        core.bus.apu.sample_buffer.is_empty(),
        "rollback requires drained PCM"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
