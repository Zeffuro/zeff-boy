use std::path::PathBuf;

use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::types::{
    CliArgs, HeadlessBusTraceAccess, HeadlessBusTraceFilter, HeadlessInputEvent,
    HeadlessMemoryDump, HeadlessOptions,
};

fn parse_u64_arg(value: &str, flag: &str) -> anyhow::Result<u64> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16)
    } else {
        trimmed.parse::<u64>()
    }
    .map_err(|_| anyhow::anyhow!("{} must be an unsigned integer", flag))
}

fn parse_usize_arg(value: &str, flag: &str) -> anyhow::Result<usize> {
    value
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("{} must be an unsigned integer", flag))
}

fn parse_u16_arg(value: &str, flag: &str) -> anyhow::Result<u16> {
    let trimmed = value.trim();
    let parsed = if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16)
    } else {
        trimmed.parse::<u16>()
    };
    parsed.map_err(|_| anyhow::anyhow!("{} must be a u16 value (decimal or 0x-prefixed hex)", flag))
}

fn parse_u8_arg(value: &str, flag: &str) -> anyhow::Result<u8> {
    let parsed = parse_u16_arg(value, flag)?;
    u8::try_from(parsed).map_err(|_| anyhow::anyhow!("{} value must fit in u8", flag))
}

fn parse_addr_arg(value: &str, flag: &str) -> anyhow::Result<u64> {
    let trimmed = value.trim();
    let trimmed = trimmed.strip_prefix('$').unwrap_or(trimmed);
    let parsed = if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16)
    } else if trimmed.len() == 4
        || trimmed.len() == 8
        || trimmed
            .chars()
            .any(|c| c.is_ascii_hexdigit() && !c.is_ascii_digit())
    {
        u64::from_str_radix(trimmed, 16)
    } else {
        trimmed.parse::<u64>()
    };
    parsed.map_err(|_| anyhow::anyhow!("{} must be an address", flag))
}

fn parse_pc_range_arg(value: &str) -> anyhow::Result<(u64, u64)> {
    let Some((start_raw, end_raw)) = value.split_once('-') else {
        anyhow::bail!("--trace-pc-range must be start-end (decimal or hex, e.g. 0x0100-0x01FF)",);
    };
    let start = parse_u64_arg(start_raw, "--trace-pc-range")?;
    let end = parse_u64_arg(end_raw, "--trace-pc-range")?;
    if start > end {
        anyhow::bail!("--trace-pc-range start must be <= end");
    }
    Ok((start, end))
}

fn parse_addr_range_list_arg(
    value: &str,
    flag: &str,
    access: HeadlessBusTraceAccess,
) -> anyhow::Result<Vec<HeadlessBusTraceFilter>> {
    let mut filters = Vec::new();
    for raw in value.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }

        let (start_addr, end_addr) = if let Some((start_raw, end_raw)) = raw.split_once('-') {
            (
                parse_addr_arg(start_raw, flag)?,
                parse_addr_arg(end_raw, flag)?,
            )
        } else {
            let addr = parse_addr_arg(raw, flag)?;
            (addr, addr)
        };

        if start_addr > end_addr {
            anyhow::bail!("{flag} range start must be <= end");
        }

        filters.push(HeadlessBusTraceFilter {
            start_addr,
            end_addr,
            access,
        });
    }

    if filters.is_empty() {
        anyhow::bail!("{flag} did not contain any address ranges");
    }

    Ok(filters)
}

fn parse_memory_dump_arg(value: &str, flag: &str) -> anyhow::Result<HeadlessMemoryDump> {
    let Some((addr_raw, len_raw)) = value.split_once(':') else {
        anyhow::bail!("{flag} must be addr:len, e.g. C000:60 or 0xC000:0x60");
    };
    let addr = parse_addr_arg(addr_raw, flag)?;
    let len = parse_u64_arg(len_raw, flag)?;
    if addr > u64::from(u16::MAX) {
        anyhow::bail!("{flag} address must fit in the GB 16-bit address space");
    }
    if len == 0 {
        anyhow::bail!("{flag} length must be greater than zero");
    }
    if len > 4096 {
        anyhow::bail!("{flag} length is capped at 4096 bytes");
    }
    Ok(HeadlessMemoryDump {
        start_addr: addr as u16,
        len: len as u16,
    })
}

fn parse_gba_bg_layer_list_arg(value: &str, flag: &str) -> anyhow::Result<[bool; 4]> {
    let mut hidden = [false; 4];
    let mut parsed_any = false;
    for raw in value.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        parsed_any = true;
        let raw = raw
            .strip_prefix("bg")
            .or_else(|| raw.strip_prefix("BG"))
            .unwrap_or(raw);
        let index = parse_usize_arg(raw, flag)?;
        if index > 3 {
            anyhow::bail!("{flag} accepts BG layers 0, 1, 2, or 3");
        }
        hidden[index] = true;
    }
    if !parsed_any {
        anyhow::bail!("{flag} requires at least one BG layer");
    }
    Ok(hidden)
}

