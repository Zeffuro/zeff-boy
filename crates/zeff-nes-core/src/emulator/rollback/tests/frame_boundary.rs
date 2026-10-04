use super::*;

#[test]
fn status_suppression_keeps_frame_notification_and_blocks_nmi() {
    for region in 0..3 {
        let mut subject = core(region, 34);
        subject.bus.ppu.scanline = subject.bus.timing.vblank_start_scanline();
        subject.bus.ppu.dot = 1;
        subject.bus.ppu.regs.ctrl = 0x80;
        assert_eq!(subject.bus.cpu_read(0x2002) & 0x80, 0);
        assert!(subject.bus.ppu.suppress_vblank_edge);
        let events = subject.bus.tick_peripherals(1);
        assert!(subject.frame_ready());
        assert!(!events.nmi_raised);
        assert_eq!(subject.ppu_status() & 0x80, 0);
        assert!(!subject.bus.ppu.nmi_output);
        assert!(!subject.bus.ppu.suppress_vblank_edge);
    }
}

#[test]
fn status_polling_preserves_each_regional_frame_and_corrected_trajectory() {
    for region in 0..3 {
        let mut bytes = fixture::rom(region, 34);
        let program = [
            0x78, 0xa9, 1, 0x8d, 0x15, 0x40, 0xa9, 0xbf, 0x8d, 0, 0x40, 0xa9, 0x60, 0x8d, 2, 0x40,
            0xa9, 8, 0x8d, 3, 0x40, 0xad, 2, 0x20, 0x10, 0xfb, 0xad, 2, 0x20, 0x10, 0xfb, 0xa9, 1,
            0x8d, 0x16, 0x40, 0xa9, 0, 0x8d, 0x16, 0x40, 0xad, 0x16, 0x40, 0x85, 0, 0x4c, 0x15,
            0x80,
        ];
        for bank in bytes[16..16 + 0x10000].as_chunks_mut::<0x8000>().0 {
            bank[..program.len()].copy_from_slice(&program);
            bank[0x7ffc..0x7ffe].copy_from_slice(&[0, 0x80]);
        }
        let mut control = Emulator::new(&bytes, 48_000.0).unwrap();
        let mut subject = Emulator::new(&bytes, 48_000.0).unwrap();
        let session = subject.begin_rollback_session().unwrap();
        for frame in 0..240 {
            let snapshot = session.capture(&subject).unwrap();
            session
                .advance_frame(&mut subject, input(frame + 17))
                .unwrap();
            session.restore(&mut subject, &snapshot).unwrap();
            let actual = session.advance_frame(&mut subject, input(frame)).unwrap();
            let expected = ordinary_frame(&mut control, input(frame));
            assert_eq!(subject.frame_count(), frame as u64 + 1);
            assert_eq!(control.frame_count(), frame as u64 + 1);
            assert_eq!(observe(&subject, actual), observe(&control, expected));
        }
    }
}
