use crate::api::*;
use crate::callbacks::*;
use std::os::raw::{c_uint, c_void};
use zeff_emu_common::memory::MemoryRegionKind;

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_data(id: c_uint) -> *mut c_void {
    match id {
        RETRO_MEMORY_SAVE_RAM => lock(&crate::sram::SRAM).data(),
        RETRO_MEMORY_SYSTEM_RAM => {
            let mut core = lock(&CORE);
            if let Some(state) = core.as_mut()
                && !state.system_ram_buf.is_empty()
            {
                return state.system_ram_buf.as_mut_ptr() as *mut c_void;
            }
            std::ptr::null_mut()
        }
        RETRO_MEMORY_VIDEO_RAM => {
            let mut core = lock(&CORE);
            if let Some(state) = core.as_mut()
                && !state.video_ram_buf.is_empty()
            {
                return state.video_ram_buf.as_mut_ptr() as *mut c_void;
            }
            std::ptr::null_mut()
        }
        _ => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_size(id: c_uint) -> usize {
    match id {
        RETRO_MEMORY_SAVE_RAM => lock(&crate::sram::SRAM).len(),
        RETRO_MEMORY_SYSTEM_RAM => lock(&CORE)
            .as_ref()
            .map_or(0, |s| s.memory_region_size(MemoryRegionKind::SystemRam)),
        RETRO_MEMORY_VIDEO_RAM => lock(&CORE)
            .as_ref()
            .map_or(0, |s| s.memory_region_size(MemoryRegionKind::VideoRam)),
        _ => 0,
    }
}
