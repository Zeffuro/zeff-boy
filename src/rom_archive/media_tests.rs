use std::io::{Cursor, Read};
use std::path::Path;

use super::*;

fn md_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x200];
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    bytes
}

#[test]
fn binary_probe_reads_only_the_console_prefix() -> anyhow::Result<()> {
    let mut reader = Cursor::new(md_rom());
    assert_eq!(
        probe_rom_reader(Path::new("game.bin"), &mut reader)?,
        Some(ActiveSystem::MegaDrive)
    );
    assert_eq!(
        reader.position(),
        zeff_emu_common::system::MEDIA_PROBE_LEN as u64
    );
    let mut reader = Cursor::new(md_rom());
    assert_eq!(
        probe_rom_reader(Path::new("game.nes"), &mut reader)?,
        Some(ActiveSystem::Nes)
    );
    assert_eq!(reader.position(), 0);
    let mut reader = Cursor::new(md_rom()).take(0x10f);
    assert_eq!(probe_rom_reader(Path::new("game.bin"), &mut reader)?, None);
    Ok(())
}

#[test]
fn authenticated_single_zip_counts_signed_binary_roms_but_not_disc_tracks() -> anyhow::Result<()> {
    let directory = crate::test_support::test_directory("md-gba-authenticated-selection")?;
    let archive = directory.path().join("mixed.zip");
    let rom = md_rom();
    crate::test_support::write_zip(&archive, &[("game.gba", &[0; 0xc0]), ("other.bin", &rom)])?;
    assert!(extract_single_authenticated_bounded_zip_member(&archive, "gba", 4096, 1024).is_err());
    assert!(
        extract_authenticated_bounded_zip_member(
            &archive,
            Some(&archive.join("game.gba")),
            "gba",
            4096,
            1024
        )
        .is_ok()
    );
    crate::test_support::write_zip(
        &archive,
        &[("game.gba", &[0; 0xc0]), ("track.bin", &[0; 0x200])],
    )?;
    assert!(extract_single_authenticated_bounded_zip_member(&archive, "gba", 4096, 1024).is_ok());
    Ok(())
}
