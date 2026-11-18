use super::super::{CallStackStatus, Emulator, Region, TimingModel};
use crate::{ControllerKind, ControllerPort};
use anyhow::{Result, ensure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdRollbackConfig {
    region: Region,
    timing: TimingModel,
    concurrent_sound: bool,
    pub(super) pads: [ControllerKind; 2],
    rom_digest: [u8; 32],
    rom_len: u64,
    master_hz: u64,
}

impl MdRollbackConfig {
    pub fn capture(core: &Emulator) -> Result<Self> {
        let config = Self::capture_policy(core)?;
        core.encode_state()?;
        Ok(config)
    }

    pub(super) fn capture_policy(core: &Emulator) -> Result<Self> {
        ensure!(
            !cfg!(any(
                feature = "sound-shadow-observer",
                feature = "first-failure-diagnostic"
            )),
            "MD rollback excludes instrumented builds"
        );
        ensure!(
            core.bus.cartridge.rollback_plain_factory() && core.bus.tmss.is_none(),
            "MD rollback requires an ordinary factory cartridge without save or firmware"
        );
        ensure!(
            !core.bus.audio.batch_active() && core.bus.audio.trace_origin.is_none(),
            "MD rollback requires a completed audio batch"
        );
        ensure!(
            core.sample_rate() == 48_000
                && core.bus.audio.sample_generation_enabled()
                && core.apu_channel_mutes().iter().all(|&mute| !mute),
            "MD rollback requires full 48000 Hz audio"
        );
        ensure!(
            !core.bus.audio.trace.is_active()
                && core.call_stack_status() == CallStackStatus::Disabled
                && !core.cpu.control_flow_recording()
                && core.bus.video.rollback_capture_idle()
                && core.bus.irq_level == 0,
            "MD rollback excludes active traces and debugger recording"
        );
        ensure!(
            core.bus.input.rollback_parallel_only(),
            "MD rollback excludes serial input"
        );
        ensure!(
            !core.bus.clock_fault
                && core.bus.owned_fault().is_none()
                && core.sound_schedule.fault.is_none()
                && core.video_error().is_none(),
            "MD rollback excludes faulted runtime"
        );
        Ok(Self {
            region: core.region(),
            timing: core.timing_model(),
            concurrent_sound: core.concurrent_sound,
            pads: [ControllerPort::One, ControllerPort::Two]
                .map(|port| core.input().controller_kind(port)),
            rom_digest: core.bus.cartridge.rom_digest(),
            rom_len: core.rom_bytes().len() as u64,
            master_hz: core.region().master_hz(),
        })
    }

    pub fn controller_kinds(&self) -> [ControllerKind; 2] {
        self.pads
    }

    pub fn identity_bytes(&self) -> Vec<u8> {
        let mut bytes = b"ZMDOWN01ZMDPAD01".to_vec();
        bytes.extend(crate::save_state::STATE_VERSION.to_le_bytes());
        bytes.push(match self.region {
            Region::NtscUs => 0,
            Region::NtscJapan => 1,
            Region::Pal => 2,
        });
        bytes.push(match self.timing {
            TimingModel::Standard => 0,
            TimingModel::Precise => 1,
        });
        bytes.push(u8::from(self.concurrent_sound));
        for kind in self.pads {
            bytes.push(match kind {
                ControllerKind::ThreeButton => 0,
                ControllerKind::SixButton => 1,
            });
        }
        bytes.extend(48_000u32.to_le_bytes());
        bytes.extend([1, 0, 0]);
        bytes.extend(self.master_hz.to_le_bytes());
        bytes.extend(self.region.frame_ticks().to_le_bytes());
        bytes.extend(self.rom_len.to_le_bytes());
        bytes.extend(self.rom_digest);
        bytes
    }
}