fn parse_dmg_palette_arg(value: &str, flag: &str) -> anyhow::Result<DmgPalettePreset> {
    let token = value.trim().to_ascii_lowercase().replace(['_', '-'], "");
    match token.as_str() {
        "gray" | "grey" | "grayscale" | "greyscale" => Ok(DmgPalettePreset::Gray),
        "dmggreen" | "green" => Ok(DmgPalettePreset::DmgGreen),
        "pocket" | "gameboypocket" => Ok(DmgPalettePreset::Pocket),
        "mint" => Ok(DmgPalettePreset::Mint),
        "chocolate" => Ok(DmgPalettePreset::Chocolate),
        _ => anyhow::bail!(
            "{flag} has unknown palette {value:?}; expected gray, dmg-green, pocket, mint, or chocolate"
        ),
    }
}

fn parse_gba_audio_mute_list_arg(value: &str, flag: &str) -> anyhow::Result<[bool; 6]> {
    let mut mutes = [false; 6];
    let mut parsed_any = false;
    for raw in value.split(',') {
        let token = raw.trim().to_ascii_lowercase().replace(['_', '-'], "");
        if token.is_empty() {
            continue;
        }
        parsed_any = true;
        let index = match token.as_str() {
            "0" | "1" | "psg0" | "psg1" | "square1" | "ch1" => 0,
            "2" | "psg2" | "square2" | "ch2" => 1,
            "3" | "psg3" | "wave" | "ch3" => 2,
            "4" | "psg4" | "noise" | "ch4" => 3,
            "5" | "fifoa" | "directa" | "a" => 4,
            "6" | "fifob" | "directb" | "b" => 5,
            "psg" => {
                mutes[..4].fill(true);
                continue;
            }
            "fifo" | "direct" | "pcm" => {
                mutes[4] = true;
                mutes[5] = true;
                continue;
            }
            other => anyhow::bail!(
                "{flag} has unknown channel {other:?}; expected psg1..psg4, fifoA, fifoB, psg, fifo"
            ),
        };
        mutes[index] = true;
    }
    if !parsed_any {
        anyhow::bail!("{flag} requires at least one audio channel");
    }
    Ok(mutes)
}

fn parse_frame_range_arg(value: &str, flag: &str) -> anyhow::Result<(u64, u64)> {
    let trimmed = value.trim();
    let (start, end) = if let Some((start_raw, end_raw)) = trimmed.split_once('-') {
        (
            parse_u64_arg(start_raw.trim(), flag)?,
            parse_u64_arg(end_raw.trim(), flag)?,
        )
    } else {
        let frame = parse_u64_arg(trimmed, flag)?;
        (frame, frame)
    };
    if start == 0 {
        anyhow::bail!("{flag} frame ranges are 1-based; start must be >= 1");
    }
    if start > end {
        anyhow::bail!("{flag} range start must be <= end");
    }
    Ok((start, end))
}

fn parse_input_keys(value: &str, flag: &str) -> anyhow::Result<(u8, u8, bool)> {
    let mut buttons = 0u8;
    let mut dpad = 0u8;
    let mut reset = false;
    let mut parsed_any = false;

    for raw_key in value.split(|c: char| c == '+' || c == '|' || c.is_whitespace()) {
        let key = raw_key.trim().to_ascii_lowercase().replace(['_', '-'], "");
        if key.is_empty() {
            continue;
        }
        parsed_any = true;
        match key.as_str() {
            "a" => buttons |= 0x01,
            "b" => buttons |= 0x02,
            "select" | "sel" => buttons |= 0x04,
            "start" => buttons |= 0x08,
            "l" | "leftshoulder" | "shoulderl" => buttons |= 0x10,
            "r" | "rightshoulder" | "shoulderr" => buttons |= 0x20,
            "right" => dpad |= 0x01,
            "left" => dpad |= 0x02,
            "up" => dpad |= 0x04,
            "down" => dpad |= 0x08,
            "reset" | "softreset" | "softresetbutton" => reset = true,
            _ => anyhow::bail!(
                "{flag} has unknown key {:?}; expected a,b,select,start,l,r,up,down,left,right,reset",
                raw_key
            ),
        }
    }

    if !parsed_any {
        anyhow::bail!("{flag} requires at least one key");
    }

    Ok((buttons, dpad, reset))
}

