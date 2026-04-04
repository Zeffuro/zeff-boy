use super::*;

fn make_vrc6(prg_size: usize, chr_size: usize) -> Vrc6 {
    Vrc6::new(
        vec![0u8; prg_size],
        vec![0u8; chr_size],
        Mirroring::Vertical,
        false,
    )
}

#[test]
fn vrc6_prg_banking_16k() {
    let mut prg = vec![0u8; 4 * 0x4000];
    for bank in 0..4 {
        prg[bank * 0x4000] = bank as u8;
    }
    let mut m = Vrc6::new(prg, vec![0u8; 0x2000], Mirroring::Vertical, false);
    m.cpu_write(0x8000, 2);
    assert_eq!(m.cpu_peek(0x8000), 2);
}

#[test]
fn vrc6_prg_banking_8k() {
    let mut prg = vec![0u8; 8 * 0x2000];
    for bank in 0..8 {
        prg[bank * 0x2000] = bank as u8;
    }
    let mut m = Vrc6::new(prg, vec![0u8; 0x2000], Mirroring::Vertical, false);
    m.cpu_write(0xC000, 3);
    assert_eq!(m.cpu_peek(0xC000), 3);
}

#[test]
fn vrc6_fixed_last_bank() {
    let mut prg = vec![0u8; 4 * 0x2000];
    prg[3 * 0x2000] = 0xAA;
    let m = Vrc6::new(prg, vec![0u8; 0x2000], Mirroring::Vertical, false);
    assert_eq!(m.cpu_peek(0xE000), 0xAA);
}

#[test]
fn vrc6_chr_banking() {
    let mut chr = vec![0u8; 8 * 0x0400];
    chr[5 * 0x0400] = 0x77;
    let mut m = Vrc6::new(vec![0u8; 0x8000], chr, Mirroring::Vertical, false);
    m.cpu_write(0xD002, 5);
    assert_eq!(m.chr_read(0x0800), 0x77);
}

#[test]
fn vrc6_mirroring_control() {
    let mut m = make_vrc6(0x8000, 0x2000);
    assert_eq!(m.mirroring(), Mirroring::Vertical);
    m.cpu_write(0xB003, 0x04);
    assert_eq!(m.mirroring(), Mirroring::Horizontal);
    m.cpu_write(0xB003, 0x08);
    assert_eq!(m.mirroring(), Mirroring::SingleScreenLower);
}

#[test]
fn vrc6_pulse_output() {
    let mut m = make_vrc6(0x8000, 0x2000);
    m.cpu_write(0x9000, 0x80 | 10);
    m.cpu_write(0x9001, 1);
    m.cpu_write(0x9002, 0x80);
    for _ in 0..10 {
        m.audio.tick();
    }
    assert_eq!(m.audio.pulse1.output(), 10);
}

#[test]
fn vrc6_sawtooth_output() {
    let mut m = make_vrc6(0x8000, 0x2000);
    m.cpu_write(0xB000, 8);
    m.cpu_write(0xB001, 0);
    m.cpu_write(0xB002, 0x80);
    for _ in 0..20 {
        m.audio.tick();
    }
    assert!(m.audio.sawtooth.output() > 0);
}

#[test]
fn vrc6_audio_output_nonzero_when_active() {
    let mut m = make_vrc6(0x8000, 0x2000);
    m.cpu_write(0x9000, 0x80 | 15);
    m.cpu_write(0x9001, 1);
    m.cpu_write(0x9002, 0x80);
    for _ in 0..5 {
        m.audio.tick();
    }
    assert!(m.audio_output() > 0.0);
}

#[test]
fn vrc6_audio_halt_silences_output() {
    let mut m = make_vrc6(0x8000, 0x2000);
    m.cpu_write(0x9000, 0x80 | 15);
    m.cpu_write(0x9001, 1);
    m.cpu_write(0x9002, 0x80);
    for _ in 0..5 {
        m.audio.tick();
    }
    assert!(m.audio_output() > 0.0);
    m.cpu_write(0x9003, 0x04);
    let before = m.audio.pulse1.output();
    for _ in 0..10 {
        m.audio.tick();
    }
    assert_eq!(m.audio.pulse1.output(), before);
}

#[test]
fn vrc6b_address_swap() {
    let mut m = Vrc6::new(
        vec![0u8; 0x8000],
        vec![0u8; 0x2000],
        Mirroring::Vertical,
        true,
    );

    m.cpu_write(0x9000, 0x80 | 8);
    m.cpu_write(0x9002, 5);
    m.cpu_write(0x9001, 0x80);

    for _ in 0..5 {
        m.audio.tick();
    }
    assert_eq!(m.audio.pulse1.output(), 8);
}

