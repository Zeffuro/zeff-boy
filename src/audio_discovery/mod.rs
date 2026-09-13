use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) use zeff_audio_discovery::*;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod assets;
#[cfg(not(target_arch = "wasm32"))]
mod audio_file;
#[cfg(not(target_arch = "wasm32"))]
mod bank;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod batch;
#[cfg(not(target_arch = "wasm32"))]
mod bundle;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod cdda;
#[cfg(not(target_arch = "wasm32"))]
mod container_export;
#[cfg(not(target_arch = "wasm32"))]
mod dls;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod export;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod extract;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod formats;
#[cfg(not(target_arch = "wasm32"))]
mod gb_export;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod gsf;
pub(crate) mod media;
#[cfg(not(target_arch = "wasm32"))]
mod midi;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod naming;
#[cfg(not(target_arch = "wasm32"))]
mod native_rip_export;
pub(crate) mod natsume;
#[cfg(not(target_arch = "wasm32"))]
mod natsume_export;
#[cfg(not(target_arch = "wasm32"))]
mod nes_export;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod pcm;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod preview;
#[cfg(not(target_arch = "wasm32"))]
mod projection;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod render;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod session;
#[cfg(not(target_arch = "wasm32"))]
mod sf2;
#[cfg(not(target_arch = "wasm32"))]
mod sfz;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(not(target_arch = "wasm32"))]
mod timeline;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod vgm_export;

struct Budget<'a> {
    cancel: &'a AtomicBool,
    remaining: u64,
}

fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

impl Budget<'_> {
    fn charge(&mut self) -> Result<(), ScanStop> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(ScanStop::Cancelled);
        }
        if self.remaining == 0 {
            return Err(ScanStop::WorkLimit);
        }
        self.remaining -= 1;
        Ok(())
    }
}
