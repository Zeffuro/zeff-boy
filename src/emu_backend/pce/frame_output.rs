use std::cell::{OnceCell, RefCell};

use zeff_pce_core::hardware::{
    PceHardwareTopology, PcePresentedFrame, native_frame_descriptor, project_native_raw_frame,
};

use super::{PCE_PRESENTED_RGBA_BYTES, PceBackend};
use crate::emu_backend::pce_display::project_presented_frame;
use crate::settings::{PceOverscanMode, PcePaletteMode};

#[derive(Default)]
pub(super) struct PceFrameOutput {
    canonical: OnceCell<Vec<u8>>,
    canonical_spare: RefCell<Vec<u8>>,
    native: OnceCell<(Vec<u8>, (u32, u32))>,
    native_spare: RefCell<Vec<u8>>,
}

impl PceFrameOutput {
    pub(super) fn invalidate(&mut self) {
        if let Some(pixels) = self.canonical.take() {
            *self.canonical_spare.get_mut() = pixels;
        }
        if let Some((pixels, _)) = self.native.take() {
            *self.native_spare.get_mut() = pixels;
        }
    }

    fn canonical(
        &self,
        frame: PcePresentedFrame<'_>,
        topology: PceHardwareTopology,
        overscan: PceOverscanMode,
        palette: PcePaletteMode,
    ) -> &[u8] {
        self.canonical.get_or_init(|| {
            let mut pixels = self.canonical_spare.take();
            pixels.resize(PCE_PRESENTED_RGBA_BYTES, 0);
            project_presented_frame(frame, topology, overscan, palette, &mut pixels);
            pixels
        })
    }
}

impl PceBackend {
    pub(super) fn canonical_framebuffer(&self) -> &[u8] {
        self.frame_output.canonical(
            self.machine.presented_frame(),
            self.machine.hardware_topology(),
            self.overscan_mode,
            self.palette_mode,
        )
    }

    pub(crate) fn display_framebuffer(&self) -> (&[u8], Option<(u32, u32)>) {
        if self.overscan_mode != PceOverscanMode::Full
            || self.palette_mode != PcePaletteMode::RawRgb
        {
            return (self.canonical_framebuffer(), None);
        }
        let frame = self.machine.presented_frame();
        let Some(descriptor) = native_frame_descriptor(frame, self.machine.hardware_topology())
        else {
            return (self.canonical_framebuffer(), None);
        };
        let (pixels, dimensions) = self.frame_output.native.get_or_init(|| {
            let mut pixels = self.frame_output.native_spare.take();
            pixels.resize(descriptor.width() * descriptor.height() * 4, 0);
            project_native_raw_frame(frame, descriptor, &mut pixels);
            (
                pixels,
                (descriptor.width() as u32, descriptor.height() as u32),
            )
        });
        (pixels, Some(*dimensions))
    }
}

#[cfg(any(test, feature = "profile-cores"))]
fn synthetic_backend() -> PceBackend {
    use zeff_emu_common::time::FrameLifecycle;
    use zeff_pce_core::hardware::cpu::VdcPort;
    use zeff_pce_core::hardware::{VcePort, VdcRegister};
    let mut rom = vec![0xEA; 0x2000];
    rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let mut backend =
        PceBackend::new_with_overrides(rom, "synthetic-display.pce".into(), None, None, None)
            .unwrap();
    backend.machine.set_sample_generation_enabled(false);
    let vdc = backend.machine.devices_mut().vdc_mut();
    for (register, value) in [
        (VdcRegister::Control, 0x0080_u16),
        (VdcRegister::HorizontalDisplay, 31),
        (VdcRegister::VerticalSync, 0x0F02),
        (VdcRegister::VerticalDisplay, 0x00EF),
        (VdcRegister::VerticalDisplayEnd, 0x0004),
    ] {
        vdc.write_port(VdcPort::SelectOrStatus, register as u8);
        vdc.write_port(VdcPort::DataLow, value as u8);
        vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
    }
    let vce = backend.machine.devices_mut().vce_mut();
    for address in [0, 256] {
        vce.write_port(VcePort::from_offset(2), address as u8);
        vce.write_port(VcePort::from_offset(3), (address >> 8) as u8);
        vce.write_port(VcePort::from_offset(4), 0xFF);
        vce.write_port(VcePort::from_offset(5), 1);
    }
    backend.step_frame();
    backend.step_frame();
    let active_rows = backend
        .machine
        .presented_frame()
        .rows()
        .iter()
        .filter(|row| row.is_active())
        .count();
    assert!(active_rows >= 240);
    backend
}

