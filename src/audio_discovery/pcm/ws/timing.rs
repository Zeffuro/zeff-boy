use std::collections::HashMap;

use serde_json::json;
use zeff_audio_discovery::ws_tose::WsToseTiming;
use zeff_ws_core::hardware::cpu::CpuState;

use super::*;

const MAX_ANALYSIS_FRAMES: usize = 24_000;
const DRIVER_START: u32 = 0xe00;
const DRIVER_END: u32 = 0xf6d;

#[cfg(test)]
mod tests;

impl WsSession {
    pub(super) fn measure_duration(&mut self, cancel: &AtomicBool) -> Result<()> {
        let Some(profile) = self.prepared.timing else {
            self.warnings.push("Stops at the requested duration; automatic song-end and loop detection are unavailable. Native channels are mixed together.".into());
            return Ok(());
        };
        let (name, idle_address, slots, stride, detect_loop) = match profile {
            WsToseTiming::FixedV14 { idle_address } => {
                ("ws-tose-fixed-v14", idle_address, 0xe1d, 0x2a, true)
            }
            WsToseTiming::FixedParagraph {
                idle_address,
                slots,
                profile,
            } => (profile, idle_address, u32::from(slots), 0x2a, false),
            WsToseTiming::DirectV1 { idle_address } => {
                ("ws-tose-direct-v1", idle_address, 0x1e21, 0x34, false)
            }
            WsToseTiming::VolumeV1 { idle_address } => {
                ("ws-tose-volume-v1", idle_address, 0x2000, 0x2a, false)
            }
            WsToseTiming::ScaledV1 { idle_address } => {
                ("ws-tose-scaled-v1", idle_address, 0xc00, 0x2e, false)
            }
            WsToseTiming::ScaledV2 { idle_address } => {
                ("ws-tose-scaled-v2", idle_address, 0xc00, 0x2e, false)
            }
        };
        self.finish_boot(cancel)?;
        let mut seen = HashMap::new();
        let mut frames = 0;
        let mut loop_safe = detect_loop;
        let mut reason = "duration_limit";
        let mut loop_start = None;
        let mut analysed = 0;
        for video_frame in 1..=MAX_ANALYSIS_FRAMES {
            self.step(cancel)?;
            ensure!(
                self.floats.len().is_multiple_of(2)
                    && self.floats.iter().all(|sample| sample.is_finite()),
                "WonderSwan core returned invalid stereo audio"
            );
            frames += self.floats.len() / 2;
            analysed = video_frame;
            if frames >= self.duration {
                break;
            }
            if self.emulator.cpu_state() != CpuState::Halted
                || self.emulator.cpu_pc() != idle_address
            {
                loop_safe = false;
                continue;
            }
            if (0..8).all(|slot| self.emulator.cpu_peek16(slots + slot * stride) == 0xffff)
                && self.emulator.io_peek8(0x90) & 15 == 0
            {
                self.duration = frames;
                reason = "driver_end";
                break;
            }
            loop_safe &= self.emulator.io_peek8(0x8c) == 0
                && self.emulator.io_peek8(0x90) & 0x20 == 0
                && self.emulator.io_peek8(0x95) & 2 == 0;
            if loop_safe {
                let state = control_state(&self.emulator);
                if let Some(&start) = seen.get(&state) {
                    self.duration = frames;
                    loop_start = Some(start);
                    reason = "driver_loop";
                    break;
                }
                seen.insert(state, frames);
            } else {
                seen.clear();
            }
        }
        self.timing = Some(json!({
            "profile": name,
            "stop_reason": reason,
            "frames": self.duration,
            "sample_rate": self.options.sample_rate,
            "loop_start_frame": loop_start,
            "analysed_video_frames": analysed,
            "analysis_frame_limit": MAX_ANALYSIS_FRAMES,
        }));
        self.warnings.push(match reason {
            "driver_end" => "Stops when all driver channels have ended.".into(),
            "driver_loop" => "Stops after the driver playback state repeats. This is a sequencer loop, not a bit-exact PCM loop.".into(),
            _ if detect_loop => "No ending or qualified loop was found within the bounded timing analysis; stops at the requested duration.".into(),
            _ => "No driver ending was found within the bounded timing analysis; stops at the requested duration.".into(),
        });
        self.warnings.push(
            "Native channels are mixed together; the fade is included in the rendered duration."
                .into(),
        );
        self.reset()
    }
}

fn control_state(emulator: &Emulator) -> Vec<u8> {
    let mut state: Vec<_> = (DRIVER_START..DRIVER_END)
        .map(|address| emulator.cpu_peek8(address))
        .collect();
    // This driver masks its vibrato phase to four bits at its only read.
    state[4] &= 15;
    state.extend((0xc0..0x100).map(|address| emulator.cpu_peek8(address)));
    state.extend(
        (0x80..=0x95)
            .filter(|port| ![0x92, 0x93].contains(port))
            .map(|port| emulator.io_peek8(port)),
    );
    state.extend((0xc0..=0xc3).map(|port| emulator.io_peek8(port)));
    state
}
