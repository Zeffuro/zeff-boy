use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpMemory, VdpRegisterError, VdpRegisterPipeline, VdpWriteFifo,
};

pub fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}
pub fn accepted(index: u8, value: u8) -> bool {
    (index != 1 || value & 4 != 0) && (index != 12 || matches!(value & 0x81, 0 | 0x81))
}
pub fn bases(bank: &VdpRegisterPipeline) -> [Option<u16>; 5] {
    [
        bank.plane_a_base(),
        bank.window_base(),
        bank.plane_b_base(),
        bank.sprite_table_base(),
        bank.hscroll_table_base(),
    ]
}
pub fn publish(bank: &mut VdpRegisterPipeline, index: u8, value: u8) {
    bank.advance_to(tick(bank.now().get() + 1)).unwrap();
    let write = bank.queue_register(index, value).unwrap();
    assert_eq!(bank.advance_to(write.publish_at), Ok(Some(write)));
}

pub fn published_bank_tracks_payloads() {
    let unknown = VdpRegisterPipeline::at_h32_edge(tick(0));
    assert_eq!(unknown.registers(), &[None; 24]);
    assert_eq!(unknown.auto_increment(), None);
    assert_eq!(bases(&unknown), [None; 5]);
    let mut allowed = 0;
    let mut rejected = 0;
    for index in 0..24 {
        for value in 0..=255u8 {
            let mut bank = unknown;
            bank.advance_to(tick(1)).unwrap();
            let saved = bank;
            if accepted(index, value) {
                let event = bank.queue_register(index, value).unwrap();
                assert_eq!(event.publish_at, tick(20));
                assert_eq!(bank.registers(), &[None; 24]);
                bank.advance_to(tick(19)).unwrap();
                assert_eq!(bank.registers(), &[None; 24]);
                let mut copied = bank;
                assert_eq!(bank.advance_to(tick(20)), Ok(Some(event)));
                assert_eq!(copied.advance_to(tick(20)), Ok(Some(event)));
                let mut expected = [None; 24];
                expected[index as usize] = Some(value);
                assert_eq!(bank.registers(), &expected);
                assert_eq!(bank, copied);
                assert_eq!(
                    bank.auto_increment(),
                    if index == 15 { Some(value) } else { None }
                );
                assert_eq!(bank.advance_to(tick(20)), Ok(None));
                assert_eq!(bank.registers(), &expected);
                allowed += 1;
            } else {
                assert_eq!(
                    bank.queue_register(index, value),
                    Err(VdpRegisterError::UnsupportedModeChange { index, value })
                );
                assert_eq!(bank, saved);
                rejected += 1;
            }
        }
    }
    assert_eq!((allowed, rejected), (5888, 256));
}

pub fn table_addresses_follow_published_bits() {
    for value in 0..=255u8 {
        let v = u16::from(value);
        let expected = [
            (v / 8 % 8) * 0x2000,
            (v / 2 % 32) * 0x800,
            (v % 8) * 0x2000,
            (v % 128) * 0x200,
            (v % 64) * 0x400,
        ];
        for (table, index) in [2, 3, 4, 5, 13].into_iter().enumerate() {
            let mut bank = VdpRegisterPipeline::at_h32_edge(tick(0));
            bank.advance_to(tick(1)).unwrap();
            bank.queue_register(index, value).unwrap();
            assert_eq!(bases(&bank), [None; 5]);
            bank.advance_to(tick(19)).unwrap();
            assert_eq!(bases(&bank), [None; 5]);
            bank.advance_to(tick(20)).unwrap();
            let mut addresses = [None; 5];
            addresses[table] = Some(expected[table]);
            assert_eq!(bases(&bank), addresses);
            assert_eq!(bank.registers()[index as usize], Some(value));
        }
    }
    for (payloads, expected) in [
        (
            [0x30, 0x3c, 0x07, 0x7c, 0x3f],
            [0xc000, 0xf000, 0xe000, 0xf800, 0xfc00],
        ),
        ([0xff; 5], [0xe000, 0xf800, 0xe000, 0xfe00, 0xfc00]),
        ([0x07, 0x01, 0xf8, 0x80, 0xc0], [0; 5]),
    ] {
        let mut bank = VdpRegisterPipeline::at_h32_edge(tick(7));
        for (index, value) in [2, 3, 4, 5, 13].into_iter().zip(payloads) {
            publish(&mut bank, index, value);
        }
        assert_eq!(bases(&bank), expected.map(Some));
        for (index, value) in [2, 3, 4, 5, 13].into_iter().zip(payloads) {
            assert_eq!(bank.registers()[index as usize], Some(value));
        }
        assert_eq!(bank.registers()[12], None);
    }
}

