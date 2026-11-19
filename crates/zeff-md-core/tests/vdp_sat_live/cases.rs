use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, ControlWrite, Region, Vdp, VdpInterrupts, VdpPortError, VdpRegisterPipeline,
    VdpRenderError, VdpScheduleError,
};

type Control = Result<ControlWrite, VdpPortError>;
type Data = Result<(), VdpPortError>;
type Advance = Result<(), VdpScheduleError>;
type Words = Result<[Option<u16>; 2], VdpRenderError>;
const REGION: Region = Region::NtscUs;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn at(now: u64) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    device.advance_to(tick(now), REGION).unwrap();
    device
}

fn command(
    control: &mut impl FnMut(&mut Vdp, u16, Region) -> Control,
    device: &mut Vdp,
    address: u32,
    code: u8,
) {
    control(
        device,
        (address as u16 & 0x3fff) | (u16::from(code & 3) << 14),
        REGION,
    )
    .unwrap();
    control(
        device,
        ((address >> 14) as u16 & 7) | (u16::from(code & 0x3c) << 2),
        REGION,
    )
    .unwrap();
    assert_eq!(device.command(), ControlCommand { address, code });
}

fn register(device: &mut Vdp, index: u8, value: u8) {
    device
        .advance_to(tick(device.now().get() + 1), REGION)
        .unwrap();
    let pending = device.queue_register(index, value, REGION).unwrap();
    device.advance_to(pending.publish_at, REGION).unwrap();
    device
        .advance_to(tick(device.now().get() + 1), REGION)
        .unwrap();
}

fn slot_after(now: u64) -> u64 {
    let mut slot = (now / 20) * 20 + 10;
    if slot <= now {
        slot += 20;
    }
    if [530, 1170, 1810, 2450, 3310].contains(&(slot % 3420)) {
        slot += 20;
    }
    slot
}

fn write(
    control: &mut impl FnMut(&mut Vdp, u16, Region) -> Control,
    data: &mut impl FnMut(&mut Vdp, u16, Region) -> Data,
    advance: &mut impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
    device: &mut Vdp,
    (address, code): (u32, u8),
    word: u16,
) {
    command(control, device, address, code);
    assert_eq!(data(device, word, REGION), Ok(()));
    let end = slot_after(device.now().get()) + if code & 0xf == 1 { 41 } else { 1 };
    assert_eq!(advance(device, tick(end), REGION), Ok(()));
    assert!(device.fifo().is_empty());
}

fn inspect(device: &Vdp, index: u8) -> Words {
    device.sprite_cache()?.entry_words(index)
}

macro_rules! live_case {
    ($name:ident, $with:ident, |$control:ident, $data:ident, $advance:ident, $words:ident| $body:block) => {
        pub fn $with(
            mut $control: impl FnMut(&mut Vdp, u16, Region) -> Control,
            mut $data: impl FnMut(&mut Vdp, u16, Region) -> Data,
            mut $advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
            mut $words: impl FnMut(&Vdp, u8) -> Words,
        ) $body
        pub fn $name() {
            $with(Vdp::write_control, Vdp::write_data, Vdp::advance_to, inspect);
        }
    };
}

live_case!(
    cold_and_known_seeds,
    cold_and_known_seeds_with,
    |control, data, advance, words| {
        let mut bank = VdpRegisterPipeline::at_h32_edge(tick(0));
        for (index, value) in [(0, 0), (1, 4), (10, 0), (12, 0), (15, 0)] {
            bank.advance_to(tick(bank.now().get() + 1)).unwrap();
            let pending = bank.queue_register(index, value).unwrap();
            bank.advance_to(pending.publish_at).unwrap();
        }
        let interrupts = VdpInterrupts::at_counters_zero(bank.now(), 0, false, false);
        let mut unknown = Vdp::from_known_state(interrupts, bank, REGION).unwrap();
        assert_eq!(unknown.registers().registers()[5], None);
        for device in [&at(1), &unknown] {
            for index in 0..64 {
                assert_eq!(words(device, index), Ok([None, None]));
            }
        }
        unknown
            .advance_to(tick(unknown.now().get() + 1), REGION)
            .unwrap();
        write(
            &mut control,
            &mut data,
            &mut advance,
            &mut unknown,
            (0, 1),
            0xffff,
        );
        assert_eq!(&unknown.memory().vram()[..2], &[255, 255]);
        for index in 0..64 {
            assert_eq!(words(&unknown, index), Ok([None, None]));
        }
        register(&mut unknown, 5, 0);
        write(
            &mut control,
            &mut data,
            &mut advance,
            &mut unknown,
            (0, 1),
            144,
        );
        assert_eq!(words(&unknown, 0), Ok([Some(144), None]));
    }
);

