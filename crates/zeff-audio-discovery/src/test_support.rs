pub mod camelot;
pub mod engine_software;
pub mod gax;
pub mod gb_music;
pub mod nes_music;
pub mod rips;
pub mod sample;
pub mod tracker;
pub mod vgm;

pub fn put_word(rom: &mut [u8], at: usize, value: u32) {
    rom[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn collection(rom: &mut [u8], base: usize) {
    rom[base..base + 4].copy_from_slice(&[2, 0, 5, 0x80]);
    put_word(rom, base + 4, 0x0800_0000 + (base + 0x100) as u32);
    put_word(rom, base + 8, 0x0800_0000 + (base + 0x300) as u32);
    put_word(rom, base + 12, 0x0800_0000 + (base + 0x340) as u32);
    let tone = base + 0x100;
    rom[tone..tone + 4].copy_from_slice(&[0, 60, 0, 0]);
    put_word(rom, tone + 4, 0x0800_0000 + (base + 0x200) as u32);
    rom[tone + 8..tone + 12].copy_from_slice(&[255, 180, 128, 100]);
    let sample = base + 0x200;
    put_word(rom, sample, 0x4000_0000);
    put_word(rom, sample + 4, 8_000 * 1024);
    put_word(rom, sample + 8, 2);
    put_word(rom, sample + 12, 5);
    rom[sample + 16..sample + 21].copy_from_slice(&[0, 127, 0, 128, 255]);
    for track in [base + 0x300, base + 0x340] {
        rom[track..track + 11]
            .copy_from_slice(&[0xBD, 0, 0xBB, 60, 0xBE, 100, 0xD0, 60, 100, 0x81, 0xB1]);
    }
}

pub fn fixture() -> Vec<u8> {
    let mut rom = vec![0; 0x1000];
    collection(&mut rom, 0x100);
    rom
}

pub fn gba_fixture() -> Vec<u8> {
    let mut rom = fixture();
    put_word(&mut rom, 0, 0xEAFF_FFFE);
    rom[0xB2] = 0x96;
    rom
}
