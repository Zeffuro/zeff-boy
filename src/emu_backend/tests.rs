use super::firmware::{
    default_firmware_manifests_for_active_system, firmware_plan_for_active_system,
};
use super::{
    ActiveSystem, BackendLoadConfig, BackendRuntimeConfig, DetachedFrameBackend, EmuBackend,
    ROM_EXTENSIONS, load_backend_from_rom_source, system_specs,
};
use crate::cheats::{CheatPatch, CheatValue};
use crate::debug::DebugUiActions;
use crate::emu_core_trait::DebuggableEmulator;
use crate::emu_thread::GuestCallRequest;
use crate::symbols::ExecMode;
use std::collections::BTreeSet;
use std::path::PathBuf;
use zeff_emu_common::debug::{DebugEvent, TraceExecMode, WatchType};
use zeff_emu_common::memory::MemoryRegionDescriptor;
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::{FrameLifecycle, MachineTiming, Reset};
use zeff_gb_core::hardware::types::constants::{INTERRUPT_IF, SERIAL_SB, SERIAL_SC};

mod audio;
mod capabilities;
#[cfg(target_arch = "wasm32")]
mod coleco_browser;
mod debug;
mod fixtures;
mod guest_calls;
mod link;
mod loading;
mod pce;
mod runahead_conformance;
mod state;
mod system;

use fixtures::*;

static TEST_COLECO_BIOS: [u8; 8 * 1024] = [0; 8 * 1024];
