//! Run a supplied libretro cdylib and ROM with repeatable fixed-frame inputs.

#[cfg(not(target_family = "wasm"))]
#[path = "../libretro_harness.rs"]
mod libretro_harness;

#[cfg(not(target_family = "wasm"))]
mod native {

    use super::libretro_harness::{
        CoreOption, FrameCaptureRequest, HarnessConfig, JoypadInput, PixelFormat, load_rom,
        run_repeated_fixed_frames,
    };
    use std::env;
    use std::error::Error;
    use std::path::PathBuf;

    pub(super) fn main() -> Result<(), Box<dyn Error>> {
        let mut args = env::args().skip(1);
        let Some(core_path) = args.next() else {
            println!("{}", usage());
            return Ok(());
        };
        if matches!(core_path.as_str(), "--help" | "-h") {
            println!("{}", usage());
            return Ok(());
        }
        let Some(content_path) = args.next() else {
            return Err(usage().into());
        };
        let mut warmup_frames = 120;
        let mut measurement_frames = 600;
        let mut repeats = 1;
        let mut pixel_format = PixelFormat::Xrgb8888;
        let mut inputs = Vec::new();
        let mut core_options = Vec::new();
        let mut system_directory = None;
        let mut save_directory = None;
        let mut capture_frame = None;
        let mut capture_png = None;
        let mut audio_frame_csv = None;
        let mut audio_s16le = None;
        let mut blackhole_output = false;
        while let Some(option) = args.next() {
            match option.as_str() {
                "--warmup" => warmup_frames = parse_usize(args.next(), "--warmup")?,
                "--frames" => measurement_frames = parse_usize(args.next(), "--frames")?,
                "--repeat" => repeats = parse_usize(args.next(), "--repeat")?,
                "--pixel-format" => {
                    let value = args.next().ok_or("--pixel-format requires a value")?;
                    pixel_format = PixelFormat::parse(&value)
                        .ok_or("pixel format must be xrgb8888 or rgb565")?;
                }
                "--input" => inputs.push(parse_input(
                    &args.next().ok_or("--input requires a value")?,
                )?),
                "--core-option" => core_options.push(parse_core_option(
                    &args.next().ok_or("--core-option requires key=value")?,
                )?),
                "--system-dir" => {
                    system_directory = Some(PathBuf::from(
                        args.next().ok_or("--system-dir requires a path")?,
                    ));
                }
                "--save-dir" => {
                    save_directory = Some(PathBuf::from(
                        args.next().ok_or("--save-dir requires a path")?,
                    ));
                }
                "--capture-frame" => {
                    capture_frame = Some(parse_usize(args.next(), "--capture-frame")?);
                }
                "--capture-png" => {
                    capture_png = Some(PathBuf::from(
                        args.next().ok_or("--capture-png requires a path")?,
                    ));
                }
                "--audio-frame-csv" => {
                    audio_frame_csv = Some(PathBuf::from(
                        args.next().ok_or("--audio-frame-csv requires a path")?,
                    ));
                }
                "--audio-s16le" => {
                    audio_s16le = Some(PathBuf::from(
                        args.next().ok_or("--audio-s16le requires a path")?,
                    ));
                }
                "--blackhole-output" => blackhole_output = true,
                "--help" | "-h" => {
                    println!("{}", usage());
                    return Ok(());
                }
                _ => return Err(format!("unknown option '{option}'\n{}", usage()).into()),
            }
        }
        let frame_capture = match (capture_frame, capture_png) {
            (Some(frame), Some(path)) => Some(FrameCaptureRequest { frame, path }),
            (None, None) => None,
            _ => return Err("--capture-frame and --capture-png must be supplied together".into()),
        };
        let content_path = PathBuf::from(content_path);
        let result = run_repeated_fixed_frames(
            &HarnessConfig {
                core_path: PathBuf::from(core_path),
                rom_bytes: load_rom(&content_path)?,
                content_path,
                warmup_frames,
                measurement_frames,
                pixel_format,
                inputs,
                core_options,
                system_directory,
                save_directory,
                frame_capture,
                audio_frame_csv,
                audio_s16le,
                blackhole_output,
            },
            repeats,
        )?;
        let run = result.runs.last().expect("nonzero repeats has a result");
        println!(
            "runs={} fps_p50={:.2} fps_p95={:.2} elapsed_ms_p50={:.3} elapsed_ms_p95={:.3} callback_payload_hashing={} video_calls={} video_bytes={} visible_video_bytes={} video_pixel_format={} video_sha256={} audio_sample_calls={} audio_batch_calls={} audio_frames={} audio_bytes={} invalid_audio_buffer_len={} audio_sha256={} input_polls={} input_queries={} geometry={}x{} max={}x{} aspect={} advertised_fps={} advertised_sample_rate={} last_video={}x{} pitch={} serialize_size={} serialize_sha256={} save_ram_size={} save_ram_nonnull={} save_ram_sha256={} save_ram_post_roundtrip_sha256={} state_roundtrip={} undersized_serialize_rejected={} repeated_state_hashes_match={} repeated_video_hashes_match={} repeated_audio_hashes_match={} repeated_callback_counts_match={} unsupported_environment_commands={}",
            result.runs.len(),
            result.fps_p50,
            result.fps_p95,
            result.elapsed_ms_p50,
            result.elapsed_ms_p95,
            run.callback_payload_hashing,
            run.callbacks.video_calls,
            run.callbacks.video_bytes,
            run.callbacks.visible_video_bytes,
            run.video_pixel_format,
            callback_hash(run.video_hash.as_ref()),
            run.callbacks.audio_sample_calls,
            run.callbacks.audio_batch_calls,
            run.callbacks.audio_frames,
            run.callbacks.audio_bytes,
            run.invalid_audio_buffer_len,
            callback_hash(run.audio_hash.as_ref()),
            run.callbacks.input_poll_calls,
            run.callbacks.input_state_calls,
            run.geometry.base_width,
            run.geometry.base_height,
            run.geometry.max_width,
            run.geometry.max_height,
            run.geometry.aspect_ratio,
            run.advertised_frames_per_second,
            run.advertised_sample_rate,
            run.last_video.width,
            run.last_video.height,
            run.last_video.pitch,
            run.serialize_size,
            hex(&run.serialize_hash),
            run.save_ram_size,
            run.save_ram_nonnull,
            save_ram_hash(run.save_ram_sha256.as_ref()),
            save_ram_hash(run.save_ram_post_roundtrip_sha256.as_ref()),
            run.state_roundtrip,
            run.undersized_serialize_rejected,
            result.state_hashes_match,
            evaluated(result.video_hashes_match),
            evaluated(result.audio_hashes_match),
            result.callback_counts_match,
            comma_separated(&run.unsupported_environment_commands),
        );
        Ok(())
    }

