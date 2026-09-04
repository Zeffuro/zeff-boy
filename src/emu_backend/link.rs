use super::EmuBackend;

#[cfg(not(target_arch = "wasm32"))]
const WONDER_SWAN_REMOTE_LINK_WAIT_SPINS: usize = 2048;
#[cfg(not(target_arch = "wasm32"))]
const WONDER_SWAN_REMOTE_LINK_POLL_INTERVAL_CYCLES: u64 = 800;

impl EmuBackend {
    pub(crate) fn game_boy_cpu_cycles(&self) -> Option<u64> {
        match self {
            Self::Gb(gb) => Some(gb.emu.cpu_cycles()),
            _ => None,
        }
    }

    pub(crate) fn probe_replay_state_load(
        &self,
        bytes: &[u8],
        game_boy_link_start_state: Option<zeff_emu_common::replay::ReplayGameBoyLinkState>,
        has_game_boy_link: bool,
        has_wonder_swan_link: bool,
    ) -> anyhow::Result<(u64, Option<u64>, Option<u64>)> {
        match self {
            Self::Gb(gb) => {
                anyhow::ensure!(
                    !has_wonder_swan_link,
                    "replay contains WonderSwan link data for a Game Boy state"
                );
                let (frame_count, cpu_cycles) = gb
                    .emu
                    .probe_native_state_for_replay(bytes, game_boy_link_start_state)?;
                Ok((frame_count, Some(cpu_cycles), None))
            }
            Self::Ws(ws) => {
                anyhow::ensure!(
                    !has_game_boy_link,
                    "replay contains Game Boy link data for a WonderSwan state"
                );
                let mut candidate = ws.emu.clone();
                candidate.load_state(bytes)?;
                Ok((candidate.frame_count(), None, Some(candidate.cpu_cycles())))
            }
            _ => {
                anyhow::ensure!(
                    !has_game_boy_link && !has_wonder_swan_link,
                    "replay link data does not match the current system"
                );
                Ok((self.frame_count(), None, None))
            }
        }
    }

