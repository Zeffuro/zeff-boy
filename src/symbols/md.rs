use super::ExecMode;

pub(super) fn normalize_mode(mode: &mut ExecMode) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(mode, ExecMode::Unknown | ExecMode::M68000),
        "Mega Drive symbols require M68000 execution mode"
    );
    *mode = ExecMode::M68000;
    Ok(())
}

pub(super) fn validate_cpu_range(address: u64, size: Option<u64>) -> anyhow::Result<()> {
    anyhow::ensure!(
        address <= 0xFF_FFFF && size.unwrap_or(1).max(1) - 1 <= 0xFF_FFFF - address,
        "symbol exceeds the 24-bit Mega Drive CPU address space"
    );
    Ok(())
}
