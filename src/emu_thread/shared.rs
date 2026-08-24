use crate::emu_backend::{BackendRuntimeConfig, EmuBackend};
use crate::ui;

use super::types::{publish_framebuffer, publish_owned_framebuffer};
use super::{
    EmuResponse, EmuThread, FrameInput, FrameResult, ReplayJoypadFrame, SharedFramebuffer,
    WorkerRuntimeFault,
};
use crate::audio_tooling::AudioSemanticFrame;
#[cfg(not(target_arch = "wasm32"))]
use crate::link::RemoteLink;
#[cfg(not(target_arch = "wasm32"))]
use crate::link::transport::TcpLinkTransport;

impl EmuThread {
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn handle_step_frames(
        backend: &mut EmuBackend,
        mut input: FrameInput,
        cheats: &[crate::cheats::CheatPatch],
        uncapped_mode: bool,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        rewind_seconds: &mut usize,
        shared_fb: &SharedFramebuffer,
        runtime_fault: &mut WorkerRuntimeFault,
    ) -> FrameResult {
        runtime_fault.latch(backend.take_runtime_fault());
        if !runtime_fault.can_step() {
            return Self::build_inert_frame_result(
                backend,
                input,
                rewind_buffer.fill_ratio(),
                shared_fb,
                runtime_fault,
            );
        }
        Self::configure_system(backend, &input, uncapped_mode);

        backend.set_pce_mouse_state(
            input.pce_mouse.mode,
            input.pce_mouse.delta_x,
            input.pce_mouse.delta_y,
            input.pce_mouse.buttons,
        );
        backend.set_pce_memory_base_mode(input.pce_mouse.memory_base_mode);
        backend.set_input(input.joypad.buttons, input.joypad.dpad);
        backend.set_input_p2(input.joypad.buttons_p2, input.joypad.dpad_p2);
        backend.set_zapper_state(
            input.zapper.enabled,
            input.zapper.trigger,
            input.zapper.hit,
            input.zapper.screen_pos,
        );

        if let Some(mutes) = &input.debug_actions.apu_channel_mutes {
            backend.set_apu_channel_mutes(mutes);
        }

        let semantic_capture_active = input.audio.recording_capture.semantic;
        let mut audio_semantic_frames = Vec::new();
        let mut advanced_frames = 0;
        let stepped_frames = input.frames > 0 && backend.is_running();
        if stepped_frames {
            advanced_frames = Self::step_n_frames_with_runtime_fault(
                backend,
                input.frames,
                cheats,
                semantic_capture_active,
                &mut audio_semantic_frames,
                input.replay_joypad_frames.as_deref(),
                runtime_fault,
            );
        }
        if runtime_fault.can_step() {
            Self::suspend_after_debug_frame(
                backend,
                input.debug_suspend_after_frame,
                advanced_frames,
            );
            if input.rewind_seconds != *rewind_seconds {
                *rewind_seconds = input.rewind_seconds;
                *rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(
                    *rewind_seconds,
                    super::REWIND_SNAPSHOTS_PER_SECOND,
                );
            }
            Self::capture_rewind_snapshot(backend, rewind_buffer, input.rewind_enabled);
        }

        let reusable_audio = input.buffers.audio.take();
        let ui_data = Self::collect_ui_snapshot(backend, &input.snapshot, input.buffers);

        publish_framebuffer(shared_fb, backend.framebuffer());

        Self::build_frame_result(
            backend,
            runtime_fault,
            reusable_audio,
            ui_data,
            audio_semantic_frames,
            rewind_buffer.fill_ratio(),
            advanced_frames,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn handle_step_frames_with_game_boy_replay_link(
        backend: &mut EmuBackend,
        mut input: FrameInput,
        cheats: &[crate::cheats::CheatPatch],
        replay_link: &mut crate::link::gb::GameBoyReplayLink,
        uncapped_mode: bool,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        rewind_seconds: &mut usize,
        shared_fb: &SharedFramebuffer,
        runtime_fault: &mut WorkerRuntimeFault,
    ) -> FrameResult {
        runtime_fault.latch(backend.take_runtime_fault());
        if !runtime_fault.can_step() {
            return Self::build_inert_frame_result(
                backend,
                input,
                rewind_buffer.fill_ratio(),
                shared_fb,
                runtime_fault,
            );
        }
        Self::configure_system(backend, &input, uncapped_mode);

        backend.set_pce_mouse_state(
            input.pce_mouse.mode,
            input.pce_mouse.delta_x,
            input.pce_mouse.delta_y,
            input.pce_mouse.buttons,
        );
        backend.set_pce_memory_base_mode(input.pce_mouse.memory_base_mode);
        backend.set_input(input.joypad.buttons, input.joypad.dpad);
        backend.set_input_p2(input.joypad.buttons_p2, input.joypad.dpad_p2);
        backend.set_zapper_state(
            input.zapper.enabled,
            input.zapper.trigger,
            input.zapper.hit,
            input.zapper.screen_pos,
        );

        if let Some(mutes) = &input.debug_actions.apu_channel_mutes {
            backend.set_apu_channel_mutes(mutes);
        }

        let semantic_capture_active = input.audio.recording_capture.semantic;
        let mut audio_semantic_frames = Vec::new();
        let mut advanced_frames = 0;
        let mut replay_error = None;
        let stepped_frames = input.frames > 0 && backend.is_running();
        if stepped_frames {
            let result = Self::step_n_frames_with_game_boy_replay_link(
                backend,
                input.frames,
                cheats,
                replay_link,
                semantic_capture_active,
                &mut audio_semantic_frames,
                input.replay_joypad_frames.as_deref(),
                runtime_fault,
            );
            advanced_frames = result.0;
            replay_error = result.1;
        }
        if runtime_fault.can_step() {
            Self::suspend_after_debug_frame(
                backend,
                input.debug_suspend_after_frame,
                advanced_frames,
            );
            if input.rewind_seconds != *rewind_seconds {
                *rewind_seconds = input.rewind_seconds;
                *rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(
                    *rewind_seconds,
                    super::REWIND_SNAPSHOTS_PER_SECOND,
                );
            }
            Self::capture_rewind_snapshot(backend, rewind_buffer, input.rewind_enabled);
        }

        let reusable_audio = input.buffers.audio.take();
        let ui_data = Self::collect_ui_snapshot(backend, &input.snapshot, input.buffers);

        publish_framebuffer(shared_fb, backend.framebuffer());

        let mut result = Self::build_frame_result(
            backend,
            runtime_fault,
            reusable_audio,
            ui_data,
            audio_semantic_frames,
            rewind_buffer.fill_ratio(),
            advanced_frames,
        );
        result.replay_error = replay_error;
        result
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn handle_step_frames_with_wonder_swan_replay_link(
        backend: &mut EmuBackend,
        mut input: FrameInput,
        cheats: &[crate::cheats::CheatPatch],
        replay_link: &mut crate::link::ws_replay::WonderSwanReplayLink,
        uncapped_mode: bool,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        rewind_seconds: &mut usize,
        shared_fb: &SharedFramebuffer,
        runtime_fault: &mut WorkerRuntimeFault,
    ) -> FrameResult {
        runtime_fault.latch(backend.take_runtime_fault());
        if !runtime_fault.can_step() {
            return Self::build_inert_frame_result(
                backend,
                input,
                rewind_buffer.fill_ratio(),
                shared_fb,
                runtime_fault,
            );
        }
        Self::configure_system(backend, &input, uncapped_mode);

        backend.set_pce_mouse_state(
            input.pce_mouse.mode,
            input.pce_mouse.delta_x,
            input.pce_mouse.delta_y,
            input.pce_mouse.buttons,
        );
        backend.set_pce_memory_base_mode(input.pce_mouse.memory_base_mode);
        backend.set_input(input.joypad.buttons, input.joypad.dpad);
        backend.set_input_p2(input.joypad.buttons_p2, input.joypad.dpad_p2);
        backend.set_zapper_state(
            input.zapper.enabled,
            input.zapper.trigger,
            input.zapper.hit,
            input.zapper.screen_pos,
        );

        if let Some(mutes) = &input.debug_actions.apu_channel_mutes {
            backend.set_apu_channel_mutes(mutes);
        }

        let semantic_capture_active = input.audio.recording_capture.semantic;
        let mut audio_semantic_frames = Vec::new();
        let mut advanced_frames = 0;
        let mut replay_error = None;
        let stepped_frames = input.frames > 0 && backend.is_running();
        if stepped_frames {
            let result = Self::step_n_frames_with_wonder_swan_replay_link(
                backend,
                input.frames,
                cheats,
                replay_link,
                semantic_capture_active,
                &mut audio_semantic_frames,
                input.replay_joypad_frames.as_deref(),
                runtime_fault,
            );
            advanced_frames = result.0;
            replay_error = result.1;
        }
        if runtime_fault.can_step() {
            Self::suspend_after_debug_frame(
                backend,
                input.debug_suspend_after_frame,
                advanced_frames,
            );
            if input.rewind_seconds != *rewind_seconds {
                *rewind_seconds = input.rewind_seconds;
                *rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(
                    *rewind_seconds,
                    super::REWIND_SNAPSHOTS_PER_SECOND,
                );
            }
            Self::capture_rewind_snapshot(backend, rewind_buffer, input.rewind_enabled);
        }

        let reusable_audio = input.buffers.audio.take();
        let ui_data = Self::collect_ui_snapshot(backend, &input.snapshot, input.buffers);

        publish_framebuffer(shared_fb, backend.framebuffer());

        let mut result = Self::build_frame_result(
            backend,
            runtime_fault,
            reusable_audio,
            ui_data,
            audio_semantic_frames,
            rewind_buffer.fill_ratio(),
            advanced_frames,
        );
        result.replay_error = replay_error;
        result
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn handle_step_frames_with_tcp_link(
        backend: &mut EmuBackend,
        mut input: FrameInput,
        cheats: &[crate::cheats::CheatPatch],
        mut tcp_link: Option<&mut RemoteLink<TcpLinkTransport>>,
        uncapped_mode: bool,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        rewind_seconds: &mut usize,
        shared_fb: &SharedFramebuffer,
        runtime_fault: &mut WorkerRuntimeFault,
    ) -> FrameResult {
        runtime_fault.latch(backend.take_runtime_fault());
        if !runtime_fault.can_step() {
            return Self::build_inert_frame_result(
                backend,
                input,
                rewind_buffer.fill_ratio(),
                shared_fb,
                runtime_fault,
            );
        }
        Self::configure_system(backend, &input, uncapped_mode);

        backend.set_pce_mouse_state(
            input.pce_mouse.mode,
            input.pce_mouse.delta_x,
            input.pce_mouse.delta_y,
            input.pce_mouse.buttons,
        );
        backend.set_pce_memory_base_mode(input.pce_mouse.memory_base_mode);
        backend.set_input(input.joypad.buttons, input.joypad.dpad);
        backend.set_input_p2(input.joypad.buttons_p2, input.joypad.dpad_p2);
        backend.set_zapper_state(
            input.zapper.enabled,
            input.zapper.trigger,
            input.zapper.hit,
            input.zapper.screen_pos,
        );

        if let Some(mutes) = &input.debug_actions.apu_channel_mutes {
            backend.set_apu_channel_mutes(mutes);
        }

        let semantic_capture_active = input.audio.recording_capture.semantic;
        let mut audio_semantic_frames = Vec::new();
        let mut advanced_frames = 0;
        let stepped_frames = input.frames > 0 && backend.is_running();
        if stepped_frames {
            advanced_frames = Self::step_n_frames_with_tcp_link_and_runtime_fault(
                backend,
                input.frames,
                cheats,
                tcp_link.as_deref_mut(),
                semantic_capture_active,
                &mut audio_semantic_frames,
                input.replay_joypad_frames.as_deref(),
                runtime_fault,
            );
        }
        if runtime_fault.can_step() {
            Self::suspend_after_debug_frame(
                backend,
                input.debug_suspend_after_frame,
                advanced_frames,
            );
        }
        let replay_events = tcp_link
            .as_mut()
            .map(|link| link.take_replay_events())
            .unwrap_or_default();
        if runtime_fault.can_step() {
            if input.rewind_seconds != *rewind_seconds {
                *rewind_seconds = input.rewind_seconds;
                *rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(
                    *rewind_seconds,
                    super::REWIND_SNAPSHOTS_PER_SECOND,
                );
            }
            Self::capture_rewind_snapshot(backend, rewind_buffer, input.rewind_enabled);
        }

        let reusable_audio = input.buffers.audio.take();
        let ui_data = Self::collect_ui_snapshot(backend, &input.snapshot, input.buffers);

        publish_framebuffer(shared_fb, backend.framebuffer());

        let mut result = Self::build_frame_result(
            backend,
            runtime_fault,
            reusable_audio,
            ui_data,
            audio_semantic_frames,
            rewind_buffer.fill_ratio(),
            advanced_frames,
        );
        result.replay_events = replay_events;
        result
    }

    fn build_inert_frame_result(
        backend: &mut EmuBackend,
        mut input: FrameInput,
        rewind_fill: f32,
        shared_fb: &SharedFramebuffer,
        runtime_fault: &mut WorkerRuntimeFault,
    ) -> FrameResult {
        let reusable_audio = input.buffers.audio.take();
        let ui_data = Self::collect_ui_snapshot(backend, &input.snapshot, input.buffers);
        publish_framebuffer(shared_fb, backend.framebuffer());
        Self::build_frame_result(
            backend,
            runtime_fault,
            reusable_audio,
            ui_data,
            Vec::new(),
            rewind_fill,
            0,
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn encode_and_write_state(
        backend: &EmuBackend,
        path: &std::path::Path,
    ) -> EmuResponse {
        match backend.encode_state_bytes() {
            Ok(bytes) => {
                match crate::save_paths::write_state_bytes_to_file_with_backup(path, &bytes) {
                    Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                    Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
                }
            }
            Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn finalize_load_state(
        resp: &EmuResponse,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        backend: &mut EmuBackend,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        if matches!(resp, EmuResponse::LoadStateOk { .. }) {
            backend.discard_game_boy_printer_jobs();
            rewind_buffer.clear();
            backend.install_rom_patches(cheats);
        }
    }

    pub(crate) fn respond_load_state(
        backend: &mut EmuBackend,
        result: anyhow::Result<()>,
        path_label: String,
        buttons_pressed: u8,
        dpad_pressed: u8,
        shared_fb: &SharedFramebuffer,
    ) -> EmuResponse {
        match result {
            Ok(()) => {
                backend.set_input(buttons_pressed, dpad_pressed);
                publish_framebuffer(shared_fb, backend.framebuffer());
                EmuResponse::LoadStateOk {
                    path: path_label,
                    media_slot_snapshot: backend.media_slot_snapshot(),
                    game_boy_serial_device: backend.game_boy_serial_device(),
                }
            }
            Err(err) => EmuResponse::LoadStateFailed(err.to_string()),
        }
    }

    pub(crate) fn handle_rewind(
        backend: &mut EmuBackend,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        shared_fb: &SharedFramebuffer,
    ) -> EmuResponse {
        if !backend.supports_rewind() {
            return EmuResponse::RewindFailed("rewind is not supported by this core".to_string());
        }
        let current_state = Self::encode_current_state(backend).ok();
        while let Some(rewind_frame) = rewind_buffer.pop() {
            if let Some(current) = current_state.as_ref()
                && rewind_frame.state_bytes == *current
                && !rewind_buffer.is_empty()
            {
                continue;
            }
            match backend.load_state_from_bytes(rewind_frame.state_bytes) {
                Ok(()) => {
                    let fb = if rewind_frame.framebuffer.is_empty() {
                        backend.framebuffer().to_vec()
                    } else {
                        rewind_frame.framebuffer
                    };
                    publish_owned_framebuffer(shared_fb, fb);
                    return EmuResponse::RewindOk {
                        media_slot_snapshot: backend.media_slot_snapshot(),
                        game_boy_serial_device: backend.game_boy_serial_device(),
                    };
                }
                Err(err) => {
                    log::warn!("Rewind restore failed: {}", err);
                    return EmuResponse::RewindFailed("rewind restore failed".to_string());
                }
            }
        }

        EmuResponse::RewindFailed("no rewind data".to_string())
    }

    pub(crate) fn encode_current_state(backend: &EmuBackend) -> anyhow::Result<Vec<u8>> {
        backend.encode_state_bytes()
    }

    pub(crate) fn capture_rewind_snapshot(
        backend: &EmuBackend,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        enabled: bool,
    ) {
        if enabled
            && backend.supports_rewind()
            && rewind_buffer.tick()
            && let Ok(bytes) = Self::encode_current_state(backend)
        {
            rewind_buffer.push(&bytes, backend.framebuffer());
        }
    }

    pub(crate) fn configure_system(
        backend: &mut EmuBackend,
        input: &FrameInput,
        uncapped_mode: bool,
    ) {
        let runtime_config = BackendRuntimeConfig {
            opcode_log_enabled: input.snapshot.want_debug_info,
            debug_continue: input.debug_continue,
            debug_step: input.debug_step,
            uncapped_mode,
            apu_capture_enabled: input.audio.apu_capture_enabled,
            skip_audio: input.audio.skip_audio,
            host_tilt: input.host_tilt,
            host_camera_frame: input.host_camera_frame.as_deref(),
            dmg_palette_preset: input.snapshot.render.dmg_palette_preset,
            sgb_border_enabled: input.snapshot.render.sgb_border_enabled,
            nes_palette_mode: input.snapshot.render.nes_palette_mode,
            nes_custom_palette: input.snapshot.render.nes_custom_palette.as_ref(),
            pce_overscan_mode: input.snapshot.render.pce_overscan_mode,
            pce_palette_mode: input.snapshot.render.pce_palette_mode,
            ..BackendRuntimeConfig::new(&input.debug_actions)
        };
        backend.apply_runtime_config(runtime_config);
    }

    fn suspend_after_debug_frame(
        backend: &mut EmuBackend,
        requested: bool,
        advanced_frames: usize,
    ) {
        if requested && advanced_frames > 0 {
            backend.debug_suspend();
        }
    }

    #[cfg(test)]
    pub(crate) fn step_n_frames(
        backend: &mut EmuBackend,
        n: usize,
        cheats: &[crate::cheats::CheatPatch],
        semantic_capture_active: bool,
        audio_semantic_frames: &mut Vec<AudioSemanticFrame>,
        replay_joypad_frames: Option<&[ReplayJoypadFrame]>,
    ) -> usize {
        let mut runtime_fault = WorkerRuntimeFault::default();
        Self::step_n_frames_with_runtime_fault(
            backend,
            n,
            cheats,
            semantic_capture_active,
            audio_semantic_frames,
            replay_joypad_frames,
            &mut runtime_fault,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step_n_frames_with_runtime_fault(
        backend: &mut EmuBackend,
        n: usize,
        cheats: &[crate::cheats::CheatPatch],
        semantic_capture_active: bool,
        audio_semantic_frames: &mut Vec<AudioSemanticFrame>,
        replay_joypad_frames: Option<&[ReplayJoypadFrame]>,
        runtime_fault: &mut WorkerRuntimeFault,
    ) -> usize {
        let mut advanced_frames = 0;
        let mut replay_frame_index = 0;
        for _ in 0..n {
            if let Some(frame) =
                replay_joypad_frames.and_then(|frames| frames.get(replay_frame_index))
            {
                backend.apply_replay_input(frame);
            }

            let frame_count_before = backend.frame_count();
            backend.step_frame();
            if runtime_fault.latch(backend.take_runtime_fault()) {
                break;
            }
            backend.apply_ram_cheats(cheats);
            Self::collect_semantic_snapshot_if_frame_advanced(
                backend,
                semantic_capture_active.then_some(frame_count_before),
                audio_semantic_frames,
            );
            if backend.frame_count() != frame_count_before {
                advanced_frames += 1;
                replay_frame_index += 1;
            } else {
                break;
            }
            if backend.is_suspended() {
                break;
            }
        }
        advanced_frames
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step_n_frames_with_game_boy_replay_link(
        backend: &mut EmuBackend,
        n: usize,
        cheats: &[crate::cheats::CheatPatch],
        replay_link: &mut crate::link::gb::GameBoyReplayLink,
        semantic_capture_active: bool,
        audio_semantic_frames: &mut Vec<AudioSemanticFrame>,
        replay_joypad_frames: Option<&[ReplayJoypadFrame]>,
        runtime_fault: &mut WorkerRuntimeFault,
    ) -> (usize, Option<String>) {
        let mut advanced_frames = 0;
        let mut replay_frame_index = 0;
        for _ in 0..n {
            if let Some(frame) =
                replay_joypad_frames.and_then(|frames| frames.get(replay_frame_index))
            {
                backend.apply_replay_input(frame);
            }

            let frame_count_before = backend.frame_count();
            if let Err(err) = backend.step_game_boy_frame_with_replay_link(replay_link) {
                let summary = replay_link.debug_summary();
                return (
                    advanced_frames,
                    Some(format!("GB link replay event failed: {err:?}; {summary}")),
                );
            }
            if runtime_fault.latch(backend.take_runtime_fault()) {
                break;
            }
            backend.apply_ram_cheats(cheats);
            Self::collect_semantic_snapshot_if_frame_advanced(
                backend,
                semantic_capture_active.then_some(frame_count_before),
                audio_semantic_frames,
            );
            if backend.frame_count() != frame_count_before {
                advanced_frames += 1;
                replay_frame_index += 1;
            } else {
                break;
            }
            if backend.is_suspended() {
                break;
            }
        }
        (advanced_frames, None)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step_n_frames_with_wonder_swan_replay_link(
        backend: &mut EmuBackend,
        n: usize,
        cheats: &[crate::cheats::CheatPatch],
        replay_link: &mut crate::link::ws_replay::WonderSwanReplayLink,
        semantic_capture_active: bool,
        audio_semantic_frames: &mut Vec<AudioSemanticFrame>,
        replay_joypad_frames: Option<&[ReplayJoypadFrame]>,
        runtime_fault: &mut WorkerRuntimeFault,
    ) -> (usize, Option<String>) {
        let mut advanced_frames = 0;
        let mut replay_frame_index = 0;
        for _ in 0..n {
            if let Some(frame) =
                replay_joypad_frames.and_then(|frames| frames.get(replay_frame_index))
            {
                backend.apply_replay_input(frame);
            }

            let frame_count_before = backend.frame_count();
            if let Err(err) = backend.step_wonder_swan_frame_with_replay_link(replay_link) {
                return (
                    advanced_frames,
                    Some(format!(
                        "WonderSwan link replay event failed: {err:?}; {}",
                        replay_link.debug_summary()
                    )),
                );
            }
            if runtime_fault.latch(backend.take_runtime_fault()) {
                break;
            }
            backend.apply_ram_cheats(cheats);
            Self::collect_semantic_snapshot_if_frame_advanced(
                backend,
                semantic_capture_active.then_some(frame_count_before),
                audio_semantic_frames,
            );
            if backend.frame_count() != frame_count_before {
                advanced_frames += 1;
                replay_frame_index += 1;
            } else {
                break;
            }
            if backend.is_suspended() {
                break;
            }
        }
        (advanced_frames, None)
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step_n_frames_with_tcp_link(
        backend: &mut EmuBackend,
        n: usize,
        cheats: &[crate::cheats::CheatPatch],
        tcp_link: Option<&mut RemoteLink<TcpLinkTransport>>,
        semantic_capture_active: bool,
        audio_semantic_frames: &mut Vec<AudioSemanticFrame>,
        replay_joypad_frames: Option<&[ReplayJoypadFrame]>,
    ) -> usize {
        let mut runtime_fault = WorkerRuntimeFault::default();
        Self::step_n_frames_with_tcp_link_and_runtime_fault(
            backend,
            n,
            cheats,
            tcp_link,
            semantic_capture_active,
            audio_semantic_frames,
            replay_joypad_frames,
            &mut runtime_fault,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_n_frames_with_tcp_link_and_runtime_fault(
        backend: &mut EmuBackend,
        n: usize,
        cheats: &[crate::cheats::CheatPatch],
        mut tcp_link: Option<&mut RemoteLink<TcpLinkTransport>>,
        semantic_capture_active: bool,
        audio_semantic_frames: &mut Vec<AudioSemanticFrame>,
        replay_joypad_frames: Option<&[ReplayJoypadFrame]>,
        runtime_fault: &mut WorkerRuntimeFault,
    ) -> usize {
        let mut advanced_frames = 0;
        let mut replay_frame_index = 0;
        'frames: for _ in 0..n {
            if let Some(frame) =
                replay_joypad_frames.and_then(|frames| frames.get(replay_frame_index))
            {
                backend.apply_replay_input(frame);
            }

            let frame_count_before = backend.frame_count();
            let retry_deadline = std::time::Instant::now() + std::time::Duration::from_millis(20);
            loop {
                if let Some(link) = tcp_link.as_deref_mut() {
                    if backend.step_frame_with_remote_link(link).is_err() {
                        link.disconnect();
                        backend.set_link_peer_present(false);
                        backend.step_frame();
                    }
                } else {
                    backend.step_frame();
                }

                if runtime_fault.latch(backend.take_runtime_fault()) {
                    break 'frames;
                }

                if backend.frame_count() != frame_count_before
                    || !matches!(
                        backend,
                        EmuBackend::Gb(gb)
                            if gb.emu.game_boy_link_pending_master_response()
                    )
                    || std::time::Instant::now() >= retry_deadline
                {
                    break;
                }
                std::thread::yield_now();
            }
            backend.apply_ram_cheats(cheats);
            Self::collect_semantic_snapshot_if_frame_advanced(
                backend,
                semantic_capture_active.then_some(frame_count_before),
                audio_semantic_frames,
            );
            if backend.frame_count() != frame_count_before {
                advanced_frames += 1;
                replay_frame_index += 1;
            } else {
                break;
            }

            if backend.is_suspended() {
                break;
            }
        }
        advanced_frames
    }

    pub(crate) fn collect_ui_snapshot(
        backend: &mut EmuBackend,
        snapshot: &super::SnapshotRequest,
        buffers: super::ReusableBuffers,
    ) -> ui::UiFrameData {
        ui::collect_backend_snapshot(backend, snapshot, buffers)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_frame_result(
        backend: &mut EmuBackend,
        runtime_fault: &mut WorkerRuntimeFault,
        reusable_audio: Option<Vec<f32>>,
        ui_data: ui::UiFrameData,
        audio_semantic_frames: Vec<AudioSemanticFrame>,
        rewind_fill: f32,
        advanced_frames: usize,
    ) -> FrameResult {
        let rumble = backend.rumble_active();
        let mut audio_samples = reusable_audio.unwrap_or_default();
        audio_samples.clear();
        backend.drain_audio_samples_into(&mut audio_samples);
        let is_mbc7 = backend.is_mbc7();
        let is_pocket_camera = backend.is_pocket_camera();
        let game_boy_serial_device = backend.game_boy_serial_device();
        let game_boy_printer_jobs = Self::take_game_boy_printer_jobs(backend);
        let media_slot_snapshot = backend.media_slot_snapshot();
        runtime_fault.latch(backend.take_runtime_fault());
        let runtime_fault = runtime_fault.take_pending_delivery();

        FrameResult {
            advanced_frames,
            delivery_merged: false,
            replay_events: Vec::new(),
            replay_error: None,
            runtime_fault,
            rumble,
            audio_samples,
            ui_data,
            is_mbc7,
            is_pocket_camera,
            game_boy_serial_device,
            game_boy_printer_jobs,
            media_slot_snapshot,
            rewind_fill,
            audio_semantic_frames,
            audio_timeline_discontinuities: Vec::new(),
        }
    }

    pub(crate) fn take_game_boy_printer_jobs(
        backend: &mut EmuBackend,
    ) -> Vec<zeff_gb_core::hardware::GameBoyPrinterJob> {
        backend
            .take_game_boy_printer_jobs()
            .into_iter()
            .filter(|job| job.validate().is_ok())
            .collect()
    }

    fn collect_semantic_snapshot_if_frame_advanced(
        backend: &EmuBackend,
        frame_count_before: Option<u64>,
        audio_semantic_frames: &mut Vec<AudioSemanticFrame>,
    ) {
        let Some(frame_count_before) = frame_count_before else {
            return;
        };
        if backend.frame_count() == frame_count_before {
            return;
        }
        if let Some(frame) = backend.audio_semantic_frame() {
            audio_semantic_frames.push(frame);
        }
    }
}

#[cfg(test)]
mod tests;