live_case!(
    queued_partial_and_settled,
    queued_partial_and_settled_with,
    |control, data, advance, words| {
        for region in [REGION, Region::NtscJapan] {
            let mut device = at(1);
            command(&mut control, &mut device, 0, 1);
            assert_eq!(data(&mut device, 0xff91, region), Ok(()));
            assert_eq!(words(&device, 0), Err(VdpRenderError::PendingSpriteWrites));
            assert_eq!(advance(&mut device, tick(10), region), Ok(()));
            assert_eq!(&device.memory().vram()[..2], &[0, 0x91]);
            assert_eq!(words(&device, 0), Err(VdpRenderError::PendingSpriteWrites));
            assert_eq!(advance(&mut device, tick(29), region), Ok(()));
            assert_eq!(words(&device, 0), Err(VdpRenderError::PendingSpriteWrites));
            assert_eq!(advance(&mut device, tick(30), region), Ok(()));
            assert_eq!(words(&device, 0), Ok([Some(0x391), None]));
            assert_eq!(&device.memory().vram()[..2], &[0xff, 0x91]);
            device.advance_to(tick(31), region).unwrap();
            command(&mut control, &mut device, 2, 0x11);
            assert_eq!(data(&mut device, 0xffff, region), Ok(()));
            command(&mut control, &mut device, 4, 3);
            assert_eq!(data(&mut device, 0xffff, region), Ok(()));
            assert_eq!(advance(&mut device, tick(70), region), Ok(()));
            assert_eq!(words(&device, 0), Err(VdpRenderError::PendingSpriteWrites));
            assert_eq!(advance(&mut device, tick(90), region), Ok(()));
            assert_eq!(words(&device, 0), Ok([Some(0x391), Some(0x0f7f)]));
        }
    }
);

live_case!(
    all_entries_fields_and_aliases,
    all_entries_fields_and_aliases_with,
    |control, data, advance, words| {
        for code in [1, 0x11] {
            let mut device = at(1);
            register(&mut device, 5, 0x20);
            for index in 0..64u8 {
                let base = 0x4000 + u32::from(index) * 8;
                let y = 0xfe00 | (u16::from(index) * 7);
                let layout = 0xf080 | (u16::from(index) * 257);
                write(
                    &mut control,
                    &mut data,
                    &mut advance,
                    &mut device,
                    (base + 2, code),
                    layout,
                );
                assert_eq!(words(&device, index), Ok([None, Some(layout & 0x0f7f)]));
                write(
                    &mut control,
                    &mut data,
                    &mut advance,
                    &mut device,
                    (base, code),
                    y,
                );
                for offset in [4, 6] {
                    write(
                        &mut control,
                        &mut data,
                        &mut advance,
                        &mut device,
                        (base + offset, code),
                        0xffff,
                    );
                }
                assert_eq!(
                    words(&device, index),
                    Ok([Some(y & 0x3ff), Some(layout & 0x0f7f)])
                );
                assert_eq!(
                    &device.memory().vram()[base as usize..base as usize + 2],
                    &y.to_be_bytes()
                );
            }
            for index in 64..=255 {
                assert_eq!(
                    words(&device, index),
                    Err(VdpRenderError::InvalidSpriteIndex(index))
                );
            }
        }
    }
);

live_case!(
    aperture_boundaries_and_bit16,
    aperture_boundaries_and_bit16_with,
    |control, data, advance, words| {
        for r5 in [0, 1, 0x7f, 0x80, 0x81, 0xff] {
            let mut device = at(1);
            register(&mut device, 5, r5);
            let base = u32::from(r5 & 0x7f) << 9;
            for offset in [0, 2, 504, 506, 508, 510] {
                write(
                    &mut control,
                    &mut data,
                    &mut advance,
                    &mut device,
                    (base + offset, 1),
                    0xffff,
                );
            }
            for address in [base.checked_sub(2), base.checked_add(512)]
                .into_iter()
                .flatten()
                .filter(|address| *address < 65536)
            {
                write(
                    &mut control,
                    &mut data,
                    &mut advance,
                    &mut device,
                    (address, 1),
                    128,
                );
            }
            for index in 0..64 {
                let expected = if r5 < 128 && (index == 0 || index == 63) {
                    [Some(0x3ff), Some(0x0f7f)]
                } else {
                    [None, None]
                };
                assert_eq!(words(&device, index), Ok(expected));
            }
            assert_eq!(
                &device.memory().vram()[base as usize..base as usize + 2],
                &[255, 255]
            );
        }
    }
);