    fn parse_core_option(value: &str) -> Result<CoreOption, Box<dyn Error>> {
        let (key, value) = value
            .split_once('=')
            .filter(|(key, value)| !key.is_empty() && !value.is_empty())
            .ok_or("--core-option needs nonempty key=value")?;
        Ok(CoreOption {
            key: key.to_owned(),
            value: value.to_owned(),
        })
    }

    fn parse_usize(value: Option<String>, option: &str) -> Result<usize, Box<dyn Error>> {
        value
            .ok_or_else(|| format!("{option} requires a value"))?
            .parse()
            .map_err(|_| format!("{option} must be a nonnegative integer").into())
    }

    fn parse_input(value: &str) -> Result<JoypadInput, Box<dyn Error>> {
        let mut fields = value.split(':');
        let frame = fields
            .next()
            .ok_or("input needs frame:port:mask")?
            .parse()?;
        let port = fields
            .next()
            .ok_or("input needs frame:port:mask")?
            .parse()?;
        let mask = fields.next().ok_or("input needs frame:port:mask")?;
        if fields.next().is_some() {
            return Err("input needs frame:port:mask".into());
        }
        let mask = mask.strip_prefix("0x").unwrap_or(mask);
        Ok(JoypadInput {
            frame,
            port,
            mask: u16::from_str_radix(mask, 16)?,
        })
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn callback_hash(hash: Option<&[u8; 32]>) -> String {
        hash.map_or_else(|| "disabled".into(), |hash| hex(hash))
    }

    fn save_ram_hash(hash: Option<&[u8; 32]>) -> String {
        hash.map_or_else(|| "empty".into(), |hash| hex(hash))
    }

    fn evaluated(value: Option<bool>) -> &'static str {
        match value {
            Some(true) => "true",
            Some(false) => "false",
            None => "not_evaluated",
        }
    }

    fn comma_separated(values: &[u32]) -> String {
        if values.is_empty() {
            return "none".into();
        }
        values
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }

    fn usage() -> &'static str {
        "usage: libretro_harness <core-library> <rom> [--warmup N] [--frames N] [--repeat N] [--pixel-format xrgb8888|rgb565] [--input frame:port:joypad-mask] [--core-option key=value] [--system-dir path] [--save-dir path] [--capture-frame N --capture-png path] [--audio-frame-csv path] [--audio-s16le path] [--blackhole-output]"
    }

    #[cfg(test)]
    mod tests {
        use super::{callback_hash, evaluated, save_ram_hash};

        #[test]
        fn disabled_callback_hashes_are_explicit() {
            assert_eq!(callback_hash(None), "disabled");
            assert_eq!(save_ram_hash(None), "empty");
            assert_eq!(evaluated(None), "not_evaluated");
        }

        #[test]
        fn enabled_callback_hash_results_remain_machine_parseable() {
            assert_eq!(callback_hash(Some(&[0xAB; 32])), "ab".repeat(32));
            assert_eq!(evaluated(Some(true)), "true");
            assert_eq!(evaluated(Some(false)), "false");
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::main()
}

#[cfg(target_family = "wasm")]
fn main() {}
