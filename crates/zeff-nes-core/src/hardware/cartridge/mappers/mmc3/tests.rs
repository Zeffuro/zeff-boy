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

#[test]
fn ppu_a12_rising_edge_clocks_irq_counter_once() {
    let mut mapper = Mmc3::new(
        vec![0u8; 4 * 0x2000],
        vec![0u8; 0x2000],
        Mirroring::Horizontal,
    );
    mapper.cpu_write(0xC000, 0x01);
    mapper.cpu_write(0xC001, 0x00);
    mapper.cpu_write(0xE001, 0x00);

    mapper.notify_ppu_a12(true);
    assert!(!mapper.irq_pending());

    mapper.notify_ppu_a12(true);
    assert!(!mapper.irq_pending(), "held-high A12 must not double-clock");

    mapper.notify_ppu_a12(false);
    mapper.notify_ppu_a12(true);
    assert!(mapper.irq_pending());
}

#[test]
fn irq_latch_zero_sets_irq_on_every_a12_clock() {
    let mut mapper = Mmc3::new(
        vec![0u8; 4 * 0x2000],
        vec![0u8; 0x2000],
        Mirroring::Horizontal,
    );
    mapper.cpu_write(0xC000, 0x00);
    mapper.cpu_write(0xC001, 0x00);
    mapper.cpu_write(0xE001, 0x00);

    mapper.notify_ppu_a12(true);
    assert!(mapper.irq_pending());

    mapper.cpu_write(0xE000, 0x00);
    mapper.cpu_write(0xE001, 0x00);
    mapper.notify_ppu_a12(false);
    mapper.notify_ppu_a12(true);
    assert!(mapper.irq_pending());
}

#[test]
fn irq_latch_zero_does_not_refire_immediately_after_decrement_to_zero() {
    let mut mapper = Mmc3::new(
        vec![0u8; 4 * 0x2000],
        vec![0u8; 0x2000],
        Mirroring::Horizontal,
    );
    mapper.cpu_write(0xC000, 0x01);
    mapper.cpu_write(0xC001, 0x00);
    mapper.cpu_write(0xE001, 0x00);

    mapper.notify_ppu_a12(true);
    assert!(!mapper.irq_pending());

    mapper.cpu_write(0xC000, 0x00);
    mapper.notify_ppu_a12(false);
    mapper.notify_ppu_a12(true);
    assert!(mapper.irq_pending());

    mapper.cpu_write(0xE000, 0x00);
    mapper.cpu_write(0xE001, 0x00);
    mapper.notify_ppu_a12(false);
    mapper.notify_ppu_a12(true);
    assert!(!mapper.irq_pending());

    mapper.cpu_write(0xC000, 0x00);
    mapper.notify_ppu_a12(false);
    mapper.notify_ppu_a12(true);
    assert!(mapper.irq_pending());
}