live_case!(
    relocated_cache_and_live_fields,
    relocated_cache_and_live_fields_with,
    |control, data, advance, words| {
        let mut device = at(1);
        for (address, value) in [
            (0, 144),
            (2, 0x0603),
            (4, 0x1234),
            (6, 160),
            (512, 192),
            (514, 0x0f07),
            (516, 0xabcd),
            (518, 208),
        ] {
            write(
                &mut control,
                &mut data,
                &mut advance,
                &mut device,
                (address, 1),
                value,
            );
        }
        register(&mut device, 5, 1);
        assert_eq!(words(&device, 0), Ok([Some(144), Some(0x0603)]));
        let sprite = device
            .sprite_cache()
            .unwrap()
            .sprite_entry(device.memory(), device.registers(), 0)
            .unwrap();
        assert_eq!(
            (
                sprite.x,
                sprite.y,
                sprite.width,
                sprite.height,
                sprite.link,
                sprite.name
            ),
            (80, 16, 16, 24, 3, 0xabcd)
        );
        assert_eq!(
            device
                .memory()
                .sprite_entry(device.registers(), 0)
                .unwrap()
                .y,
            64
        );
        write(
            &mut control,
            &mut data,
            &mut advance,
            &mut device,
            (0, 1),
            200,
        );
        assert_eq!(words(&device, 0), Ok([Some(144), Some(0x0603)]));
        write(
            &mut control,
            &mut data,
            &mut advance,
            &mut device,
            (514, 1),
            0x0105,
        );
        assert_eq!(words(&device, 0), Ok([Some(144), Some(0x0105)]));
        register(&mut device, 5, 0x81);
        write(
            &mut control,
            &mut data,
            &mut advance,
            &mut device,
            (512, 1),
            208,
        );
        assert_eq!(words(&device, 0), Ok([Some(144), Some(0x0105)]));
        assert_eq!(
            device
                .sprite_cache()
                .unwrap()
                .sprite_entry(device.memory(), device.registers(), 0)
                .unwrap()
                .name,
            0xabcd
        );
    }
);

live_case!(
    odd_invalidation_and_repair,
    odd_invalidation_and_repair_with,
    |control, data, advance, words| {
        let mut device = at(1);
        register(&mut device, 5, 1);
        for index in 0..64u8 {
            for offset in [0, 2] {
                write(
                    &mut control,
                    &mut data,
                    &mut advance,
                    &mut device,
                    (512 + u32::from(index) * 8 + offset, 1),
                    0xffff,
                );
            }
        }
        for (address, code) in [
            (511, 1),
            (1025, 0x11),
            (513, 3),
            (513, 5),
            (513, 0x13),
            (513, 0x15),
        ] {
            write(
                &mut control,
                &mut data,
                &mut advance,
                &mut device,
                (address, code),
                0xffff,
            );
            for index in 0..64 {
                assert_eq!(words(&device, index), Ok([Some(0x3ff), Some(0x0f7f)]));
            }
        }
        for address in [513, 517, 1023] {
            write(
                &mut control,
                &mut data,
                &mut advance,
                &mut device,
                (address, 1),
                0x1234,
            );
            for index in 0..64 {
                assert_eq!(words(&device, index), Ok([None, None]));
            }
            write(
                &mut control,
                &mut data,
                &mut advance,
                &mut device,
                (512, 1),
                144,
            );
            assert_eq!(words(&device, 0), Ok([Some(144), None]));
            write(
                &mut control,
                &mut data,
                &mut advance,
                &mut device,
                (514, 0x11),
                0x0201,
            );
            assert_eq!(words(&device, 0), Ok([Some(144), Some(0x0201)]));
        }
    }
);

