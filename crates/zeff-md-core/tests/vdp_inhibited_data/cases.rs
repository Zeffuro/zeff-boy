use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, Region, Vdp, VdpDmaError, VdpMemory, VdpPortError, VdpReadError, VdpWrite,
    VdpWriteError, VdpWriteFifo,
};

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn register(vdp: &mut Vdp, index: u8, value: u8, region: Region) {
    vdp.advance_to(tick(vdp.now().get() + 1), region).unwrap();
    let write = vdp.queue_register(index, value, region).unwrap();
    vdp.advance_to(write.publish_at, region).unwrap();
}

fn command(vdp: &mut Vdp, address: u32, code: u8, region: Region) {
    vdp.write_control(
        (address as u16 & 0x3fff) | u16::from(code & 3) << 14,
        region,
    )
    .unwrap();
    vdp.write_control(
        ((address >> 14) as u16 & 7) | u16::from(code & 0x3c) << 2,
        region,
    )
    .unwrap();
    assert_eq!(vdp.command(), ControlCommand { address, code });
}

fn fixture(region: Region, horizontal: u8, increment: u8) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    for (index, value) in [(12, horizontal), (15, 2), (1, 4)] {
        register(&mut vdp, index, value, region);
    }
    for (address, code, data) in [(0, 1, 0x369c), (0x20, 3, 0x0ace), (0x20, 5, 0x0567)] {
        command(&mut vdp, address, code, region);
        vdp.write_data(data, region).unwrap();
    }
    vdp.advance_to(tick(vdp.now().get() + 3420), region)
        .unwrap();
    register(&mut vdp, 15, increment, region);
    command(&mut vdp, 0, 0, region);
    let end = vdp.data_read_wait(region).unwrap().unwrap();
    vdp.advance_to(end, region).unwrap();
    assert_eq!(vdp.read_cache().buffered_word(), Some(0x369c));
    vdp
}

fn memory() -> VdpMemory {
    let mut memory = VdpMemory::default();
    for (code, data) in [(1, 0xa135), (3, 0x0ace), (5, 0x0567)] {
        memory
            .write_word(
                ControlCommand {
                    address: 0x20,
                    code,
                },
                data,
            )
            .unwrap();
    }
    memory
}

pub fn inhibited_target_uses_one_fifo_slot_without_memory_writes() {
    for increment in [0, 1, 2, 0xff] {
        for address in [0, 1, 0x20, 0x4e, 0x50, 0x7f, 0xffff, 0x1fffe, 0x1ffff] {
            let mut memory = memory();
            let saved = memory.clone();
            let mut fifo = VdpWriteFifo::default();
            let original = ControlCommand {
                address,
                code: 0x14,
            };
            let mut current = original;
            let entry = VdpWrite {
                command: original,
                data: 0xbeef,
            };
            assert_eq!(
                fifo.push_data(&mut current, Some(increment), entry.data),
                Ok(entry)
            );
            assert_eq!(current.address, (address + u32::from(increment)) & 0x1ffff);
            assert_eq!(current.code, 0x14);
            assert_eq!(fifo.len(), 1);
            assert_eq!(fifo.front(), Some(entry));
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(Some(entry)));
            assert!(fifo.is_empty());
            assert_eq!(memory, saved);
            assert_eq!(fifo.service_write_slot(&mut memory), Ok(None));
            assert_eq!(memory, saved);
            memory.write_word(original, 0x1234).unwrap();
            assert_eq!(memory, saved);
        }
    }
}

pub fn inhibited_port_write_cancels_parser_and_keeps_cached_history() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for horizontal in [0, 0x81] {
            for increment in [0, 1, 2, 0xff] {
                let mut vdp = fixture(region, horizontal, increment);
                let memory = vdp.memory().clone();
                command(&mut vdp, 0x1ffff, 0x14, region);
                vdp.write_control(0x3fff, region).unwrap();
                assert_eq!(vdp.control().pending_word(), Some(0x3fff));
                assert_eq!(vdp.command().code, 0x14);
                let now = vdp.now();
                vdp.write_data(0xbeef, region).unwrap();
                assert_eq!(vdp.now(), now);
                assert_eq!(
                    vdp.command().address,
                    (0x1ffff + u32::from(increment)) & 0x1ffff
                );
                assert_eq!(vdp.control().pending_word(), None);
                assert!(!vdp.read_cache().is_ready());
                assert_eq!(vdp.read_cache().buffered_word(), Some(0x369c));
                assert_eq!(vdp.fifo().len(), 1);
                assert_eq!(
                    vdp.fifo().front().unwrap().command,
                    ControlCommand {
                        address: 0x1ffff,
                        code: 0x14
                    }
                );
                let mut full = vdp.clone();
                while !full.fifo().is_full() {
                    full.write_data(0, region).unwrap();
                }
                let slot = full.data_write_wait(region).unwrap().unwrap();
                vdp.advance_to(tick(slot.get() - 1), region).unwrap();
                assert_eq!(vdp.fifo().len(), 1);
                assert_eq!(vdp.memory(), &memory);
                vdp.advance_to(slot, region).unwrap();
                assert!(vdp.fifo().is_empty());
                assert_eq!(vdp.memory(), &memory);
                assert!(!vdp.read_cache().is_ready());
                assert_eq!(vdp.read_cache().buffered_word(), Some(0x369c));
            }
        }
    }
}

