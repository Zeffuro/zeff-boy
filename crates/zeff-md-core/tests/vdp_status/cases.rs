use zeff_md_core::{
    ControlCommand, VdpMemory, VdpReadCache, VdpWrite, VdpWriteError, VdpWriteFifo,
};

pub fn entry(address: u32, code: u8, data: u16) -> VdpWrite {
    VdpWrite {
        command: ControlCommand { address, code },
        data,
    }
}

pub fn expected(count: usize) -> u16 {
    match count {
        0 => 0x0200,
        4 => 0x0100,
        _ => 0,
    }
}

pub fn check(fifo: &VdpWriteFifo, count: usize) {
    let saved = fifo.clone();
    assert_eq!(fifo.len(), count);
    for _ in 0..2 {
        assert_eq!(fifo.status_bits(), expected(count));
    }
    assert_eq!(*fifo, saved);
}

pub fn fifo_flags_follow_occupancy_and_reuse() {
    let mut memory = VdpMemory::default();
    let mut fifo = VdpWriteFifo::default();
    for round in 0..8 {
        check(&fifo, 0);
        for index in 0..4 {
            fifo.push(entry(index * 2, 3, round * 5 + index as u16))
                .unwrap();
            check(&fifo, index as usize + 1);
        }
        fifo.complete_write(&mut memory).unwrap();
        check(&fifo, 3);
        fifo.push(entry(8, 3, round * 5 + 4)).unwrap();
        check(&fifo, 4);
        for count in (0..4).rev() {
            fifo.complete_write(&mut memory).unwrap();
            check(&fifo, count);
        }
    }
    assert!(fifo.next_slot_data().is_some());
    check(&fifo, 0);
}

pub fn failed_transfers_preserve_fifo_flags() {
    let mut fifo = VdpWriteFifo::default();
    for index in 0..4 {
        fifo.push(entry(index * 2, 1, index as u16)).unwrap();
    }
    let saved = fifo.clone();
    for (write, error) in [
        (entry(8, 1, 0xffff), VdpWriteError::Full),
        (entry(8, 0x80, 0xffff), VdpWriteError::InvalidCode(0x80)),
        (
            entry(0x20000, 1, 0xffff),
            VdpWriteError::InvalidAddress(0x20000),
        ),
    ] {
        assert_eq!(fifo.push(write), Err(error));
        check(&fifo, 4);
        assert_eq!(fifo, saved);
    }
    let mut memory = VdpMemory::default();
    let saved_memory = memory.clone();
    let mut blocked = VdpWriteFifo::default();
    for _ in 0..4 {
        blocked.push(entry(0, 0, 0xffff)).unwrap();
    }
    let saved = blocked.clone();
    for _ in 0..2 {
        assert_eq!(
            blocked.complete_write(&mut memory),
            Err(VdpWriteError::UnsupportedCommand(entry(0, 0, 0).command))
        );
        check(&blocked, 4);
        assert_eq!(blocked, saved);
        assert_eq!(memory, saved_memory);
    }
}

pub fn read_cache_readiness_is_independent() {
    let mut memory = VdpMemory::default();
    memory
        .write_word(entry(0, 1, 0x1234).command, 0x1234)
        .unwrap();
    let mut fifo = VdpWriteFifo::default();
    let mut cache = VdpReadCache::default();
    let mut current = ControlCommand {
        address: 0,
        code: 0,
    };
    check(&fifo, 0);
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, Some(2)),
        Ok(0x1234)
    );
    assert!(cache.is_ready());
    check(&fifo, 0);
    assert_eq!(cache.read(), Ok(0x1234));
    check(&fifo, 0);
    assert_eq!(
        cache.complete_read(&memory, &fifo, &mut current, Some(0)),
        Ok(0)
    );
    for index in 0..4 {
        fifo.push(entry(index * 2, 1, 0xffff)).unwrap();
    }
    check(&fifo, 4);
    assert!(cache.is_ready());
    assert_eq!(cache.read(), Ok(0));
    assert!(!cache.is_ready());
    check(&fifo, 4);
}