#[cfg(feature = "profile-cores")]
pub(crate) fn profile_native_delivery(iterations: u32) {
    use std::hint::black_box;
    use std::time::Instant;
    use zeff_emu_common::time::FrameLifecycle;

    use crate::emu_core_trait::EmulatorCore;
    use crate::emu_thread::framebuffer::{
        new_shared_framebuffer, publish_framebuffer_with_dimensions,
    };

    let mut backend = synthetic_backend();
    let state = backend.encode_state_bytes().unwrap();
    let shared = new_shared_framebuffer();
    for block in 0..5 {
        for native in if block % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            backend.frame_output.invalidate();
            let cold = Instant::now();
            let (pixels, dimensions) = if native {
                backend.display_framebuffer()
            } else {
                (backend.canonical_framebuffer(), None)
            };
            publish_framebuffer_with_dimensions(&shared, pixels, dimensions);
            let cold_elapsed = cold.elapsed();
            for _ in 0..10 {
                backend.frame_output.invalidate();
                let (pixels, dimensions) = if native {
                    backend.display_framebuffer()
                } else {
                    (backend.canonical_framebuffer(), None)
                };
                publish_framebuffer_with_dimensions(&shared, pixels, dimensions);
            }
            let start = Instant::now();
            for _ in 0..iterations {
                backend.frame_output.invalidate();
                let (pixels, dimensions) = if native {
                    backend.display_framebuffer()
                } else {
                    (backend.canonical_framebuffer(), None)
                };
                publish_framebuffer_with_dimensions(black_box(&shared), pixels, dimensions);
            }
            let elapsed = start.elapsed();
            let frame = shared.load_full().unwrap();
            println!(
                "PCE delivery block={block} native={native} frames={iterations} cold_us={:.3} wall_ms={:.3} fps={:.3} bytes={} dimensions={:?}",
                cold_elapsed.as_secs_f64() * 1_000_000.0,
                elapsed.as_secs_f64() * 1000.0,
                f64::from(iterations) / elapsed.as_secs_f64(),
                frame.len(),
                frame.dimensions(),
            );
        }
    }
    assert_eq!(state, backend.encode_state_bytes().unwrap());
    let frames = 300;
    let mut expected_state = None;
    for block in 0..5 {
        for native in if block % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let mut backend = synthetic_backend();
            for _ in 0..10 {
                backend.step_frame();
            }
            let start = Instant::now();
            for _ in 0..frames {
                backend.step_frame();
                let (pixels, dimensions) = if native {
                    backend.display_framebuffer()
                } else {
                    (backend.canonical_framebuffer(), None)
                };
                publish_framebuffer_with_dimensions(black_box(&shared), pixels, dimensions);
            }
            let elapsed = start.elapsed();
            println!(
                "PCE frame delivery block={block} native={native} frames={frames} wall_ms={:.3} fps={:.3}",
                elapsed.as_secs_f64() * 1000.0,
                f64::from(frames) / elapsed.as_secs_f64(),
            );
            let state = backend.encode_state_bytes().unwrap();
            if let Some(expected) = &expected_state {
                assert_eq!(expected, &state);
            } else {
                expected_state = Some(state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emu_core_trait::EmulatorCore;
    use zeff_emu_common::time::{FrameLifecycle, Reset};

    #[test]
    fn native_delivery_preserves_canonical_pixels_and_state() {
        let backend = synthetic_backend();
        let state = backend.encode_state_bytes().unwrap();
        let (native, dimensions) = backend.display_framebuffer();
        assert_eq!(dimensions, Some((256, 242)));
        assert_eq!(native.len(), 256 * 242 * 4);
        assert!(backend.frame_output.canonical.get().is_none());
        let canonical = backend.framebuffer();
        for y in 0..480 {
            for x in 0..640 {
                let native_offset = ((y * 242 / 480) * 256 + x * 256 / 640) * 4;
                let canonical_offset = (y * 640 + x) * 4;
                assert_eq!(
                    &canonical[canonical_offset..][..4],
                    &native[native_offset..][..4]
                );
            }
        }
        assert_eq!(state, backend.encode_state_bytes().unwrap());
        assert!(backend.tas_presented_frame_is_current());
    }

    #[test]
    fn display_modes_fall_back_and_resume_native_output() {
        let mut backend = synthetic_backend();
        for (overscan, palette) in [
            (PceOverscanMode::TvSafe, PcePaletteMode::RawRgb),
            (PceOverscanMode::Conservative, PcePaletteMode::RawRgb),
            (PceOverscanMode::Full, PcePaletteMode::Composite),
        ] {
            backend.set_display_config(overscan, palette);
            let (pixels, dimensions) = backend.display_framebuffer();
            assert_eq!(dimensions, None);
            assert_eq!(pixels, backend.framebuffer());
            assert_eq!(pixels.len(), PCE_PRESENTED_RGBA_BYTES);
            backend.set_display_config(PceOverscanMode::Full, PcePaletteMode::RawRgb);
            assert_eq!(backend.display_framebuffer().1, Some((256, 242)));
        }
    }

    #[test]
    fn changed_native_width_is_published_with_its_pixels() {
        use zeff_pce_core::hardware::VdcRegister;
        use zeff_pce_core::hardware::cpu::VdcPort;
        let mut backend = synthetic_backend();
        assert_eq!(backend.display_framebuffer().1, Some((256, 242)));
        let vdc = backend.machine.devices_mut().vdc_mut();
        vdc.write_port(
            VdcPort::SelectOrStatus,
            VdcRegister::HorizontalDisplay as u8,
        );
        vdc.write_port(VdcPort::DataLow, 39);
        vdc.write_port(VdcPort::DataHigh, 0);
        backend.step_frame();
        backend.step_frame();
        let (pixels, dimensions) = backend.display_framebuffer();
        assert_eq!(dimensions, Some((320, 242)));
        assert_eq!(pixels.len(), 320 * 242 * 4);
        assert!(backend.tas_presented_frame_is_current());
    }

    #[test]
    fn supergrafx_keeps_the_canonical_projection() {
        let mut rom = vec![0xEA; 0x2000];
        rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let mut backend = PceBackend::new_with_overrides(
            rom,
            "synthetic-sgx.pce".into(),
            None,
            None,
            Some(zeff_pce_core::hardware::PceCartridgeHardware::SuperGrafx),
        )
        .unwrap();
        backend.machine.set_sample_generation_enabled(false);
        backend.step_frame();
        let (pixels, dimensions) = backend.display_framebuffer();
        assert_eq!(dimensions, None);
        assert_eq!(pixels, backend.framebuffer());
        assert_eq!(pixels.len(), PCE_PRESENTED_RGBA_BYTES);
        assert!(backend.frame_output.native.get().is_none());
    }

    #[test]
    fn frame_restore_and_reset_invalidate_both_caches() {
        let mut backend = synthetic_backend();
        let state = backend.encode_state_bytes().unwrap();
        let canonical = backend.framebuffer().to_vec();
        let native = backend.display_framebuffer().0.to_vec();
        let native_allocation = backend.display_framebuffer().0.as_ptr();
        let canonical_allocation = backend.framebuffer().as_ptr();
        for transition in 0..3 {
            backend.frame_output.canonical.get_mut().unwrap().fill(0xA5);
            backend.frame_output.native.get_mut().unwrap().0.fill(0x5A);
            match transition {
                0 => backend.step_frame(),
                1 => {
                    backend.load_state_from_bytes(state.clone()).unwrap();
                }
                _ => backend.reset(),
            }
            assert!(backend.frame_output.canonical.get().is_none());
            assert!(backend.frame_output.native.get().is_none());
            assert!(backend.tas_presented_frame_is_current());
            if transition < 2 {
                assert_eq!(backend.framebuffer(), canonical);
                assert_eq!(backend.display_framebuffer().0, native);
                assert_eq!(backend.framebuffer().as_ptr(), canonical_allocation);
                assert_eq!(backend.display_framebuffer().0.as_ptr(), native_allocation);
            }
        }
    }

    #[test]
    fn failed_restore_preserves_both_cached_outputs() {
        let mut backend = synthetic_backend();
        let canonical = backend.framebuffer().to_vec();
        let native = backend.display_framebuffer().0.to_vec();
        assert!(backend.load_state_from_bytes(vec![0; 32]).is_err());
        assert_eq!(backend.framebuffer(), canonical);
        assert_eq!(backend.display_framebuffer().0, native);
    }
}
