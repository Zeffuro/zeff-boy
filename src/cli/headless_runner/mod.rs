use std::collections::VecDeque;
use std::path::Path;
use std::time::Instant;

use zeff_gb_core::emulator::Emulator as GbEmulator;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_gba_core::emulator::Emulator as GbaEmulator;
use zeff_gba_core::hardware::constants::CYCLES_PER_FRAME as GBA_CYCLES_PER_FRAME;
use zeff_nes_core::emulator::Emulator as NesEmulator;
use zeff_ws_core::emulator::Emulator as WsEmulator;

use crate::emu_backend::ActiveSystem;

use super::output::{
    TraceContext, format_headless_breakpoint, format_headless_serial, format_headless_summary,
    format_op_line, format_op_tail_line,
};
use super::trace_filters::{ime_short, mode_short, should_trace_op};
use super::types::{HeadlessBusTraceAccess, HeadlessOptions};
use audio::AudioStats;
use coleco::run_coleco_headless;
use debug_state::*;
use gb::run_gb_headless;
use gba::run_gba_headless;
use input::{
    InputMasks, input_for_frame, input_p2_for_frame, input_p3_for_frame, input_p4_for_frame,
    input_p5_for_frame,
};
use nes::run_nes_headless;
use pce::{run_pce_archive_headless, run_pce_headless};
use replay::run_replay_headless;
use screenshots::*;
use sega8::run_sega8_headless;
use state_artifacts::*;
use stuck::{
    StuckReport, StuckTracker, fail_on_stuck_if_needed, format_pc, framebuffer_fingerprint,
    observe_stuck,
};
use support::{
    ensure_no_reset_events, ensure_system_headless_options, flush_battery, load_headless_rom,
    print_memory_region_dumps, print_perf, read_headless_state_if_requested,
};
use trace::*;
use ws::run_ws_headless;

mod audio;
mod coleco;
mod debug_state;
mod gb;
mod gba;
mod input;
mod nes;
mod pce;
mod replay;
mod screenshots;
mod sega8;
mod state_artifacts;
mod stuck;
mod support;
#[cfg(test)]
mod tests;
mod trace;
mod ws;

pub(crate) fn run_headless(
    path: &Path,
    mode_preference: HardwareModePreference,
    firmware_search_dirs: Vec<std::path::PathBuf>,
    opts: &HeadlessOptions,
) -> anyhow::Result<()> {
    if crate::app::is_native_archive_path(path) {
        if opts.replay_path.is_some() {
            anyhow::bail!("replay playback from archives is not supported in headless mode");
        }
        return run_pce_archive_headless(path, mode_preference, firmware_search_dirs, opts);
    }

    let (rom_path, preloaded_data, system) = crate::app::detect_and_extract_rom(path)?;
    if system == ActiveSystem::Pce {
        if opts.replay_path.is_some() {
            anyhow::bail!("PC Engine replay playback is not supported yet");
        }
        return run_pce_headless(
            path,
            &rom_path,
            preloaded_data,
            mode_preference,
            firmware_search_dirs,
            opts,
        );
    }
    let rom_data = match preloaded_data {
        Some(data) => data,
        None => std::fs::read(path)?,
    };

    if opts.replay_path.is_some() {
        return run_replay_headless(
            path,
            &rom_path,
            rom_data,
            system,
            firmware_search_dirs,
            opts,
        );
    }

    match system {
        ActiveSystem::GameBoy => run_gb_headless(&rom_path, &rom_data, mode_preference, opts),
        ActiveSystem::GameBoyAdvance => run_gba_headless(&rom_path, &rom_data, opts),
        ActiveSystem::Nes => run_nes_headless(&rom_path, &rom_data, &firmware_search_dirs, opts),
        ActiveSystem::Coleco => {
            run_coleco_headless(&rom_path, &rom_data, &firmware_search_dirs, opts)
        }
        ActiveSystem::Pce => unreachable!("PC Engine runs dispatch before raw ROM loading"),
        ActiveSystem::WonderSwan => run_ws_headless(&rom_path, &rom_data, opts),
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => {
            run_sega8_headless(&rom_path, &rom_data, system, opts)
        }
    }
}
