use super::*;
use crate::hardware::cartridge::Mirroring;

fn make_test_mapper() -> Namco163 {
    let prg = vec![0xEA; 0x8000];
    let chr = vec![0; 0x2000];
    Namco163::new(prg, chr, Mirroring::Vertical, 0x2000, false)
}

#[test]
fn prg_banking_selects_correct_bank() {
    let mut m = make_test_mapper();
    let prg_size = m.prg_rom.len();
    for i in 0..4 {
        m.prg_rom[i * 0x2000] = (i + 1) as u8;
    }

    m.cpu_write(0xE000, 0);
    assert_eq!(m.cpu_peek(0x8000), 1);
    m.cpu_write(0xE000, 1);
    assert_eq!(m.cpu_peek(0x8000), 2);

    m.cpu_write(0xE800, 2);
    assert_eq!(m.cpu_peek(0xA000), 3);

    m.cpu_write(0xF000, 0);
    assert_eq!(m.cpu_peek(0xC000), 1);

    let _ = prg_size;
    assert_eq!(m.cpu_peek(0xE000), 4);
}

#[test]
fn chr_banking_selects_correct_bank() {
    let mut m = make_test_mapper();
    for i in 0..8 {
        m.chr[i * 0x0400] = (i + 1) as u8;
    }

    m.cpu_write(0x8000, 0);
    m.cpu_write(0x8800, 3);
    m.cpu_write(0x9000, 7);

    assert_eq!(m.chr_read(0x0000), 1);
    assert_eq!(m.chr_read(0x0400), 4);
    assert_eq!(m.chr_read(0x0800), 8);
}

#[test]
fn prg_ram_read_write() {
    let mut m = make_test_mapper();
    m.cpu_write(0x6000, 0x42);
    assert_eq!(m.cpu_peek(0x6000), 0x42);
    m.cpu_write(0x7FFF, 0xAB);
    assert_eq!(m.cpu_peek(0x7FFF), 0xAB);
}

#[test]
fn irq_counter_fires_at_max() {
    let mut m = make_test_mapper();
    m.cpu_write(0x5000, 0xFE);
    m.cpu_write(0x5800, 0xFF);
    assert_eq!(m.irq_counter, 0x7FFE);
    assert!(m.irq_enable);
    assert!(!m.irq_pending);

    m.clock_cpu();
    assert!(!m.irq_pending);
    assert_eq!(m.irq_counter, 0x7FFF);

    m.clock_cpu();
    assert!(m.irq_pending);
}

#[test]
fn irq_reading_counter_acknowledges() {
    let mut m = make_test_mapper();
    m.irq_pending = true;
    let _ = m.cpu_read(0x5000);
    assert!(!m.irq_pending);

    m.irq_pending = true;
    let _ = m.cpu_read(0x5800);
    assert!(!m.irq_pending);
}

#[test]
fn sound_ram_read_write_with_auto_increment() {
    let mut m = make_test_mapper();

    m.audio.write_addr(0x80);
    m.cpu_write(0x4800, 0x11);
    m.cpu_write(0x4800, 0x22);
    m.cpu_write(0x4800, 0x33);

    assert_eq!(m.audio.ram[0], 0x11);
    assert_eq!(m.audio.ram[1], 0x22);
    assert_eq!(m.audio.ram[2], 0x33);

    m.audio.write_addr(0x80);
    assert_eq!(m.cpu_read(0x4800), 0x11);
    assert_eq!(m.cpu_read(0x4800), 0x22);
    assert_eq!(m.cpu_read(0x4800), 0x33);
}

#[test]
fn sound_ram_no_auto_increment() {
    let mut m = make_test_mapper();
    m.audio.write_addr(0x05);
    m.cpu_write(0x4800, 0xAA);
    m.cpu_write(0x4800, 0xBB);
    assert_eq!(m.audio.ram[5], 0xBB);
}

#[test]
fn audio_output_nonzero_when_channel_active() {
    let mut m = make_test_mapper();
    m.audio.ram[0x7F] = 0x0F;

    for i in 0..8 {
        m.audio.ram[i] = 0xFF;
    }

    m.audio.ram[0x78] = 0x00;
    m.audio.ram[0x79] = 0x00;
    m.audio.ram[0x7A] = 0x10;
    m.audio.ram[0x7B] = 0x00;
    m.audio.ram[0x7C] = 60 << 2;
    m.audio.ram[0x7D] = 0x00;
    m.audio.ram[0x7E] = 0x00;

    for _ in 0..100 {
        m.audio.tick();
    }

    assert!(m.audio.output() > 0.0, "expected nonzero audio output");
}

#[test]
fn audio_silent_when_disabled() {
    let mut m = make_test_mapper();
    m.cpu_write(0xE000, 0x40);
    assert!(!m.sound_enabled);

    for _ in 0..100 {
        m.clock_cpu();
    }
    assert_eq!(m.audio_output(), 0.0);
}

#[test]
fn nametable_uses_ciram_when_bank_ge_e0() {
    let mut m = make_test_mapper();
    let mut ciram = [0u8; 0x800];

    m.cpu_write(0xC000, 0xE0);
    ciram[0x42] = 0xAB;

    let val = m.ppu_nametable_read(0x2042, &ciram);
    assert_eq!(val, Some(0xAB));

    m.cpu_write(0xC800, 0xE1);
    ciram[0x400 + 0x10] = 0xCD;

    let val = m.ppu_nametable_read(0x2410, &ciram);
    assert_eq!(val, Some(0xCD));
}

#[test]
fn battery_save_data_roundtrip() {
    let mut m = make_test_mapper();
    m.has_battery = true;
    m.cpu_write(0x6000, 0xDE);
    m.cpu_write(0x6001, 0xAD);

    let data = m.dump_battery_data().expect("should have battery data");
    assert_eq!(data[0], 0xDE);
    assert_eq!(data[1], 0xAD);

    let mut m2 = make_test_mapper();
    m2.has_battery = true;
    m2.load_battery_data(&data).unwrap();
    assert_eq!(m2.cpu_peek(0x6000), 0xDE);
    assert_eq!(m2.cpu_peek(0x6001), 0xAD);
}
