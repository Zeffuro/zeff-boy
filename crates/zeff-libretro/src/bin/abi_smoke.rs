//! ROM-free C-ABI regression smoke for the built Zeff libretro cdylib.

#[cfg(not(target_family = "wasm"))]
#[path = "../libretro_harness.rs"]
#[allow(dead_code)]
mod libretro_harness;

#[cfg(not(target_family = "wasm"))]
mod native {

    use super::libretro_harness::{HarnessConfig, PixelFormat, run_fixed_frames};
    use std::env;
    use std::error::Error;
    use std::path::PathBuf;

    const PCE_WIDTH: u32 = zeff_pce_core::hardware::PCE_HOST_FRAME_WIDTH as u32;
    const PCE_HEIGHT: u32 = zeff_pce_core::hardware::PCE_HOST_FRAME_HEIGHT as u32;
    const GB_WIDTH: u32 = zeff_emu_common::system::GAME_BOY_SCREEN_SIZE.0;
    const GB_HEIGHT: u32 = zeff_emu_common::system::GAME_BOY_SCREEN_SIZE.1;
    const XRGB8888_BYTES_PER_PIXEL: usize = zeff_emu_common::system::RGBA_BYTES_PER_PIXEL;
    const RGB565_BYTES_PER_PIXEL: usize = 2;

    #[derive(Clone, Copy)]
    enum SmokeSystem {
        Pce,
        Gb,
    }

    fn synthetic_hucard() -> Vec<u8> {
        let mut rom = vec![0xEA; 0x2000];
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        rom
    }

    fn parse_args() -> Result<(PathBuf, PixelFormat, SmokeSystem), Box<dyn Error>> {
        let mut args = env::args().skip(1);
        let mut library = None;
        let mut format = None;
        let mut system = SmokeSystem::Pce;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--pixel-format" => {
                    let value = args.next().ok_or("--pixel-format requires a value")?;
                    format = PixelFormat::parse(&value);
                    if format.is_none() {
                        return Err("pixel format must be xrgb8888 or rgb565".into());
                    }
                }
                "--system" => match args.next().as_deref() {
                    Some("pce") => system = SmokeSystem::Pce,
                    Some("gb") => system = SmokeSystem::Gb,
                    _ => return Err("system must be pce or gb".into()),
                },
                value if !value.starts_with('-') && library.is_none() => {
                    library = Some(PathBuf::from(value))
                }
                _ => {
                    return Err(
                    "usage: abi_smoke [core-library] --pixel-format <xrgb8888|rgb565> [--system pce|gb]".into(),
                );
                }
            }
        }
        Ok((
            library.unwrap_or_else(default_library_path),
            format.ok_or("--pixel-format is required")?,
            system,
        ))
    }

    fn default_library_path() -> PathBuf {
        let library = if cfg!(target_os = "windows") {
            "zeff_libretro.dll"
        } else if cfg!(target_os = "macos") {
            "libzeff_libretro.dylib"
        } else {
            "libzeff_libretro.so"
        };
        PathBuf::from("target").join("release").join(library)
    }

    pub(super) fn main() -> Result<(), Box<dyn Error>> {
        let (core_path, pixel_format, system) = parse_args()?;
        let (content_path, rom_bytes, width, height, requires_large_state) = match system {
            SmokeSystem::Pce => (
                PathBuf::from("abi-smoke.pce"),
                synthetic_hucard(),
                PCE_WIDTH,
                PCE_HEIGHT,
                true,
            ),
            SmokeSystem::Gb => (
                PathBuf::from("abi-smoke.gb"),
                vec![0; 0x8000],
                GB_WIDTH,
                GB_HEIGHT,
                false,
            ),
        };
        let result = run_fixed_frames(&HarnessConfig {
            core_path,
            content_path,
            rom_bytes,
            warmup_frames: 0,
            measurement_frames: 4,
            pixel_format,
            inputs: Vec::new(),
            core_options: Vec::new(),
            system_directory: None,
            save_directory: None,
        })?;
        assert_eq!(
            (result.geometry.base_width, result.geometry.base_height),
            (width, height)
        );
        assert_eq!(
            (result.last_video.width, result.last_video.height),
            (width, height)
        );
        if requires_large_state {
            assert_eq!(
                (result.geometry.max_width, result.geometry.max_height),
                (PCE_WIDTH, PCE_HEIGHT)
            );
            assert_eq!(result.geometry.aspect_ratio, 4.0 / 3.0);
            assert!(result.serialize_size > 4 * 1024 * 1024);
        } else {
            assert!(result.serialize_size > 0);
        }
        assert!(result.undersized_serialize_rejected);
        assert!(result.state_roundtrip);
        assert_eq!(result.callbacks.video_calls, 4);
        let expected_pitch = width as usize
            * if pixel_format == PixelFormat::Xrgb8888 {
                XRGB8888_BYTES_PER_PIXEL
            } else {
                RGB565_BYTES_PER_PIXEL
            };
        assert_eq!(result.last_video.pitch, expected_pitch);
        println!(
            "libretro C-ABI smoke passed: {pixel_format} {width}x{height} pitch {expected_pitch}"
        );
        Ok(())
    }
}

#[cfg(not(target_family = "wasm"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::main()
}

#[cfg(target_family = "wasm")]
fn main() {}