pub fn replacement_and_rejections_preserve_bank() {
    let mut bank = VdpRegisterPipeline::at_h32_edge(tick(0));
    for index in 0..24 {
        publish(
            &mut bank,
            index,
            if index == 1 {
                4
            } else if index == 12 {
                0
            } else {
                index ^ 0xff
            },
        );
    }
    let previous = *bank.registers();
    bank.advance_to(tick(481)).unwrap();
    let event = bank.queue_register(2, 0x20).unwrap();
    assert_eq!(event.publish_at, tick(500));
    for index in 24..=255 {
        let saved = bank;
        assert_eq!(
            bank.queue_register(index, 0),
            Err(VdpRegisterError::UnsupportedRegister(index))
        );
        assert_eq!(bank, saved);
    }
    for (index, value, error) in [
        (
            1,
            0,
            VdpRegisterError::UnsupportedModeChange { index: 1, value: 0 },
        ),
        (
            12,
            0x80,
            VdpRegisterError::UnsupportedModeChange {
                index: 12,
                value: 0x80,
            },
        ),
        (
            7,
            0,
            VdpRegisterError::Busy {
                publish_at: tick(500),
            },
        ),
    ] {
        let saved = bank;
        assert_eq!(bank.queue_register(index, value), Err(error));
        assert_eq!(bank, saved);
    }
    let saved = bank;
    assert_eq!(
        bank.advance_to(tick(480)),
        Err(VdpRegisterError::TimeReversal {
            current: tick(481),
            requested: tick(480)
        })
    );
    assert_eq!(bank, saved);
    let mut copied = bank;
    bank.advance_to(tick(499)).unwrap();
    assert_eq!(bank.registers(), &previous);
    assert_eq!(bank.advance_to(tick(500)), Ok(Some(event)));
    copied.advance_to(tick(499)).unwrap();
    assert_eq!(copied.registers(), &previous);
    assert_eq!(copied.advance_to(tick(700)), Ok(Some(event)));
    bank.advance_to(tick(700)).unwrap();
    assert_eq!(copied, bank);
    let mut expected = previous;
    expected[2] = Some(0x20);
    assert_eq!(bank.registers(), &expected);
    assert_eq!(bank.plane_a_base(), Some(0x8000));
    let saved = bank;
    assert_eq!(
        bank.queue_register(0, 1),
        Err(VdpRegisterError::CoincidentClockEdge)
    );
    assert_eq!(bank, saved);
    bank.advance_to(tick(u64::MAX)).unwrap();
    let saved = bank;
    assert_eq!(
        bank.queue_register(0, 1),
        Err(VdpRegisterError::ClockOverflow)
    );
    assert_eq!(bank, saved);
    assert_eq!(bank.registers(), &expected);
}

pub fn published_addresses_and_increment_drive_fifo() {
    let mut bank = VdpRegisterPipeline::at_h32_edge(tick(0));
    for (index, value) in [(2, 0x30), (3, 0x3c), (4, 7), (5, 0x7c), (13, 0x3f), (15, 2)] {
        publish(&mut bank, index, value);
    }
    let mut memory = VdpMemory::default();
    let mut fifo = VdpWriteFifo::default();
    assert_eq!(
        bases(&bank),
        [0xc000, 0xf000, 0xe000, 0xf800, 0xfc00].map(Some)
    );
    for (address, data) in [
        bank.plane_a_base(),
        bank.window_base(),
        bank.plane_b_base(),
        bank.sprite_table_base(),
    ]
    .into_iter()
    .zip([0x1234, 0x5678, 0x9abc, 0xdef0])
    {
        let address = u32::from(address.unwrap());
        let mut current = ControlCommand { address, code: 1 };
        fifo.push_data(&mut current, bank.auto_increment(), data)
            .unwrap();
        assert_eq!(current.address, address + 2);
    }
    publish(&mut bank, 2, 0x38);
    publish(&mut bank, 15, 255);
    assert_eq!(bank.plane_a_base(), Some(0xe000));
    assert_eq!(bank.auto_increment(), bank.registers()[15]);
    assert_eq!(bank.auto_increment(), Some(255));
    for (address, data) in [0xc000, 0xf000, 0xe000, 0xf800]
        .into_iter()
        .zip([0x1234, 0x5678, 0x9abc, 0xdef0])
    {
        assert_eq!(fifo.front().unwrap().command.address, address);
        assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
        assert_eq!(
            fifo.service_write_slot(&mut memory).unwrap().unwrap().data,
            data
        );
        assert_eq!(
            memory.read_word(ControlCommand { address, code: 0 }),
            Ok(data)
        );
    }
    let mut current = ControlCommand {
        address: u32::from(bank.hscroll_table_base().unwrap()),
        code: 0x11,
    };
    fifo.push_data(&mut current, bank.auto_increment(), 0x1357)
        .unwrap();
    assert_eq!(current.address, 0xfcff);
    fifo.complete_write(&mut memory).unwrap();
    assert_eq!(
        memory.read_word(ControlCommand {
            address: 0xfc00,
            code: 0
        }),
        Ok(0x1357)
    );
}
