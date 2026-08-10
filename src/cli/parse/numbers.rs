pub(super) fn parse_u64_arg(value: &str, flag: &str) -> anyhow::Result<u64> {
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

pub(super) fn parse_usize_arg(value: &str, flag: &str) -> anyhow::Result<usize> {
    value
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("{} must be an unsigned integer", flag))
}

pub(super) fn parse_u16_arg(value: &str, flag: &str) -> anyhow::Result<u16> {
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

pub(super) fn parse_u8_arg(value: &str, flag: &str) -> anyhow::Result<u8> {
    let parsed = parse_u16_arg(value, flag)?;
    u8::try_from(parsed).map_err(|_| anyhow::anyhow!("{} value must fit in u8", flag))
}

pub(super) fn parse_addr_arg(value: &str, flag: &str) -> anyhow::Result<u64> {
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

pub(super) fn parse_pc_range_arg(value: &str) -> anyhow::Result<(u64, u64)> {
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

pub(super) fn parse_frame_range_arg(value: &str, flag: &str) -> anyhow::Result<(u64, u64)> {
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