    pub(crate) fn wonder_swan_cpu_cycles(&self) -> Option<u64> {
        match self {
            Self::Ws(ws) => Some(ws.emu.cpu_cycles()),
            _ => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn begin_game_boy_frame_slice(
        &self,
    ) -> Result<zeff_gb_core::emulator::FrameSliceCursor, crate::link::LinkSessionError> {
        let Self::Gb(gb) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };
        Ok(gb.emu.begin_frame_slice())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_game_boy_frame_slice_until(
        &mut self,
        cursor: &mut zeff_gb_core::emulator::FrameSliceCursor,
        target_tick: Option<u64>,
        stop_on_link_action: bool,
    ) -> Result<zeff_gb_core::emulator::FrameSliceProgress, crate::link::LinkSessionError> {
        let Self::Gb(gb) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };
        if !cursor.is_complete()
            && (target_tick.is_some_and(|tick| gb.emu.cpu_cycles() >= tick)
                || (stop_on_link_action
                    && gb
                        .emu
                        .game_boy_link_replay_state()
                        .queued_master_action
                        .is_some()))
        {
            return Ok(zeff_gb_core::emulator::FrameSliceProgress {
                outcome: zeff_gb_core::emulator::FrameSliceOutcome::Boundary,
                boundary_reached: true,
            });
        }
        Ok(gb.emu.step_frame_slice_until(cursor, |emulator| {
            target_tick.is_some_and(|tick| emulator.cpu_cycles() >= tick)
                || (stop_on_link_action
                    && emulator
                        .game_boy_link_replay_state()
                        .queued_master_action
                        .is_some())
        }))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_game_boy_frame_with_remote_link<T: crate::link::LinkTransport>(
        &mut self,
        link: &mut crate::link::gb::GameBoyRemoteLink<T>,
    ) -> Result<(), crate::link::LinkSessionError> {
        let mut cursor = self.begin_game_boy_frame_slice()?;
        let _ = self.step_game_boy_frame_slice_with_remote_link(link, &mut cursor, None)?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_game_boy_frame_slice_with_remote_link<T: crate::link::LinkTransport>(
        &mut self,
        link: &mut crate::link::gb::GameBoyRemoteLink<T>,
        cursor: &mut zeff_gb_core::emulator::FrameSliceCursor,
        activation_tick: Option<u64>,
    ) -> Result<zeff_gb_core::emulator::FrameSliceOutcome, crate::link::LinkSessionError> {
        let Self::Gb(gb) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        let link_is_active = activation_tick.is_none_or(|tick| gb.emu.cpu_cycles() >= tick);
        if link_is_active {
            link.poll_emulator(&mut gb.emu)?;
            if gb.emu.game_boy_link_pending_master_response() {
                link.trace_wait_pending_master(gb.emu.cpu_cycles(), "frame_start");
                return Ok(zeff_gb_core::emulator::FrameSliceOutcome::Boundary);
            }
        } else {
            gb.emu
                .restore_game_boy_link_peer_present_without_action(false);
        }

        let mut link_error = None;
        let outcome = gb.emu.step_frame_slice_or(cursor, |emulator| {
            if activation_tick.is_some_and(|tick| emulator.cpu_cycles() < tick) {
                return false;
            }
            if let Err(err) = link.poll_emulator(emulator) {
                link_error = Some(err);
                return true;
            }
            if emulator.game_boy_link_pending_master_response() {
                link.trace_wait_pending_master(emulator.cpu_cycles(), "mid_frame_activation");
                return true;
            }
            false
        });

        if let Some(err) = link_error {
            return Err(err);
        }
        link.poll_emulator(&mut gb.emu)?;
        Ok(outcome)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_game_boy_frame_with_replay_link(
        &mut self,
        link: &mut crate::link::gb::GameBoyReplayLink,
    ) -> Result<(), crate::link::LinkSessionError> {
        let Self::Gb(gb) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        link.poll_emulator(&mut gb.emu)?;
        if gb.emu.game_boy_link_waiting_at_completion_boundary() {
            link.trace_wait_boundary(gb.emu.cpu_cycles(), "frame_start");
            return Ok(());
        }

        let mut link_error = None;
        gb.emu.step_until_frame_or(|emulator| {
            if let Err(err) = link.poll_emulator(emulator) {
                link_error = Some(err);
                return true;
            }
            if emulator.game_boy_link_waiting_at_completion_boundary() {
                link.trace_wait_boundary(emulator.cpu_cycles(), "mid_frame");
                return true;
            }
            false
        });

        if let Some(err) = link_error {
            return Err(err);
        }
        link.poll_emulator(&mut gb.emu)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_wonder_swan_frame_with_remote_link(
        &mut self,
        link: &mut crate::link::ws::WonderSwanRemoteLink<crate::link::transport::TcpLinkTransport>,
    ) -> Result<(), crate::link::LinkSessionError> {
        let Self::Ws(ws) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        link.poll_emulator(&mut ws.emu)?;
        if ws.emu.is_cpu_suspended() {
            return Ok(());
        }
        if !wait_for_wonder_swan_remote_link_window(&mut ws.emu, link)? {
            return Ok(());
        }

        ws.emu.clear_frame_ready();
        let mut next_link_poll_cycle = ws
            .emu
            .cpu_cycles()
            .saturating_add(WONDER_SWAN_REMOTE_LINK_POLL_INTERVAL_CYCLES);
        let guard = ws
            .emu
            .cpu_cycles()
            .wrapping_add(u64::from(zeff_ws_core::hardware::constants::CYCLES_PER_FRAME) * 2);
        while !ws.emu.frame_ready() && ws.emu.cpu_cycles() < guard {
            if ws.emu.cpu_cycles() >= next_link_poll_cycle {
                link.poll_emulator(&mut ws.emu)?;
                if !wait_for_wonder_swan_remote_link_window(&mut ws.emu, link)? {
                    return Ok(());
                }
                next_link_poll_cycle = ws
                    .emu
                    .cpu_cycles()
                    .saturating_add(WONDER_SWAN_REMOTE_LINK_POLL_INTERVAL_CYCLES);
            }
            let fetched = if link.trace_enabled() {
                let (fetched, bus_events) = ws.emu.step_instruction_with_io_trace();
                link.trace_serial_io_events(
                    &ws.emu,
                    fetched.or_else(|| ws.emu.last_fetch()),
                    &bus_events,
                );
                fetched
            } else {
                ws.emu.step_instruction()
            };
            if fetched.is_none() && ws.emu.is_cpu_suspended() {
                break;
            }
        }
        ws.emu.finish_frame();
        link.poll_emulator(&mut ws.emu)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_wonder_swan_frame_with_replay_link(
        &mut self,
        link: &mut crate::link::ws_replay::WonderSwanReplayLink,
    ) -> Result<(), crate::link::LinkSessionError> {
        let Self::Ws(ws) = self else {
            return Err(crate::link::LinkSessionError::IncompatibleSystems);
        };

        link.poll_emulator(&mut ws.emu)?;
        if ws.emu.is_cpu_suspended() {
            return Ok(());
        }

        ws.emu.clear_frame_ready();
        let guard = ws
            .emu
            .cpu_cycles()
            .wrapping_add(u64::from(zeff_ws_core::hardware::constants::CYCLES_PER_FRAME) * 2);
        while !ws.emu.frame_ready() && ws.emu.cpu_cycles() < guard {
            link.poll_emulator(&mut ws.emu)?;
            let fetched = ws.emu.step_instruction();
            if fetched.is_none() && ws.emu.is_cpu_suspended() {
                break;
            }
        }
        ws.emu.finish_frame();
        link.poll_emulator(&mut ws.emu)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn step_frame_with_remote_link(
        &mut self,
        link: &mut crate::link::RemoteLink<crate::link::transport::TcpLinkTransport>,
    ) -> Result<(), crate::link::LinkSessionError> {
        match link {
            crate::link::RemoteLink::GameBoy(link) => {
                self.step_game_boy_frame_with_remote_link(link)
            }
            crate::link::RemoteLink::WonderSwan(link) => {
                self.step_wonder_swan_frame_with_remote_link(link)
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wait_for_wonder_swan_remote_link_window(
    emu: &mut zeff_ws_core::emulator::Emulator,
    link: &mut crate::link::ws::WonderSwanRemoteLink<crate::link::transport::TcpLinkTransport>,
) -> Result<bool, crate::link::LinkSessionError> {
    for _ in 0..WONDER_SWAN_REMOTE_LINK_WAIT_SPINS {
        link.poll_emulator(emu)?;
        if !emu.is_cpu_suspended() {
            return Ok(true);
        }
    }
    Ok(false)
}