fn parse_input_event_arg(value: &str, flag: &str) -> anyhow::Result<Vec<HeadlessInputEvent>> {
    let mut events = Vec::new();
    for raw_event in value.split(',') {
        let event = raw_event.trim();
        if event.is_empty() {
            continue;
        }

        let (keys_raw, range_raw) = if let Some((keys, range)) = event.split_once('@') {
            (keys.trim(), range.trim())
        } else if let Some((left, right)) = event.split_once(':') {
            if left
                .trim()
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
            {
                (right.trim(), left.trim())
            } else {
                (left.trim(), right.trim())
            }
        } else {
            anyhow::bail!("{flag} event must be key@start-end, key:start-end, or start-end:key");
        };

        let (buttons, dpad, reset) = parse_input_keys(keys_raw, flag)?;
        let (start_frame, end_frame) = parse_frame_range_arg(range_raw, flag)?;
        events.push(HeadlessInputEvent {
            start_frame,
            end_frame,
            buttons,
            dpad,
            reset,
        });
    }

    if events.is_empty() {
        anyhow::bail!("{flag} did not contain any input events");
    }

    Ok(events)
}

fn parse_input_script(path: &str) -> anyhow::Result<Vec<HeadlessInputEvent>> {
    let contents = std::fs::read_to_string(path)
        .map_err(|err| anyhow::anyhow!("failed to read --input-script '{}': {}", path, err))?;
    let mut events = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        events.extend(
            parse_input_event_arg(line, "--input-script")
                .map_err(|err| anyhow::anyhow!("{} at {}:{}", err, path, index + 1))?,
        );
    }
    Ok(events)
}

