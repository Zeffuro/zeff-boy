use super::App;
use crate::emu_thread::{
    AudioConfig, EmuCommand, FrameInput, JoypadInput, MdTimedPortReply, MdTimedPortRequest,
    SpeculationBlockers,
};

impl App {
    pub(super) fn tick_md_timed_ports(&mut self) -> bool {
        if !self.debug_windows.md_timed_ports.busy() {
            if !self.debug_windows.md_timed_ports.ordering_pending() {
                return false;
            }
            self.drain_emu_responses();
            if self.emu_thread.is_none() {
                self.debug_windows
                    .md_timed_ports
                    .cancel_session("Emulator worker is unavailable");
                return false;
            }
            if self
                .debug_windows
                .md_timed_ports
                .ordinary_delivery_pending()
                || self.frames_in_flight != 0
            {
                return true;
            }
            if self.debug_windows.md_timed_ports.ordinary_flush_required() {
                return false;
            }
            if let Some(actions) = self.debug_windows.md_timed_ports.take_deferred() {
                self.merge_deferred_debug_actions(actions);
            }
            return false;
        }
        self.drain_emu_responses();
        if self.emu_thread.is_none() {
            self.debug_windows
                .md_timed_ports
                .cancel_session("Emulator worker is unavailable");
            return true;
        }
        if self.frames_in_flight != 0 {
            return true;
        }
        if let Some(request) = self.debug_windows.md_timed_ports.queued() {
            let id = request.id;
            let requirements = crate::debug::dock::TabDataRequirements::default();
            let mut snapshot = self.build_snapshot_request(&requirements, true);
            snapshot.want_debug_info = true;
            snapshot.show_instruction_trace = true;
            let buffers = self.take_reusable_buffers();
            let input = isolated_input(request, snapshot, buffers);
            match self.send_emu_command_checked(EmuCommand::StepFrames(Box::new(input))) {
                Ok(()) => {
                    self.debug_windows.md_timed_ports.sent(id);
                    self.frames_in_flight += 1;
                }
                Err(error) => self
                    .debug_windows
                    .md_timed_ports
                    .send_failed(error.to_string()),
            }
        }
        true
    }

    pub(in crate::app) fn consume_md_timed_ports(&mut self, reply: MdTimedPortReply) {
        self.debug_windows.md_timed_ports.receive(reply);
    }
}

pub(crate) fn isolated_input(
    request: MdTimedPortRequest,
    snapshot: crate::emu_thread::SnapshotRequest,
    buffers: crate::emu_thread::ReusableBuffers,
) -> FrameInput {
    let mut actions = crate::debug::DebugUiActions::none();
    actions.md_timed_ports = Some(request);
    FrameInput {
        #[cfg(all(test, not(target_arch = "wasm32")))]
        audio_timing_request: 0,
        #[cfg(all(test, not(target_arch = "wasm32")))]
        audio_timing_pacing: (0, 0),
        frames: 0,
        speculation_blockers: SpeculationBlockers::feature_disabled(),
        replay_joypad_frames: None,
        host_tilt: (0.0, 0.0),
        host_camera_frame: None,
        joypad: JoypadInput {
            buttons: 0,
            dpad: 0,
            buttons_p2: 0,
            dpad_p2: 0,
            buttons_p3: 0,
            dpad_p3: 0,
            buttons_p4: 0,
            dpad_p4: 0,
            buttons_p5: 0,
            dpad_p5: 0,
        },
        pce_mouse: Default::default(),
        zapper: Default::default(),
        debug_step: false,
        debug_continue: false,
        debug_suspend_after_frame: false,
        audio: AudioConfig {
            apu_capture_enabled: false,
            skip_audio: false,
            playback_speed: 1,
            recording_capture: Default::default(),
        },
        debug_actions: actions,
        snapshot,
        buffers,
        rewind_enabled: false,
        rewind_seconds: 0,
    }
}
