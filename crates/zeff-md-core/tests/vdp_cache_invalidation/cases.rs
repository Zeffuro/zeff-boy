use zeff_md_core::{
    ControlCommand, ControlWrite, VdpControl, VdpMemory, VdpReadCache, VdpReadError, VdpWriteError,
    VdpWriteFifo,
};

pub fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}

fn control_write(
    cache: &mut VdpReadCache,
    control: &mut VdpControl,
    current: &mut ControlCommand,
    word: u16,
) -> Result<ControlWrite, VdpWriteError> {
    let request = control.complete_control_write(current, word, false)?;
    cache.invalidate();
    Ok(request)
}

fn data_write(
    cache: &mut VdpReadCache,
    control: &mut VdpControl,
    fifo: &mut VdpWriteFifo,
    current: &mut ControlCommand,
    increment: Option<u8>,
    data: u16,
) -> Result<(), VdpWriteError> {
    fifo.push_data(current, increment, data)?;
    control.cancel_pending();
    cache.invalidate();
    Ok(())
}

pub fn invalidation_retains_completed_history() {
    let mut unknown = VdpReadCache::default();
    for _ in 0..2 {
        unknown.invalidate();
        assert_eq!(unknown, VdpReadCache::default());
        assert_eq!(unknown.read(), Err(VdpReadError::Empty));
    }
    let mut memory = VdpMemory::default();
    let fifo = VdpWriteFifo::default();
    for data in [0, 0x1234, 0xffff] {
        for consumed in [false, true] {
            memory.write_word(command(0, 1), data).unwrap();
            let mut current = command(0, 0);
            let mut cache = VdpReadCache::default();
            cache
                .complete_read(&memory, &fifo, &mut current, Some(2))
                .unwrap();
            let ready_copy = cache;
            if consumed {
                assert_eq!(cache.read(), Ok(data));
            }
            for _ in 0..2 {
                cache.invalidate();
                assert!(!cache.is_ready());
                assert_eq!(cache.buffered_word(), Some(data));
                assert_eq!(cache.read(), Err(VdpReadError::Empty));
                assert_eq!(current, command(2, 0));
            }
            assert!(ready_copy.is_ready());
            assert_eq!(ready_copy.buffered_word(), Some(data));
            memory.write_word(command(2, 1), data ^ 0xffff).unwrap();
            assert_eq!(
                cache.complete_read(&memory, &fifo, &mut current, Some(2)),
                Ok(data ^ 0xffff)
            );
            assert_eq!(cache.read(), Ok(data ^ 0xffff));
            assert_eq!(current, command(4, 0));
        }
    }
}

pub fn accepted_control_writes_require_another_fetch() {
    let mut memory = VdpMemory::default();
    memory.write_word(command(0, 1), 0x1111).unwrap();
    memory.write_word(command(2, 1), 0x2222).unwrap();
    let fifo = VdpWriteFifo::default();
    for word in [0x0002, 0x4002, 0xc002, 0x8f02, 0xaf02] {
        let mut current = command(0, 0);
        let mut cache = VdpReadCache::default();
        cache
            .complete_read(&memory, &fifo, &mut current, Some(2))
            .unwrap();
        let mut control = VdpControl::default();
        control_write(&mut cache, &mut control, &mut current, word).unwrap();
        assert!(!cache.is_ready());
        assert_eq!(cache.buffered_word(), Some(0x1111));
        assert_eq!(cache.read(), Err(VdpReadError::Empty));
        let before = current;
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(2)),
            Err(VdpReadError::UnsupportedCommand(before))
        );
        assert_eq!(current, before);
        if control.pending_word().is_some() {
            control.cancel_pending();
        }
        control_write(&mut cache, &mut control, &mut current, 0x0002).unwrap();
        assert_eq!(
            control_write(&mut cache, &mut control, &mut current, 0),
            Ok(ControlWrite::Command(command(2, 0)))
        );
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(2)),
            Ok(0x2222)
        );
        assert_eq!(cache.read(), Ok(0x2222));
        assert_eq!(current, command(4, 0));
    }
    // Seeded component states, not physical fetches between control halves.
    for (first, second, old_code, raw, applied) in [
        (0x0002, 0, 0, command(2, 0), command(2, 0)),
        (0x4002, 0x8f02, 0, command(0x8002, 1), command(0x8002, 1)),
        (0x0002, 0x80, 0, command(2, 0x20), command(2, 0)),
        (0x0002, 0, 0x20, command(2, 0), command(2, 0x20)),
    ] {
        let mut current = command(0, 0);
        let mut cache = VdpReadCache::default();
        cache
            .complete_read(&memory, &fifo, &mut current, Some(2))
            .unwrap();
        let ready = cache;
        let mut control = VdpControl::default();
        control.write_control(first);
        assert_eq!(cache, ready);
        current = command(
            u32::from(first & 0x3fff),
            old_code | (first >> 14) as u8 | 0x10,
        );
        assert_eq!(
            control_write(&mut cache, &mut control, &mut current, second),
            Ok(ControlWrite::Command(raw))
        );
        assert_eq!(current, applied);
        assert_eq!(control.pending_word(), None);
        assert!(!cache.is_ready());
        assert_eq!(cache.buffered_word(), Some(0x1111));
        assert_eq!(cache.read(), Err(VdpReadError::Empty));
    }
    for pending in [false, true] {
        for invalid in [command(0, 0x40), command(0x20000, 0)] {
            let mut current = command(0, 0);
            let mut cache = VdpReadCache::default();
            cache
                .complete_read(&memory, &fifo, &mut current, Some(2))
                .unwrap();
            let mut control = VdpControl::default();
            if pending {
                control.write_control(0x0002);
            }
            current = invalid;
            let before_control = control;
            let before_cache = cache;
            assert!(control_write(&mut cache, &mut control, &mut current, 0).is_err());
            assert_eq!(control, before_control);
            assert_eq!(cache, before_cache);
            assert_eq!(current, invalid);
            assert_eq!(cache.read(), Ok(0x1111));
        }
    }
}

