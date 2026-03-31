use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::types::{CliArgs, HeadlessOptions};

fn parse_u64_arg(value: &str, flag: &str) -> anyhow::Result<u64> {
    value
        .parse::<u64>()
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