live_case!(
    pending_table_publication,
    pending_table_publication_with,
    |control, data, advance, words| {
        for (address, r5) in [(0, 0), (0, 1), (512, 1), (512, 0x81)] {
            for change_at in [1, 11, 21] {
                for port in [false, true] {
                    let mut device = at(1);
                    command(&mut control, &mut device, address, 1);
                    data(&mut device, 144, REGION).unwrap();
                    advance(&mut device, tick(change_at), REGION).unwrap();
                    let fifo = device.fifo().clone();
                    let memory = device.memory().clone();
                    if port {
                        assert_eq!(
                            control(&mut device, 0x8500 | u16::from(r5), REGION),
                            Ok(ControlWrite::Register {
                                index: 5,
                                value: r5
                            })
                        );
                    } else {
                        device.queue_register(5, r5, REGION).unwrap();
                    }
                    assert_eq!(device.fifo(), &fifo);
                    assert_eq!(device.memory(), &memory);
                    assert_eq!(device.registers().registers()[5], Some(0));
                    assert_eq!(words(&device, 0), Err(VdpRenderError::PendingSpriteWrites));
                    let mut split = device.clone();
                    advance(&mut device, tick(100), REGION).unwrap();
                    for endpoint in [change_at + 1, 29, 30, 100] {
                        advance(&mut split, tick(endpoint), REGION).unwrap();
                    }
                    assert_eq!(device, split);
                    assert_eq!(device.registers().registers()[5], Some(r5));
                    assert_eq!(
                        &device.memory().vram()[address as usize..address as usize + 2],
                        &[0, 144]
                    );
                    assert_eq!(
                        words(&device, 0),
                        Ok([
                            if address == u32::from(if change_at < 20 { r5 } else { 0 }) << 9 {
                                Some(144)
                            } else {
                                None
                            },
                            None
                        ])
                    );
                }
            }
        }
        for code in [1, 3, 5] {
            let mut device = at(1);
            device.queue_register(5, 1, REGION).unwrap();
            command(&mut control, &mut device, 512, code);
            assert_eq!(device.data_write_wait(REGION), Ok(None));
            data(&mut device, 144, REGION).unwrap();
            advance(&mut device, tick(100), REGION).unwrap();
            assert_eq!(
                words(&device, 0),
                Ok([if code == 1 { Some(144) } else { None }, None])
            );
        }
    }
);

live_case!(
    segmented_horizon_and_copies,
    segmented_horizon_and_copies_with,
    |control, data, advance, words| {
        let mut device = at(1);
        register(&mut device, 15, 2);
        command(&mut control, &mut device, 0, 1);
        for value in [144, 0x0603, 0xabcd, 208] {
            assert_eq!(data(&mut device, value, REGION), Ok(()));
        }
        let mut segmented = device.clone();
        assert_eq!(advance(&mut device, tick(u64::MAX), REGION), Ok(()));
        for endpoint in [30, 50, 70, 90, 110, 130, 150, 170, 2660, 766090, u64::MAX] {
            assert_eq!(advance(&mut segmented, tick(endpoint), REGION), Ok(()));
        }
        assert_eq!(device, segmented);
        assert_eq!(words(&device, 0), Ok([Some(144), Some(0x0603)]));
        let mut source = at(1);
        write(
            &mut control,
            &mut data,
            &mut advance,
            &mut source,
            (0, 1),
            144,
        );
        let copied_cache = *source.sprite_cache().unwrap();
        let copied_device = source.clone();
        write(
            &mut control,
            &mut data,
            &mut advance,
            &mut source,
            (2, 1),
            0x0603,
        );
        assert_eq!(copied_cache.entry_words(0), Ok([Some(144), None]));
        assert_eq!(words(&copied_device, 0), Ok([Some(144), None]));
        assert_eq!(words(&source, 0), Ok([Some(144), Some(0x0603)]));
    }
);

live_case!(
    future_failure_atomicity,
    future_failure_atomicity_with,
    |control, data, advance, words| {
        let mut device = at(766011);
        command(&mut control, &mut device, 0, 1);
        assert_eq!(data(&mut device, 144, REGION), Ok(()));
        control(&mut device, 0x4002, REGION).unwrap();
        data(&mut device, 0x0603, REGION).unwrap();
        assert_eq!(words(&device, 0), Err(VdpRenderError::PendingSpriteWrites));
        assert_eq!(device.control().pending_word(), None);
        assert_eq!(advance(&mut device, tick(766091), REGION), Ok(()));
        assert_eq!(words(&device, 0), Ok([Some(144), Some(0x0603)]));
        assert_eq!(&device.memory().vram()[..4], &[0, 144, 6, 3]);
        assert!(device.interrupts().vertical_pending());
        advance(&mut device, tick(766151), REGION).unwrap();
        assert_eq!(words(&device, 0), Ok([Some(144), Some(0x0603)]));
        assert_eq!(&device.memory().vram()[..4], &[0, 144, 6, 3]);
    }
);
