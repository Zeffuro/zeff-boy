use super::*;
use crate::save_state::{StateReader, StateWriter};

#[test]
fn loaded_large_cartridge_allocations_restore_and_replay() {
    for mapper in [0, 5, 34, 69, 85] {
        let mut bytes = generic_mappers::rom(mapper, true);
        bytes[7] |= 8;
        if mapper == 0 {
            bytes[5] = 128;
            bytes[9] = 0x10;
            bytes.resize(16 + 0x10000 + 3 * 1024 * 1024, 0);
        }
        bytes[10] = 14;
        let mut subject = Emulator::new(&bytes, 48_000.0).unwrap();
        let mut control = Emulator::new(&bytes, 48_000.0).unwrap();
        let session = subject.begin_rollback_session().unwrap();
        for core in [&mut subject, &mut control] {
            core.bus.cartridge.cpu_write(0x6000, 0x73);
        }
        let snapshot = session.capture(&subject).unwrap();
        let before = observe(&subject, Vec::new());
        let mut damaged = subject.encode_state().unwrap();
        let mut payload = lz4_flex::decompress_size_prepended(&damaged[12..]).unwrap();
        payload.push(0);
        damaged.truncate(12);
        damaged.extend(lz4_flex::compress_prepend_size(&payload));
        assert!(subject.load_state(&damaged).is_err());
        assert_eq!(observe(&subject, Vec::new()), before, "mapper {mapper}");
        session.advance_frame(&mut subject, [0xff, 0xff]).unwrap();
        subject.bus.cartridge.cpu_write(0x6000, 0x17);
        session.restore(&mut subject, &snapshot).unwrap();
        assert_eq!(observe(&subject, Vec::new()), before, "mapper {mapper}");
        for frame in 0..3 {
            let expected = ordinary_frame(&mut control, input(frame));
            let actual = session.advance_frame(&mut subject, input(frame)).unwrap();
            assert_eq!(observe(&subject, actual), observe(&control, expected));
        }
    }
}

#[test]
fn chr_state_rejects_dimensions_outside_loaded_cartridge() {
    for size in [7, 9, u32::MAX] {
        let mut writer = StateWriter::new();
        writer.write_u32(size);
        writer.write_bytes(&[0; 16]);
        let bytes = writer.into_bytes();
        let mut reader = StateReader::new(&bytes);
        let mut chr = vec![0x73; 8];
        assert!(crate::save_state::read_chr_state(&mut reader, &mut chr, "test").is_err());
        assert_eq!(chr, [0x73; 8]);
    }
}