pub fn inhibited_entries_preserve_interleaved_fifo_order() {
    let mut memory = memory();
    let saved = memory.clone();
    let entries =
        [(0x14, 0xbeef), (1, 0xd26b), (0x14, 0x1234), (3, 0x0246)].map(|(code, data)| VdpWrite {
            command: ControlCommand {
                address: 0x20,
                code,
            },
            data,
        });
    let mut fifo = VdpWriteFifo::default();
    for entry in entries {
        fifo.push(entry).unwrap();
    }
    let mut expected = saved.clone();
    for (index, retired) in [
        Some(entries[0]),
        None,
        Some(entries[1]),
        Some(entries[2]),
        Some(entries[3]),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(fifo.service_write_slot(&mut memory), Ok(retired));
        if index == 1 {
            expected
                .write_word(
                    ControlCommand {
                        address: 0x20,
                        code: 1,
                    },
                    0xa16b,
                )
                .unwrap();
        } else if index == 2 {
            expected
                .write_word(entries[1].command, entries[1].data)
                .unwrap();
        } else if index == 4 {
            expected
                .write_word(entries[3].command, entries[3].data)
                .unwrap();
        }
        assert_eq!(memory, expected);
        assert_eq!(fifo.len(), [3, 3, 2, 1, 0][index]);
        if let Some(next) = [
            Some(entries[1]),
            Some(entries[1]),
            Some(entries[2]),
            Some(entries[3]),
            None,
        ][index]
        {
            assert_eq!(fifo.front(), Some(next));
        }
    }
    assert!(fifo.is_empty());
    assert_eq!(memory.vsram(), saved.vsram());
}

pub fn full_inhibited_fifo_rejects_then_retries_at_its_first_slot() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for horizontal in [0, 0x81] {
            let mut vdp = fixture(region, horizontal, 2);
            let memory = vdp.memory().clone();
            command(&mut vdp, 0x1fffc, 0x14, region);
            for word in [0x1111, 0x2222, 0x3333, 0x4444] {
                assert_eq!(vdp.data_write_wait(region), Ok(None));
                vdp.write_data(word, region).unwrap();
            }
            vdp.write_control(0x0004, region).unwrap();
            let before = vdp.clone();
            assert_eq!(
                vdp.write_data(0x5555, region),
                Err(VdpPortError::Data(VdpWriteError::Full))
            );
            assert_eq!(vdp, before);
            let slot = vdp.data_write_wait(region).unwrap().unwrap();
            assert_eq!(vdp, before);
            assert!(slot > vdp.now());
            vdp.advance_to(tick(slot.get() - 1), region).unwrap();
            assert!(vdp.fifo().is_full());
            assert_eq!(vdp.data_write_wait(region), Ok(Some(slot)));
            vdp.advance_to(slot, region).unwrap();
            assert_eq!(vdp.fifo().len(), 3);
            assert_eq!(vdp.fifo().front().unwrap().data, 0x2222);
            assert_eq!(vdp.data_write_wait(region), Ok(None));
            vdp.write_data(0x5555, region).unwrap();
            assert_eq!(vdp.command().address, 6);
            assert_eq!(vdp.control().pending_word(), None);
            assert!(vdp.fifo().is_full());
            vdp.advance_to(tick(slot.get() + 3420), region).unwrap();
            assert!(vdp.fifo().is_empty());
            assert_eq!(vdp.memory(), &memory);
        }
    }
}

pub fn adjacent_commands_and_dma_keep_their_data_boundaries() {
    for region in [Region::NtscUs, Region::Pal] {
        let seed = fixture(region, 0, 2);
        for code in [0x18, 0x1c] {
            let mut vdp = seed.clone();
            command(&mut vdp, 0x8000, code, region);
            let before = vdp.clone();
            let error = Err(VdpPortError::Data(VdpWriteError::UnsupportedCommand(
                vdp.command(),
            )));
            assert_eq!(vdp.write_data(0xbeef, region), error);
            assert_eq!(vdp.data_write_wait(region), error.map(|_| None));
            assert_eq!(vdp, before);
        }
        let mut inhibited = seed.clone();
        command(&mut inhibited, 0x8000, 0x14, region);
        let before = inhibited.clone();
        assert_eq!(
            inhibited.data_read_wait(region),
            Err(VdpPortError::Read(VdpReadError::UnsupportedCommand(
                inhibited.command()
            )))
        );
        assert_eq!(
            inhibited.read_data(region),
            Err(VdpPortError::Read(VdpReadError::Empty))
        );
        assert_eq!(inhibited, before);
        let mut dma = seed;
        for (index, value) in [(1, 0x14), (19, 2), (20, 0), (21, 0), (22, 0), (23, 0xc0)] {
            register(&mut dma, index, value, region);
        }
        command(&mut dma, 0x8000, 0x34, region);
        let before = dma.clone();
        assert_eq!(
            dma.write_data(0xbeef, region),
            Err(VdpPortError::Dma(VdpDmaError::Busy))
        );
        assert_eq!(
            dma.data_write_wait(region),
            Err(VdpPortError::Dma(VdpDmaError::Busy))
        );
        assert_eq!(dma, before);
    }
    for code in [0x18, 0x1c, 0x34] {
        let mut fifo = VdpWriteFifo::default();
        let mut current = ControlCommand {
            address: 0x1ffff,
            code,
        };
        let before = current;
        assert_eq!(
            fifo.push_data(&mut current, Some(2), 0xbeef),
            Err(VdpWriteError::UnsupportedCommand(before))
        );
        assert!(fifo.is_empty());
        assert_eq!(current, before);
    }
}