pub(crate) fn parse_args() -> anyhow::Result<CliArgs> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode_override: Option<HardwareModePreference> = None;
    let mut rom_path: Option<String> = None;
    let mut headless_enabled = false;
    let mut headless = HeadlessOptions::default();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--mode requires one of: auto|dmg|cgb");
                };
                mode_override = Some(match value.as_str() {
                    "auto" => HardwareModePreference::Auto,
                    "dmg" => HardwareModePreference::ForceDmg,
                    "cgb" => HardwareModePreference::ForceCgb,
                    _ => anyhow::bail!("invalid --mode value; expected auto|dmg|cgb"),
                });
                i += 2;
            }
            "--headless" => {
                headless_enabled = true;
                i += 1;
            }
            "--max-frames" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--max-frames requires a numeric value");
                };
                headless.max_frames = parse_u64_arg(value, "--max-frames")?;
                i += 2;
            }
            "--expect-serial" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--expect-serial requires a string value");
                };
                headless.expect_serial = Some(value.to_string());
                i += 2;
            }
            "--expect-test-pass" => {
                headless.expect_test_pass = true;
                i += 1;
            }
            "--trace-opcodes" => {
                headless.trace_opcodes = true;
                i += 1;
            }
            "--trace-opcode-limit" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-opcode-limit requires a numeric value");
                };
                headless.trace_opcode_limit = parse_u64_arg(value, "--trace-opcode-limit")?;
                i += 2;
            }
            "--trace-max-ops" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-max-ops requires a numeric value");
                };
                headless.trace_opcode_limit = parse_u64_arg(value, "--trace-max-ops")?;
                i += 2;
            }
            "--trace-start-t" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-start-t requires a numeric value");
                };
                headless.trace_start_t = parse_u64_arg(value, "--trace-start-t")?;
                i += 2;
            }
            "--trace-pc-range" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-pc-range requires start-end");
                };
                headless.trace_pc_range = Some(parse_pc_range_arg(value)?);
                i += 2;
            }
            "--trace-opcode" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-opcode requires a value");
                };
                for raw in value.split(',') {
                    let opcode = parse_u8_arg(raw, "--trace-opcode")?;
                    if !headless.trace_opcode_filter.contains(&opcode) {
                        headless.trace_opcode_filter.push(opcode);
                    }
                }
                i += 2;
            }
            "--trace-watch-interrupts" => {
                headless.trace_watch_interrupts = true;
                i += 1;
            }
            "--trace-bus" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-bus requires address ranges");
                };
                headless.trace_bus_filters.extend(parse_addr_range_list_arg(
                    value,
                    "--trace-bus",
                    HeadlessBusTraceAccess::ReadWrite,
                )?);
                i += 2;
            }
            "--trace-bus-read" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-bus-read requires address ranges");
                };
                headless.trace_bus_filters.extend(parse_addr_range_list_arg(
                    value,
                    "--trace-bus-read",
                    HeadlessBusTraceAccess::Read,
                )?);
                i += 2;
            }
            "--trace-bus-write" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-bus-write requires address ranges");
                };
                headless.trace_bus_filters.extend(parse_addr_range_list_arg(
                    value,
                    "--trace-bus-write",
                    HeadlessBusTraceAccess::Write,
                )?);
                i += 2;
            }
            "--trace-bus-limit" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--trace-bus-limit requires a numeric value");
                };
                headless.trace_bus_limit = parse_u64_arg(value, "--trace-bus-limit")?;
                i += 2;
            }
            "--dump-mem" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--dump-mem requires addr:len");
                };
                headless
                    .memory_dumps
                    .push(parse_memory_dump_arg(value, "--dump-mem")?);
                i += 2;
            }
            "--break-at" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--break-at requires an address value");
                };
                headless.break_at = Some(parse_u16_arg(value, "--break-at")?);
                i += 2;
            }
            "--no-apu" => {
                headless.no_apu = true;
                i += 1;
            }
            "--no-sram" | "--no-battery" => {
                headless.no_sram = true;
                i += 1;
            }
            "--gb-dmg-palette" | "--dmg-palette" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!(
                        "{} requires one of: gray|dmg-green|pocket|mint|chocolate",
                        args[i]
                    );
                };
                headless.gb_dmg_palette_preset = Some(parse_dmg_palette_arg(value, &args[i])?);
                i += 2;
            }
            "--detect-stuck" => {
                if headless.stuck_window_frames == 0 {
                    headless.stuck_window_frames = 120;
                }
                i += 1;
            }
            "--stuck-window" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--stuck-window requires a numeric value");
                };
                headless.stuck_window_frames = parse_u64_arg(value, "--stuck-window")?;
                i += 2;
            }
            "--stuck-pc-threshold" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--stuck-pc-threshold requires a numeric value");
                };
                headless.stuck_pc_threshold = parse_usize_arg(value, "--stuck-pc-threshold")?;
                i += 2;
            }
            "--fail-on-stuck" => {
                headless.fail_on_stuck = true;
                i += 1;
            }
            "--press" | "--input" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires an input spec", args[i]);
                };
                headless
                    .input_events
                    .extend(parse_input_event_arg(value, args[i].as_str())?);
                i += 2;
            }
            "--input-script" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--input-script requires a file path");
                };
                headless.input_events.extend(parse_input_script(value)?);
                i += 2;
            }
            "--load-state" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--load-state requires a file path");
                };
                headless.load_state_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--screenshot" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--screenshot requires a file path");
                };
                headless.screenshot_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--screenshot-frame" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--screenshot-frame requires a numeric value");
                };
                headless.screenshot_frame = Some(parse_u64_arg(value, "--screenshot-frame")?);
                i += 2;
            }
            "--screenshot-dir" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--screenshot-dir requires a directory path");
                };
                headless.screenshot_dir = Some(PathBuf::from(value));
                i += 2;
            }
            "--screenshot-every" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--screenshot-every requires a numeric frame interval");
                };
                headless.screenshot_every = parse_u64_arg(value, "--screenshot-every")?;
                i += 2;
            }
            "--debug-state" => {
                headless.print_debug_state = true;
                i += 1;
            }
            "--debug-state-out" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--debug-state-out requires a file path");
                };
                headless.debug_state_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--audio-dump" | "--audio-out" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("{} requires a file path", args[i]);
                };
                headless.audio_dump_path = Some(PathBuf::from(value));
                i += 2;
            }
            "--break-on-gba-bad-state" => {
                headless.break_on_gba_bad_state = true;
                i += 1;
            }
            "--gba-mute-audio" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--gba-mute-audio requires a comma-separated channel list");
                };
                let mutes = parse_gba_audio_mute_list_arg(value, "--gba-mute-audio")?;
                for (dst, src) in headless.gba_audio_mutes.iter_mut().zip(mutes) {
                    *dst |= src;
                }
                i += 2;
            }
            "--gba-hide-bg" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--gba-hide-bg requires a comma-separated BG layer list");
                };
                let hidden = parse_gba_bg_layer_list_arg(value, "--gba-hide-bg")?;
                for (dst, src) in headless.gba_hidden_bg_layers.iter_mut().zip(hidden) {
                    *dst |= src;
                }
                i += 2;
            }
            "--gba-hide-sprites" => {
                headless.gba_hide_sprites = true;
                i += 1;
            }
            "--gba-dump-memory" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("--gba-dump-memory requires a directory path");
                };
                headless.gba_dump_memory_dir = Some(PathBuf::from(value));
                i += 2;
            }
            other => {
                if rom_path.is_none() {
                    rom_path = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    Ok(CliArgs {
        rom_path,
        mode_override,
        headless: if headless_enabled {
            Some(headless)
        } else {
            None
        },
    })
}
