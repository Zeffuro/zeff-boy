use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use anyhow::{Result, ensure};

use super::*;
use crate::hardware::controller::{ControllerDevice, MultitapDevice, MultitapPort, PadButtons};
use crate::hardware::save_state::{PCE_SAVE_STATE_FORMAT_VERSION, encode_state};

#[derive(Debug)]
pub(super) struct RollbackOwner {
    valid: AtomicBool,
}

pub struct PceRollbackSession {
    owner: Arc<RollbackOwner>,
    machine: Arc<()>,
}

pub struct PceRollbackSnapshot {
    owner: Weak<RollbackOwner>,
    state: Vec<u8>,
    cpu: HuC6280,
    vdc: HuC6270,
    vdc2: Option<HuC6270>,
    frame: u64,
}

impl PceRollbackSnapshot {
    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn native_state(&self) -> &[u8] {
        &self.state
    }

    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.state.capacity()
            + std::mem::size_of_val(self.vdc.vram())
            + self
                .vdc2
                .as_ref()
                .map_or(0, |vdc| std::mem::size_of_val(vdc.vram()))
    }
}

impl PceMachine {
    pub(crate) fn invalidate_rollback_session(&mut self) {
        if let Some(owner) = self.rollback_owner.upgrade() {
            owner.valid.store(false, Ordering::Relaxed);
        }
        self.rollback_owner = Weak::new();
    }

    pub fn validate_rollback_boundary(&self) -> Result<()> {
        self.validate_v1_encode_state()?;
        ensure!(
            self.rollback_frame_boundary && self.devices().psg().rollback_audio_is_drained(),
            "rollback requires a fresh or completed frame"
        );
        ensure!(
            self.devices().cdrom2().is_none()
                && self.devices().arcade_card().is_none()
                && matches!(
                    self.hucard_board(),
                    PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce | PceHuCardBoard::Populous
                ),
            "rollback requires HuCard media without firmware or removable devices"
        );
        let (rate, generation, mutes, capture) = self.devices().psg().runtime_config();
        ensure!(
            rate == 48_000 && generation && !capture && !mutes.into_iter().any(|mute| mute),
            "rollback requires full 48000 Hz PSG audio without debug capture"
        );
        ensure!(
            self.execution_state == PceExecutionState::Running
                && !self.suspend_after_instruction
                && !self.skip_breakpoint_once
                && !self.opcode_history.enabled
                && !self.instruction_trace.is_enabled()
                && !self.audio_trace.is_enabled()
                && self.debug.iter_breakpoints().next().is_none()
                && self.debug.iter_one_shot_breakpoints().next().is_none()
                && self.debug.iter_breakpoint_hit_conditions().next().is_none()
                && self.debug.iter_event_breakpoints().next().is_none()
                && self.debug.watchpoints.is_empty()
                && !self.debug.break_on_next
                && self.debug.hit_breakpoint.is_none()
                && self.debug.hit_watchpoint.is_none()
                && self.debug.hit_event.is_none(),
            "rollback excludes debugger and traces"
        );
        let controller = self.devices().controller();
        ensure!(
            !controller.memory_base128().is_connected(),
            "rollback excludes Memory Base 128"
        );
        let ControllerDevice::Multitap(tap) = controller.device() else {
            anyhow::bail!("rollback requires the two-pad multitap topology");
        };
        ensure!(
            [MultitapPort::One, MultitapPort::Two]
                .into_iter()
                .all(|port| matches!(tap.port(port), MultitapDevice::TwoButton(_)))
                && [MultitapPort::Three, MultitapPort::Four, MultitapPort::Five]
                    .into_iter()
                    .all(|port| matches!(tap.port(port), MultitapDevice::Disconnected)),
            "rollback requires exactly two standard pads and three disconnected ports"
        );
        Ok(())
    }

    pub fn rollback_sample_rate(&self) -> u32 {
        self.devices().psg().runtime_config().0
    }

    pub fn begin_rollback_session(&mut self) -> Result<PceRollbackSession> {
        ensure!(
            self.rollback_owner
                .upgrade()
                .is_none_or(|owner| !owner.valid.load(Ordering::Relaxed)),
            "rollback session already active"
        );
        self.validate_rollback_boundary()?;
        let owner = Arc::new(RollbackOwner {
            valid: AtomicBool::new(true),
        });
        self.rollback_owner = Arc::downgrade(&owner);
        Ok(PceRollbackSession {
            owner,
            machine: self.rollback_machine.clone(),
        })
    }

    pub fn encode_rollback_runtime_state(&self) -> Vec<u8> {
        let mut writer = StateWriter::new();
        self.cpu
            .write_state(&mut writer, PCE_SAVE_STATE_FORMAT_VERSION);
        for vdc in std::iter::once(self.devices().vdc())
            .chain(self.devices().supergrafx_video().map(|video| video.vdc2()))
        {
            vdc.horizontal_state.write_state(&mut writer);
            vdc.scanline_state.write_state(&mut writer);
        }
        writer.write_bool(self.rollback_frame_boundary);
        writer.into_bytes()
    }
}

