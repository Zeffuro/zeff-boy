use super::*;
use crate::emulator::Emulator;

fn large_chr_mmc3() -> Emulator {
    let mut rom = vec![0; 16 + 0x8000 + 0x20000];
    rom[..4].copy_from_slice(b"NES\x1a");
    rom[4] = 2;
    rom[5] = 16;
    rom[6] = 0x40;
    rom[16..19].copy_from_slice(&[0x4c, 0x00, 0x80]);
    rom[16 + 0x7ffc..16 + 0x7ffe].copy_from_slice(&[0x00, 0x80]);
    for (index, byte) in rom[16 + 0x8000..].iter_mut().enumerate() {
        *byte = ((index.wrapping_mul(73) ^ (index >> 7)) & 255) as u8;
    }
    Emulator::new(&rom, 48_000.0).unwrap()
}

#[test]
fn netplay_hash_payload_matches_large_projected_state_without_compression() {
    let mut emu = large_chr_mmc3();
    for _ in 0..3 {
        emu.step_frame();
        let mut projected = encode_state(&emu).unwrap();
        project_replay_state_bytes(&mut projected).unwrap();
        let payload = lz4_flex::decompress_size_prepended(&projected[12..]).unwrap();
        assert!(payload.len() > 65535);
        let raw = encode_netplay_hash_state(&emu);
        assert_eq!(&raw[..8], b"ZBNHASH\0");
        assert_eq!(&raw[8..12], NES_SAVE_STATE_FORMAT_VERSION.to_le_bytes());
        assert_eq!(&raw[12..], payload);
    }
}

#[test]
fn netplay_hash_excludes_output_and_retains_execution_changes() {
    let mut emu = large_chr_mmc3();
    let native = encode_state(&emu).unwrap();
    let raw = encode_netplay_hash_state(&emu);
    emu.bus.ppu.frame_ready = !emu.bus.ppu.frame_ready;
    emu.bus.ppu.framebuffer.fill(0x53);
    assert_ne!(encode_state(&emu).unwrap(), native);
    assert_eq!(encode_netplay_hash_state(&emu), raw);
    emu.cpu_write8(0x0003, 0x5a);
    assert_ne!(encode_netplay_hash_state(&emu), raw);
}
