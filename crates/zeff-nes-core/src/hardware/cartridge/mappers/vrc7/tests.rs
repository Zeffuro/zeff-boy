use super::*;

fn make_vrc7(prg_size: usize, chr_size: usize) -> Vrc7 {
    Vrc7::new(
        vec![0u8; prg_size],
        vec![0u8; chr_size],
        Mirroring::Vertical,
        0x2000,
        false,
    )
}

#[test]
fn prg_banking() {
    let mut prg = vec![0u8; 8 * 0x2000];
    for b in 0..8 {
        prg[b * 0x2000] = b as u8;
    }
    let mut m = Vrc7::new(prg, vec![0u8; 0x2000], Mirroring::Vertical, 0x2000, false);
    m.cpu_write(0x8000, 3);
    assert_eq!(m.cpu_peek(0x8000), 3);
    m.cpu_write(0x8010, 5);
    assert_eq!(m.cpu_peek(0xA000), 5);
    m.cpu_write(0x9000, 2);
    assert_eq!(m.cpu_peek(0xC000), 2);
}

#[test]
fn fixed_last_bank() {
    let mut prg = vec![0u8; 4 * 0x2000];
    prg[3 * 0x2000] = 0xBB;
    let m = Vrc7::new(prg, vec![0u8; 0x2000], Mirroring::Vertical, 0x2000, false);
    assert_eq!(m.cpu_peek(0xE000), 0xBB);
}

#[test]
fn chr_banking() {
    let mut chr = vec![0u8; 256 * 0x0400];
    chr[42 * 0x0400] = 0xDD;
    let mut m = Vrc7::new(vec![0u8; 0x8000], chr, Mirroring::Vertical, 0x2000, false);
    m.cpu_write(0xA000, 42);
    assert_eq!(m.chr_read(0x0000), 0xDD);
}

#[test]
fn mirroring_control() {
    let mut m = make_vrc7(0x8000, 0x2000);
    assert_eq!(m.mirroring(), Mirroring::Vertical);
    m.cpu_write(0xE000, 0x01);
    assert_eq!(m.mirroring(), Mirroring::Horizontal);
    m.cpu_write(0xE000, 0x02);
    assert_eq!(m.mirroring(), Mirroring::SingleScreenLower);
}

#[test]
fn wram_enable() {
    let mut m = make_vrc7(0x8000, 0x2000);
    m.cpu_write(0x6000, 0x42);
    assert_eq!(m.cpu_peek(0x6000), 0, "wram disabled by default");
    m.cpu_write(0xE000, 0x80);
    m.cpu_write(0x6000, 0x42);
    assert_eq!(m.cpu_peek(0x6000), 0x42);
}

#[test]
fn irq_fires() {
    let mut m = make_vrc7(0x8000, 0x2000);
    m.cpu_write(0xE010, 0xFE);
    m.cpu_write(0xF000, 0x06);
    assert!(!m.irq_pending());
    m.clock_cpu();
    assert!(!m.irq_pending());
    m.clock_cpu();
    assert!(m.irq_pending());
}

#[test]
fn irq_acknowledge() {
    let mut m = make_vrc7(0x8000, 0x2000);
    m.cpu_write(0xE010, 0xFF);
    m.cpu_write(0xF000, 0x06);
    m.clock_cpu();
    assert!(m.irq_pending());
    m.cpu_write(0xF010, 0);
    assert!(!m.irq_pending());
}

#[test]
fn opll_register_write() {
    let mut m = make_vrc7(0x8000, 0x2000);
    m.cpu_write(0x9010, 0x30);
    m.cpu_write(0x9030, 0xF5);
    assert_eq!(m.audio.channels[0].instrument, 0x0F);
    assert_eq!(m.audio.channels[0].volume, 0x05);
}

#[test]
fn opll_audio_output_with_keyon() {
    let mut m = make_vrc7(0x8000, 0x2000);
    m.cpu_write(0x9010, 0x30);
    m.cpu_write(0x9030, 0x10);
    m.cpu_write(0x9010, 0x10);
    m.cpu_write(0x9030, 0x80);
    m.cpu_write(0x9010, 0x20);
    m.cpu_write(0x9030, 0x15);
    for _ in 0..500 {
        m.clock_cpu();
    }
    assert!(m.audio_output().abs() > 0.0);
}

#[test]
fn opll_key_on_off_state() {
    let mut m = make_vrc7(0x8000, 0x2000);
    m.cpu_write(0x9010, 0x20);
    m.cpu_write(0x9030, 0x15);
    assert!(m.audio.channels[0].key_on);
    m.cpu_write(0x9010, 0x20);
    m.cpu_write(0x9030, 0x04);
    assert!(!m.audio.channels[0].key_on);
}

#[test]
fn battery_save_roundtrip() {
    let mut m = Vrc7::new(
        vec![0u8; 0x8000],
        vec![0u8; 0x2000],
        Mirroring::Vertical,
        0x2000,
        true,
    );
    m.wram_enable = true;
    m.cpu_write(0x6000, 0xAA);
    m.cpu_write(0x6001, 0xBB);
    let sav = m.dump_battery_data().unwrap();
    assert_eq!(sav[0], 0xAA);
    assert_eq!(sav[1], 0xBB);

    let mut m2 = Vrc7::new(
        vec![0u8; 0x8000],
        vec![0u8; 0x2000],
        Mirroring::Vertical,
        0x2000,
        true,
    );
    m2.wram_enable = true;
    m2.load_battery_data(&sav).unwrap();
    assert_eq!(m2.cpu_peek(0x6000), 0xAA);
    assert_eq!(m2.cpu_peek(0x6001), 0xBB);
}
