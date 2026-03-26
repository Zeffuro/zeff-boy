use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::types::{CliArgs, HeadlessOptions};

fn parse_u64_arg(value: &str, flag: &str) -> Result<u64, Box<dyn std::error::Error>> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{} must be an unsigned integer", flag).into())
}

fn parse_u16_arg(value: &str, flag: &str) -> Result<u16, Box<dyn std::error::Error>> {
    let trimmed = value.trim();
    let parsed = if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16)
    } else {
        trimmed.parse::<u16>()
    };
    parsed.map_err(|_| format!("{} must be a u16 value (decimal or 0x-prefixed hex)", flag).into())
}

fn parse_u8_arg(value: &str, flag: &str) -> Result<u8, Box<dyn std::error::Error>> {
    let parsed = parse_u16_arg(value, flag)?;
    u8::try_from(parsed).map_err(|_| format!("{} value must fit in u8", flag).into())
}

fn parse_pc_range_arg(value: &str) -> Result<(u16, u16), Box<dyn std::error::Error>> {
    let Some((start_raw, end_raw)) = value.split_once('-') else {
        return Err(
            "--trace-pc-range must be start-end (decimal or hex, e.g. 0x0100-0x01FF)".into(),
        );
    };
    let start = parse_u16_arg(start_raw, "--trace-pc-range")?;
    let end = parse_u16_arg(end_raw, "--trace-pc-range")?;
    if start > end {
        return Err("--trace-pc-range start must be <= end".into());
    }
    Ok((start, end))
}

pub(crate) fn parse_args() -> Result<CliArgs, Box<dyn std::error::Error>> {
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
                    return Err("--mode requires one of: auto|dmg|cgb".into());
                };
                mode_override = Some(match value.as_str() {
                    "auto" => HardwareModePreference::Auto,
                    "dmg" => HardwareModePreference::ForceDmg,
                    "cgb" => HardwareModePreference::ForceCgb,
                    _ => return Err("invalid --mode value; expected auto|dmg|cgb".into()),
                });
                i += 2;
            }
            "--headless" => {
                headless_enabled = true;
                i += 1;
            }
            "--max-frames" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--max-frames requires a numeric value".into());
                };
                headless.max_frames = parse_u64_arg(value, "--max-frames")?;
                i += 2;
            }
            "--expect-serial" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--expect-serial requires a string value".into());
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
                    return Err("--trace-opcode-limit requires a numeric value".into());
                };
                headless.trace_opcode_limit = parse_u64_arg(value, "--trace-opcode-limit")?;
                i += 2;
            }
            "--trace-max-ops" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--trace-max-ops requires a numeric value".into());
                };
                headless.trace_opcode_limit = parse_u64_arg(value, "--trace-max-ops")?;
                i += 2;
            }
            "--trace-start-t" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--trace-start-t requires a numeric value".into());
                };
                headless.trace_start_t = parse_u64_arg(value, "--trace-start-t")?;
                i += 2;
            }
            "--trace-pc-range" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--trace-pc-range requires start-end".into());
                };
                headless.trace_pc_range = Some(parse_pc_range_arg(value)?);
                i += 2;
            }
            "--trace-opcode" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--trace-opcode requires a value".into());
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
            "--break-at" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--break-at requires an address value".into());
                };
                headless.break_at = Some(parse_u16_arg(value, "--break-at")?);
                i += 2;
            }
            "--no-apu" => {
                headless.no_apu = true;
                i += 1;
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
