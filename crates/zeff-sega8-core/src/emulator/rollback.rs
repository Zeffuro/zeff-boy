use std::sync::{Arc, Weak};

use anyhow::{Result, ensure};

use super::Emulator;
use crate::hardware::{
    apu::Apu,
    cartridge::Sega8System,
    cpu::Cpu,
    input::{ControllerPort, Input},
    vdp::Vdp,
};

pub struct Sega8RollbackSession {
    owner: Arc<()>,
}

// Local runtime state supplements the deliberately canonical native audio/video state.
pub struct Sega8RollbackSnapshot {
    owner: Weak<()>,
    state: Vec<u8>,
    cpu: Cpu,
    vdp: Vdp,
    apu: Apu,
    input: Input,
    frame: u64,
    system: Sega8System,
}

impl Sega8RollbackSnapshot {
    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.state.capacity()
            + self.vdp.rollback_allocation_bytes()
            + self.apu.rollback_allocation_bytes()
    }
}

impl Emulator {
    pub fn begin_rollback_session(&mut self) -> Result<Sega8RollbackSession> {
        ensure!(
            self.rollback_owner.upgrade().is_none(),
            "rollback session already active"
        );
        validate_boundary(self)?;
        let owner = Arc::new(());
        self.rollback_owner = Arc::downgrade(&owner);
        Ok(Sega8RollbackSession { owner })
    }

    pub(crate) fn invalidate_rollback_session(&mut self) {
        self.rollback_owner = Weak::new();
    }

    pub fn encode_rollback_runtime_state(&self) -> Vec<u8> {
        let mut state = Vec::new();
        state.extend_from_slice(&self.bus.apu().rollback_sample_phase().to_le_bytes());
        state.extend_from_slice(&self.bus.input().console_pause_runtime());
        self.bus.vdp().append_rollback_runtime_state(&mut state);
        state
    }
}

impl Sega8RollbackSession {
    fn validate(&self, core: &Emulator) -> Result<()> {
        ensure!(
            core.rollback_owner.ptr_eq(&Arc::downgrade(&self.owner)),
            "stale or foreign rollback session"
        );
        validate_boundary(core)
    }

    pub fn capture(&self, core: &Emulator) -> Result<Sega8RollbackSnapshot> {
        self.validate(core)?;
        Ok(Sega8RollbackSnapshot {
            owner: Arc::downgrade(&self.owner),
            state: core.encode_state()?,
            cpu: core.cpu.clone(),
            vdp: core.bus.vdp().clone(),
            apu: core.bus.apu().clone(),
            input: core.bus.input().clone(),
            frame: core.frame_count(),
            system: core.system(),
        })
    }

    pub fn restore(&self, core: &mut Emulator, snapshot: &Sega8RollbackSnapshot) -> Result<()> {
        self.validate(core)?;
        self.restore_after_session(core, snapshot, &snapshot.state)
    }

    pub fn restore_after_session(
        &self,
        core: &mut Emulator,
        snapshot: &Sega8RollbackSnapshot,
        state: &[u8],
    ) -> Result<()> {
        ensure!(core.system() == snapshot.system, "rollback console differs");
        ensure!(
            core.rollback_owner.upgrade().is_none()
                || core.rollback_owner.ptr_eq(&Arc::downgrade(&self.owner)),
            "another rollback session is active"
        );
        ensure!(
            state == snapshot.state,
            "rollback restoration checkpoint differs"
        );
        ensure!(
            snapshot.owner.ptr_eq(&Arc::downgrade(&self.owner)),
            "stale or foreign rollback snapshot"
        );
        core.load_state(&snapshot.state)?;
        core.cpu = snapshot.cpu.clone();
        *core.bus.vdp_mut() = snapshot.vdp.clone();
        *core.bus.apu_mut() = snapshot.apu.clone();
        *core.bus.input_mut() = snapshot.input.clone();
        core.rollback_owner = Arc::downgrade(&self.owner);
        core.rollback_frame_boundary = true;
        Ok(())
    }

    pub fn advance_frame(&self, core: &mut Emulator, ports: [u8; 2]) -> Result<Vec<f32>> {
        self.validate(core)?;
        let next = core
            .frame_count()
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("rollback frame overflow"))?;
        core.invalidate_rollback_session();
        for (port, value) in [ControllerPort::One, ControllerPort::Two]
            .into_iter()
            .zip(ports)
        {
            let raw = !((value >> 4 & 0x0f) | ((value & 3) << 4));
            core.bus.input_mut().set_controller_raw(port, raw);
        }
        if core.system() == Sega8System::MasterSystem {
            // One console button: overlapping player holds must not generate extra edges.
            core.bus
                .input_mut()
                .set_console_pause_pressed(ControllerPort::One, (ports[0] | ports[1]) & 8 != 0);
            core.bus
                .input_mut()
                .set_console_pause_pressed(ControllerPort::Two, false);
        }
        core.step_frame();
        ensure!(
            core.rollback_frame_boundary && core.frame_count() == next,
            "rollback frame did not complete exactly once; session retired"
        );
        let audio = core.drain_audio_samples();
        core.rollback_owner = Arc::downgrade(&self.owner);
        Ok(audio)
    }
}

fn validate_boundary(core: &Emulator) -> Result<()> {
    ensure!(
        matches!(
            core.system(),
            Sega8System::MasterSystem | Sega8System::Sg1000
        ) && !core.bus.has_boot_rom(),
        "rollback requires a fixed Sega console without firmware"
    );
    let apu = core.bus.apu();
    ensure!(
        core.sample_rate() == 48_000
            && apu.sample_rate() == 48_000
            && apu.sample_generation_enabled()
            && apu.channel_mutes() == [false; 4],
        "rollback requires full 48000 Hz audio"
    );
    ensure!(
        !core.is_suspended() && !core.has_debugger_stop_controls() && core.rom_patches().is_empty(),
        "rollback excludes debugger, traces and cheats"
    );
    ensure!(
        core.rollback_frame_boundary && apu.buffered_sample_count() == 0,
        "rollback requires a fresh or drained completed frame"
    );
    Ok(())
}

impl Clone for Emulator {
    fn clone(&self) -> Self {
        Self {
            cpu: self.cpu.clone(),
            bus: self.bus.clone(),
            rom_hash: self.rom_hash,
            frame_count: self.frame_count,
            framebuffer: self.framebuffer.clone(),
            sample_rate: self.sample_rate,
            video_standard: self.video_standard,
            console_region: self.console_region,
            debug: self.debug.clone(),
            opcode_log: self.opcode_log.clone(),
            instruction_trace: self.instruction_trace.clone(),
            rollback_owner: Weak::new(),
            rollback_frame_boundary: self.rollback_frame_boundary,
        }
    }
}

#[cfg(test)]
mod tests;