impl PceRollbackSession {
    pub fn retire(&self) {
        self.owner.valid.store(false, Ordering::Relaxed);
    }
    fn validate(&self, core: &PceMachine) -> Result<()> {
        let result = (|| {
            ensure!(
                self.owner.valid.load(Ordering::Relaxed)
                    && core.rollback_owner.ptr_eq(&Arc::downgrade(&self.owner))
                    && Arc::ptr_eq(&self.machine, &core.rollback_machine),
                "stale or foreign rollback session"
            );
            core.validate_rollback_boundary()
        })();
        if result.is_err() {
            self.owner.valid.store(false, Ordering::Relaxed);
        }
        result
    }

    pub fn capture(&self, core: &PceMachine) -> Result<PceRollbackSnapshot> {
        self.validate(core)?;
        let mut state = encode_state(core)?;
        state.shrink_to_fit();
        Ok(PceRollbackSnapshot {
            owner: Arc::downgrade(&self.owner),
            state,
            cpu: core.cpu.clone(),
            vdc: core.devices().vdc().clone(),
            vdc2: core
                .devices()
                .supergrafx_video()
                .map(|video| video.vdc2().clone()),
            frame: core.frame_count(),
        })
    }

    pub fn restore(&self, core: &mut PceMachine, snapshot: &PceRollbackSnapshot) -> Result<()> {
        self.validate(core)?;
        self.restore_after_session(core, snapshot, snapshot.native_state())
    }

    pub fn restore_after_session(
        &self,
        core: &mut PceMachine,
        snapshot: &PceRollbackSnapshot,
        checkpoint: &[u8],
    ) -> Result<()> {
        let result = (|| {
            ensure!(
                Arc::ptr_eq(&self.machine, &core.rollback_machine),
                "foreign rollback machine"
            );
            ensure!(
                core.rollback_owner.upgrade().is_none()
                    || core.rollback_owner.ptr_eq(&Arc::downgrade(&self.owner)),
                "another rollback session is active"
            );
            ensure!(
                snapshot.owner.ptr_eq(&Arc::downgrade(&self.owner)),
                "foreign rollback snapshot"
            );
            ensure!(
                checkpoint == snapshot.native_state(),
                "rollback restoration checkpoint differs"
            );
            core.invalidate_rollback_session();
            core.faulted = false;
            core.bus.devices_mut().psg_mut().apply_runtime_config(
                48_000,
                true,
                [false; super::super::psg::PSG_CHANNEL_COUNT],
                false,
            );
            // Owned snapshots were validated at capture; restore their payload without copying immutable ROM.
            let mut reader = StateReader::new(snapshot.native_state());
            let mut header = [0; 50];
            reader.read_exact(&mut header)?;
            let payload = reader.read_slice(8 * 1024 * 1024)?;
            core.read_owned_rollback_payload(
                payload,
                core.hardware_topology(),
                false,
                false,
                PCE_SAVE_STATE_FORMAT_VERSION,
            )?;
            core.cpu = snapshot.cpu.clone();
            *core.bus.devices_mut().vdc_mut() = snapshot.vdc.clone();
            if let Some(vdc2) = &snapshot.vdc2 {
                *core
                    .bus
                    .devices_mut()
                    .supergrafx_video_mut()
                    .expect("owned SuperGrafx snapshot")
                    .vdc2_mut() = vdc2.clone();
            }
            core.faulted = false;
            core.rollback_frame_boundary = true;
            core.validate_rollback_boundary()?;
            self.owner.valid.store(true, Ordering::Relaxed);
            core.rollback_owner = Arc::downgrade(&self.owner);
            Ok(())
        })();
        if result.is_err() {
            self.owner.valid.store(false, Ordering::Relaxed);
        }
        result
    }

    pub fn advance_frame(&self, core: &mut PceMachine, ports: [u8; 2]) -> Result<Vec<f32>> {
        self.validate(core)?;
        self.retire();
        let next = core
            .frame_count()
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("rollback frame overflow"))?;
        core.invalidate_rollback_session();
        let tap = core
            .bus
            .devices_mut()
            .controller_mut()
            .multitap_mut()
            .expect("validated multitap");
        for (port, value) in [MultitapPort::One, MultitapPort::Two]
            .into_iter()
            .zip(ports)
        {
            let MultitapDevice::TwoButton(pad) = tap.port_mut(port) else {
                unreachable!()
            };
            let mut buttons = PadButtons::empty();
            for (bit, button) in [
                PadButtons::I,
                PadButtons::II,
                PadButtons::SELECT,
                PadButtons::RUN,
                PadButtons::RIGHT,
                PadButtons::LEFT,
                PadButtons::UP,
                PadButtons::DOWN,
            ]
            .into_iter()
            .enumerate()
            {
                buttons.set(button, value & (1 << bit) != 0);
            }
            pad.set_buttons(buttons);
        }
        let run = core.run_until_frame()?;
        ensure!(
            run.frames_published() == 1 && core.frame_count() == next,
            "rollback frame did not complete exactly once; session retired"
        );
        let mut audio = Vec::new();
        core.bus.devices_mut().drain_audio_samples_into(&mut audio);
        core.rollback_frame_boundary = true;
        core.validate_rollback_boundary()?;
        self.owner.valid.store(true, Ordering::Relaxed);
        core.rollback_owner = Arc::downgrade(&self.owner);
        Ok(audio)
    }
}

#[cfg(test)]
mod tests;