pub fn accepted_data_writes_invalidate_after_capture() {
    for code in [1, 3, 5, 0x11, 0x13, 0x15] {
        let mut memory = VdpMemory::default();
        memory.write_word(command(0, 1), 0xabcd).unwrap();
        let mut fifo = VdpWriteFifo::default();
        let mut cache = VdpReadCache::default();
        let mut current = command(0, 0);
        cache
            .complete_read(&memory, &fifo, &mut current, Some(2))
            .unwrap();
        let mut control = VdpControl::default();
        control.write_control(0x4000);
        current = command(2, code);
        data_write(
            &mut cache,
            &mut control,
            &mut fifo,
            &mut current,
            Some(2),
            0x1234,
        )
        .unwrap();
        assert_eq!(current, command(4, code));
        assert_eq!(control.pending_word(), None);
        assert!(!cache.is_ready());
        assert_eq!(cache.buffered_word(), Some(0xabcd));
        assert_eq!(cache.read(), Err(VdpReadError::Empty));
        assert_eq!(memory.read_word(command(2, 0)), Ok(0));
        let captured = fifo.front().unwrap();
        assert_eq!(captured.command, command(2, code));
        assert_eq!(captured.data, 0x1234);
        current = command(2, 0);
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(2)),
            Err(VdpReadError::FifoPending)
        );
        assert_eq!(cache.buffered_word(), Some(0xabcd));
        assert_eq!(fifo.complete_write(&mut memory), Ok(Some(captured)));
        let expected = if code & 0x0f == 1 { 0x1234 } else { 0 };
        assert_eq!(
            cache.complete_read(&memory, &fifo, &mut current, Some(2)),
            Ok(expected)
        );
        assert_eq!(cache.read(), Ok(expected));
        assert_eq!(current, command(4, 0));
    }
    let mut memory = VdpMemory::default();
    memory.write_word(command(0, 1), 0xabcd).unwrap();
    for (current, increment, full, error) in [
        (command(2, 1), None, false, VdpWriteError::UnknownIncrement),
        (
            command(2, 0),
            Some(2),
            false,
            VdpWriteError::UnsupportedCommand(command(2, 0)),
        ),
        (
            command(2, 0x80),
            Some(2),
            false,
            VdpWriteError::InvalidCode(0x80),
        ),
        (
            command(0x20000, 1),
            Some(2),
            false,
            VdpWriteError::InvalidAddress(0x20000),
        ),
        (
            command(0x20001, 1),
            Some(2),
            false,
            VdpWriteError::InvalidAddress(0x20001),
        ),
        (command(2, 1), Some(2), true, VdpWriteError::Full),
    ] {
        let mut cache = VdpReadCache::default();
        cache
            .complete_read(
                &memory,
                &VdpWriteFifo::default(),
                &mut command(0, 0),
                Some(2),
            )
            .unwrap();
        let saved_cache = cache;
        let mut control = VdpControl::default();
        control.write_control(0x4000);
        let saved_control = control;
        let mut fifo = VdpWriteFifo::default();
        let mut fill = command(8, 1);
        if full {
            for _ in 0..4 {
                fifo.push_data(&mut fill, Some(2), 0).unwrap();
            }
        }
        let saved_fifo = fifo.clone();
        let mut rejected = current;
        assert_eq!(
            data_write(
                &mut cache,
                &mut control,
                &mut fifo,
                &mut rejected,
                increment,
                0x1234
            ),
            Err(error)
        );
        assert_eq!(rejected, current);
        assert_eq!(cache, saved_cache);
        assert_eq!(control, saved_control);
        assert_eq!(fifo, saved_fifo);
        assert_eq!(cache.read(), Ok(0xabcd));
    }
}
