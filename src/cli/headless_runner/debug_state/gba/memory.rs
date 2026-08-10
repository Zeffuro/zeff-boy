use std::path::Path;

use zeff_gba_core::emulator::Emulator as GbaEmulator;

pub(in crate::cli::headless_runner) fn dump_gba_memory_snapshots(
    emulator: &GbaEmulator,
    dir: &Path,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("vram.bin"), emulator.vram_snapshot())?;
    std::fs::write(dir.join("palette.bin"), emulator.palette_ram_snapshot())?;
    std::fs::write(dir.join("oam.bin"), emulator.oam_snapshot())?;
    std::fs::write(dir.join("io.bin"), emulator.io_snapshot())?;
    let (ewram, iwram) = emulator.system_ram();
    std::fs::write(dir.join("ewram.bin"), ewram)?;
    std::fs::write(dir.join("iwram.bin"), iwram)?;
    println!("[headless] gba-memory-dump={}", dir.display());
    Ok(())
}
