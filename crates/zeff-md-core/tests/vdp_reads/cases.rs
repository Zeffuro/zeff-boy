use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpMemory, VdpReadCache, VdpReadError, VdpRegisterPipeline, VdpWrite,
    VdpWriteFifo,
};

pub const ADDRESSES: [u32; 10] = [
    0, 1, 0xfe, 0xff, 0x3ffe, 0x3fff, 0x8000, 0x8001, 0xfffe, 0xffff,
];
pub fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}
pub fn store(memory: &mut VdpMemory, address: u32, data: u16) {
    memory.write_word(command(address, 1), data).unwrap();
}

pub fn completed_words_ignore_odd_address_without_swapping() {
    let mut memory = VdpMemory::default();
    for (address, data, expected) in [
        (0, 0x1234, 0x1234),
        (1, 0x5678, 0x7856),
        (0xfffe, 0xabcd, 0xabcd),
        (0xffff, 0x12ef, 0xef12),
    ] {
        store(&mut memory, address, data);
        assert_eq!(memory.read_word(command(address, 0)), Ok(expected));
        assert_eq!(memory.read_word(command(address ^ 1, 0)), Ok(expected));
    }
    for address in ADDRESSES {
        for bit in 0..16 {
            let data = 1u16 << bit;
            store(&mut memory, address & !1, data);
            assert_eq!(memory.read_word(command(address, 0)), Ok(data));
        }
    }
}

pub fn cached_words_are_owned_and_retained() {
    let mut memory = VdpMemory::default();
    store(&mut memory, 0, 0x1234);
    let fifo = VdpWriteFifo::default();
    let mut cache = VdpReadCache::default();
    let mut current = command(1, 0);
    assert_eq!(cache.buffered_word(), None);
    assert!(!cache.is_ready());
    assert_eq!(cache.read(), Err(VdpReadError::Empty));
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, Some(0)),
        Ok(0x1234)
    );
    assert_eq!(current.address, 1);
    store(&mut memory, 0, 0x5678);
    assert_eq!(cache.buffered_word(), Some(0x1234));
    let saved = cache;
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, Some(2)),
        Err(VdpReadError::BufferReady)
    );
    assert_eq!(cache, saved);
    assert_eq!(current.address, 1);
    assert_eq!(cache.read(), Ok(0x1234));
    assert!(!cache.is_ready());
    assert_eq!(cache.buffered_word(), Some(0x1234));
    assert_eq!(cache.read(), Err(VdpReadError::Empty));
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, Some(2)),
        Ok(0x5678)
    );
    assert_eq!(current.address, 3);
    assert_eq!(cache.read(), Ok(0x5678));
    assert_eq!(current.address, 3);
    for address in ADDRESSES {
        store(&mut memory, address & !1, 0xabcd);
        for increment in 0u8..=255 {
            let mut cache = VdpReadCache::default();
            let mut current = command(address, 0);
            assert_eq!(
                cache.complete_read(&memory, &fifo, &mut current, Some(increment)),
                Ok(0xabcd)
            );
            let expected = command(address + u32::from(increment), 0);
            assert_eq!(current, expected);
            assert_eq!(cache.read(), Ok(0xabcd));
            assert_eq!(current, expected);
            assert_eq!(cache.buffered_word(), Some(0xabcd));
        }
    }
}

pub fn fetch_waits_for_the_entire_fifo() {
    let mut memory = VdpMemory::default();
    for count in 1..=4 {
        let mut fifo = VdpWriteFifo::default();
        for index in 0..count {
            fifo.push(VdpWrite {
                command: command(0, 1),
                data: 0x1000 + index as u16,
            })
            .unwrap();
        }
        let mut cache = VdpReadCache::default();
        let mut current = command(0, 0);
        while !fifo.is_empty() {
            let saved_fifo = fifo.clone();
            let saved_memory = memory.clone();
            let saved_cache = cache;
            assert_eq!(
                cache.complete_read(&memory, &fifo, &mut current, Some(2)),
                Err(VdpReadError::FifoPending)
            );
            assert_eq!(fifo, saved_fifo);
            assert_eq!(memory, saved_memory);
            assert_eq!(cache, saved_cache);
            assert_eq!(current, command(0, 0));
            fifo.complete_write(&mut memory).unwrap();
        }
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(2)),
            Ok(0x1000 + count as u16 - 1)
        );
        assert_eq!(current, command(2, 0));
        assert!(fifo.is_empty());
        assert_eq!(cache.read(), Ok(0x1000 + count as u16 - 1));
        assert!(fifo.is_empty());
    }
}

