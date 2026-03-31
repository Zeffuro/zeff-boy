use super::*;

#[test]
fn mmc3_switches_8k_prg_bank_at_8000() {
    let mut prg = vec![0u8; 4 * 0x2000];
    for bank in 0..4usize {
        prg[bank * 0x2000] = bank as u8;
    }
    let chr = vec![0u8; 0x2000];

    let mut mapper = Mmc3::new(prg, chr, Mirroring::Horizontal);
    mapper.cpu_write(0x8000, 0x06);
    mapper.cpu_write(0x8001, 0x01);

    assert_eq!(mapper.cpu_read(0x8000), 1);
}
