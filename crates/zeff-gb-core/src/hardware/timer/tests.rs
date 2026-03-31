use super::*;
use crate::save_state::{StateReader, StateWriter};

fn make_timer() -> Timer {
    let mut t = Timer::new();
    t.set_mode(HardwareMode::DMG);
    t
}

#[test]
fn div_increments_every_256_t_cycles() {
    let mut t = make_timer();
    t.reset_div();
    assert_eq!(t.div(), 0);

    t.step(255);
    assert_eq!(t.div(), 0);

    t.step(1);
    assert_eq!(t.div(), 1);

    t.step(256);
    assert_eq!(t.div(), 2);
}

#[test]
fn reset_div_clears_sys_counter() {
    let mut t = make_timer();
    t.step(512);
    assert!(t.div() > 0);

    t.reset_div();
    assert_eq!(t.div(), 0);
}

#[test]
fn tima_does_not_increment_when_disabled() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x00);
    t.step(1024);
    assert_eq!(t.tima(), 0);
}

#[test]
fn tima_increments_at_clock_rate_div4() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x05);
    t.step(15);
    assert_eq!(t.tima(), 0);
    t.step(1);
    assert_eq!(t.tima(), 1);
}

#[test]
fn tima_overflow_reloads_from_tma_and_fires_interrupt() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x05);
    t.set_tima_raw(0xFF);
    t.set_tma_raw(0x42);
    let irq = t.step(20);
    assert_eq!(t.tima(), 0x42);
    assert!(
        irq,
        "timer overflow should generate interrupt after 4-cycle delay"
    );
}

#[test]
fn tima_overflow_reads_zero_during_delay() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x05);
    t.set_tima_raw(0xFF);
    t.set_tma_raw(0x10);
    for _ in 0..15 {
        assert!(!t.step(1));
    }
    assert_eq!(t.tima(), 0xFF);
    assert!(!t.step(1));
    assert_eq!(t.tima(), 0x00, "TIMA should read 0 during overflow delay");
    assert!(!t.step(1));
    assert_eq!(t.tima(), 0x00);
    assert!(!t.step(1));
    assert_eq!(t.tima(), 0x00);
    assert!(!t.step(1));
    assert_eq!(t.tima(), 0x00);
    let irq = t.step(1);
    assert!(irq, "interrupt should fire after 4-cycle delay");
    assert_eq!(t.tima(), 0x10, "TIMA should be reloaded from TMA");
}

#[test]
fn write_tima_cancels_pending_overflow() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x05);
    t.set_tima_raw(0xFF);
    t.set_tma_raw(0x20);
    t.step(16); // Trigger overflow:TIMA=0, delay=4
    assert_eq!(t.tima(), 0x00, "TIMA should be 0 during delay");
    t.write_tima(0x50); // Cancel the pending reload
    assert_eq!(t.tima(), 0x50);
    let irq = t.step(1);
    assert!(!irq, "interrupt should be cancelled by TIMA write");
    assert_ne!(t.tima(), 0x20, "TMA reload should be cancelled");
}

#[test]
fn tac_glitch_falling_edge_increments_tima() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x05);
    t.step(8);
    assert_eq!(t.tima(), 0);
    let tima_before = t.tima();
    t.write_tac(0x00);
    assert_eq!(t.tima(), tima_before + 1);
}

#[test]
fn reset_div_falling_edge_increments_tima() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x04);
    t.step(512);
    assert_eq!(t.tima(), 0);
    let tima_before = t.tima();
    t.reset_div();
    assert_eq!(t.tima(), tima_before + 1);
}

#[test]
fn div_wraps_around_at_255() {
    let mut t = make_timer();
    t.reset_div();
    t.step(255 * 256);
    assert_eq!(t.div(), 255);
    t.step(256);
    assert_eq!(t.div(), 0);
}

#[test]
fn save_state_roundtrip() {
    let mut t = make_timer();
    t.write_tac(0x07);
    t.set_tma_raw(0x42);
    t.step(100);

    let mut writer = StateWriter::new();
    t.write_state(&mut writer);
    let bytes = writer.into_bytes();

    let mut reader = StateReader::new(&bytes);
    let restored = Timer::read_state(&mut reader).expect("restore should succeed");

    assert_eq!(restored.div(), t.div());
    assert_eq!(restored.tima(), t.tima());
    assert_eq!(restored.tma(), t.tma());
    assert_eq!(restored.tac(), t.tac());
}

#[test]
fn tac_write_during_pending_overflow_does_not_cancel() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x05);
    t.set_tima_raw(0xFF);
    t.set_tma_raw(0x30);
    t.step(16);
    assert_eq!(t.tima(), 0x00, "TIMA should be 0 during overflow delay");
    t.write_tac(0x07);
    let irq = t.step(4);
    assert!(irq, "overflow interrupt should still fire after TAC write");
    assert_eq!(
        t.tima(),
        0x30,
        "TMA reload should still happen after TAC write"
    );
}

#[test]
fn div_reset_during_pending_overflow_does_not_cancel() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x05);
    t.set_tima_raw(0xFF);
    t.set_tma_raw(0x25);
    t.step(16);
    assert_eq!(t.tima(), 0x00, "TIMA should be 0 during overflow delay");
    t.reset_div();
    let irq = t.step(4);
    assert!(irq, "overflow interrupt should still fire after DIV reset");
    assert_eq!(t.tima(), 0x25, "TMA reload should happen after DIV reset");
}

#[test]
fn overflow_with_tma_fe_cascades_on_second_tick() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x05);
    t.set_tima_raw(0xFF);
    t.set_tma_raw(0xFE);
    t.step(16);
    assert_eq!(t.tima(), 0x00, "TIMA should read 0 during delay");

    let irq1 = t.step(4);
    assert!(irq1, "first overflow interrupt");
    assert_eq!(t.tima(), 0xFE, "TIMA should reload to TMA (0xFE)");
    t.step(12);
    assert_eq!(t.tima(), 0xFF, "TIMA should be 0xFF after one increment");

    let irq2 = t.step(20);
    assert!(irq2, "second overflow interrupt should fire");
    assert_eq!(t.tima(), 0xFE, "TIMA should reload to TMA (0xFE) again");
}

#[test]
fn tma_write_during_overflow_delay_uses_new_value() {
    let mut t = make_timer();
    t.reset_div();
    t.write_tac(0x05);
    t.set_tima_raw(0xFF);
    t.set_tma_raw(0x10);
    t.step(16);
    assert_eq!(t.tima(), 0x00, "TIMA should be 0 during delay");
    t.write_tma(0x42);

    let irq = t.step(4);
    assert!(irq, "interrupt should fire");
    assert_eq!(t.tima(), 0x42, "TIMA should reload from new TMA value");
}