pub fn read_failures_preserve_cache_and_command() {
    let mut memory = VdpMemory::default();
    store(&mut memory, 0, 0xabcd);
    let fifo = VdpWriteFifo::default();
    let mut cache = VdpReadCache::default();
    let mut current = command(0, 0);
    cache
        .complete_read(&memory, &fifo, &mut current, Some(0))
        .unwrap();
    cache.read().unwrap();
    let saved_cache = cache;
    let saved_memory = memory.clone();
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, None),
        Err(VdpReadError::UnknownIncrement)
    );
    assert_eq!(current, command(0, 0));
    assert_eq!(cache, saved_cache);
    for code in 1u8..=255 {
        let mut current = command(0, code);
        assert_eq!(
            memory.read_word(current),
            Err(VdpReadError::UnsupportedCommand(current))
        );
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(255)),
            Err(if code == 4 || code == 8 {
                VdpReadError::UnknownFifoHistory
            } else {
                VdpReadError::UnsupportedCommand(current)
            })
        );
        assert_eq!(current, command(0, code));
        assert_eq!(cache, saved_cache);
        assert_eq!(memory, saved_memory);
    }
    for address in [0x20000, 0x20001, u32::MAX] {
        let mut current = command(address, 0);
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(255)),
            Err(VdpReadError::UnsupportedCommand(current))
        );
        assert_eq!(current, command(address, 0));
        assert_eq!(cache, saved_cache);
    }
    current = command(0xffff, 0);
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, Some(255)),
        Ok(0)
    );
    assert_eq!(current.address, 0x100fe);
    cache.read().unwrap();
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, Some(255)),
        Ok(0)
    );
    assert_eq!(current.address, 0x101fd);
    assert!(cache.is_ready());
}

pub fn published_increment_and_cloned_read_streams() {
    let mut memory = VdpMemory::default();
    store(&mut memory, 0, 0x1234);
    store(&mut memory, 2, 0x5678);
    store(&mut memory, 4, 0xabcd);
    let fifo = VdpWriteFifo::default();
    let mut registers = VdpRegisterPipeline::at_h32_edge(MasterTicks::new(0));
    let mut cache = VdpReadCache::default();
    let mut current = command(1, 0);
    registers.advance_to(MasterTicks::new(1)).unwrap();
    registers.queue_register(15, 2).unwrap();
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, registers.auto_increment()),
        Err(VdpReadError::UnknownIncrement)
    );
    registers.advance_to(MasterTicks::new(20)).unwrap();
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, registers.auto_increment()),
        Ok(0x1234)
    );
    assert_eq!(current.address, 3);
    let mut copied_cache = cache;
    let mut copied_current = current;
    assert_eq!(copied_cache.read(), Ok(0x1234));
    assert!(cache.is_ready());
    assert_eq!(cache.read(), Ok(0x1234));
    registers.advance_to(MasterTicks::new(21)).unwrap();
    registers.queue_register(15, 255).unwrap();
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, registers.auto_increment()),
        Ok(0x5678)
    );
    assert_eq!(current.address, 5);
    assert_eq!(cache.read(), Ok(0x5678));
    registers.advance_to(MasterTicks::new(40)).unwrap();
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, registers.auto_increment()),
        Ok(0xabcd)
    );
    assert_eq!(current.address, 260);
    assert_eq!(
        copied_cache.complete_read(&memory, &fifo, &mut copied_current, Some(0)),
        Ok(0x5678)
    );
    assert_eq!(copied_current.address, 3);
    assert_eq!(cache.read(), Ok(0xabcd));
    assert_eq!(copied_cache.read(), Ok(0x5678));
    assert_eq!(cache.buffered_word(), Some(0xabcd));
    assert_eq!(copied_cache.buffered_word(), Some(0x5678));
}
