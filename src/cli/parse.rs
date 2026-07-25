use std::path::PathBuf;

use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::types::{
    CliArgs, HeadlessBusTraceAccess, HeadlessBusTraceFilter, HeadlessInputEvent, HeadlessOptions,
};

fn parse_u64_arg(value: &str, flag: &str) -> anyhow::Result<u64> {
    value
        .parse::<u64>()
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

fn parse_addr_arg(value: &str, flag: &str) -> anyhow::Result<u16> {
    let trimmed = value.trim();
    let trimmed = trimmed.strip_prefix('$').unwrap_or(trimmed);
    let parsed = if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16)
    } else if trimmed.len() == 4
        || trimmed
            .chars()
            .any(|c| c.is_ascii_hexdigit() && !c.is_ascii_digit())
    {
        u16::from_str_radix(trimmed, 16)
    } else {
        trimmed.parse::<u16>()
    };
    parsed.map_err(|_| anyhow::anyhow!("{} must be a u16 address", flag))
}

fn parse_pc_range_arg(value: &str) -> anyhow::Result<(u16, u16)> {
    let Some((start_raw, end_raw)) = value.split_once('-') else {
        anyhow::bail!("--trace-pc-range must be start-end (decimal or hex, e.g. 0x0100-0x01FF)",);
    };
    let start = parse_u16_arg(start_raw, "--trace-pc-range")?;
    let end = parse_u16_arg(end_raw, "--trace-pc-range")?;
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
