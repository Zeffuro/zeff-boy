use super::*;

pub(crate) fn fixtures() -> Vec<(&'static str, SaveMemory, u32)> {
    let layouts = [
        ("Bytes512", OddSaveLayout::Bytes512),
        ("KiB8", OddSaveLayout::KiB8),
        ("FixedOddKiB8", OddSaveLayout::FixedOddKiB8),
        ("HeaderlessOddKiB8", OddSaveLayout::HeaderlessOddKiB8),
        ("KiB32", OddSaveLayout::KiB32),
        ("Parallel2048", OddSaveLayout::Parallel2048),
        ("BoundedParallel2048", OddSaveLayout::BoundedParallel2048),
        ("Serial128", OddSaveLayout::Serial128),
        ("Serial256", OddSaveLayout::Serial256),
        ("SplitSerial256", OddSaveLayout::SplitSerial256),
        ("SplitSerial512", OddSaveLayout::SplitSerial512),
        ("SplitSerial2048", OddSaveLayout::SplitSerial2048),
        ("SplitSerial8192", OddSaveLayout::SplitSerial8192),
    ];
    let mut result = Vec::new();
    for (name, layout) in layouts {
        let serial = match layout {
            OddSaveLayout::Serial128 => Some(Eeprom::new(false)),
            OddSaveLayout::Serial256 => Some(Eeprom::new(true)),
            OddSaveLayout::SplitSerial256 => Some(Eeprom::new_split_lines()),
            OddSaveLayout::SplitSerial512 => Some(Eeprom::new_split_512()),
            OddSaveLayout::SplitSerial2048 => Some(Eeprom::new_split_2048()),
            OddSaveLayout::SplitSerial8192 => Some(Eeprom::new_split_8192()),
            _ => None,
        };
        let switched = layout.split_serial();
        let save = SaveMemory {
            layout,
            start: SAVE_START,
            switched,
            initially_enabled: !switched,
            time_latch: false,
            enabled: !switched,
            write_protected: false,
            bytes: vec![0xff; layout.bytes()].into_boxed_slice(),
            serial,
        };
        save.validate_state().unwrap();
        result.push((name, save, if switched { 0x200200 } else { 0x200 }));
    }
    for (name, layout, time_latch) in [
        ("SwitchedKiB8", OddSaveLayout::KiB8, false),
        ("SwitchedKiB32", OddSaveLayout::KiB32, false),
        ("TimeLatchKiB8", OddSaveLayout::KiB8, true),
    ] {
        let save = SaveMemory {
            layout,
            start: SAVE_START,
            switched: true,
            initially_enabled: false,
            time_latch,
            enabled: false,
            write_protected: false,
            bytes: vec![0xff; layout.bytes()].into_boxed_slice(),
            serial: None,
        };
        save.validate_state().unwrap();
        result.push((name, save, 0x200200));
    }
    result
}

pub(crate) fn sf002_save() -> SaveMemory {
    let mut save = headerless_odd8::fixture();
    save.layout = OddSaveLayout::KiB8;
    save.start = 0x3c0000;
    save.validate_state().unwrap();
    assert!(save.sf002_compatible());
    save
}
