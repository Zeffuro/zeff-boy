use super::*;

#[test]
fn fme7_switches_prg_bank_at_8000() {
    let mut prg = vec![0u8; 5 * 0x2000];
    for bank in 0..5usize {
        prg[bank * 0x2000] = bank as u8;
    }
    let chr = vec![0u8; 0x2000];
    let mut mapper = Fme7::new(prg, chr, Mirroring::Horizontal, 0x2000, false);

    mapper.cpu_write(0x8000, 0x09);
    mapper.cpu_write(0xA000, 0x03);

    assert_eq!(mapper.cpu_read(0x8000), 3);
}

#[test]
fn fme7_battery_dump_roundtrip() {
    let prg = vec![0u8; 5 * 0x2000];
    let chr = vec![0u8; 0x2000];
    let mut mapper = Fme7::new(prg, chr, Mirroring::Horizontal, 0x2000, true);

    mapper.cpu_write(0x8000, 0x08);
    mapper.cpu_write(0xA000, 0xC0);
    mapper.cpu_write(0x6000, 0x5A);

    let dump = mapper.dump_battery_data().unwrap();
    let mut restored = Fme7::new(vec![0u8; 5 * 0x2000], vec![0u8; 0x2000], Mirroring::Horizontal, 0x2000, true);
    restored.load_battery_data(&dump).unwrap();
    restored.cpu_write(0x8000, 0x08);
    restored.cpu_write(0xA000, 0xC0);

    assert_eq!(restored.cpu_read(0x6000), 0x5A);
}

