use super::support::*;
use zeff_md_core::{
    ControlWrite, Vdp, VdpDmaError, VdpInterruptError, VdpLineError, VdpPortError,
    VdpScheduleError, VdpTimingError, VdpWriteError,
};

fn fixture_enabled(length: u16, source: u16, mode: u8, increment: u8) -> Vdp {
    let mut device = fixture(length, source, mode, increment);
    register(&mut device, 1, 0x14);
    device
}

fn first_word(address: u32, code: u8) -> u16 {
    (address as u16 & 0x3fff) | (u16::from(code & 3) << 14)
}

fn second_word(address: u32, code: u8) -> u16 {
    ((address >> 14) as u16 & 7) | (u16::from(code & 0x3c) << 2)
}

fn arm(run: &mut Run<'_>, device: &mut Vdp, address: u32, code: u8) {
    assert!(run(device, Op::Control(first_word(address, code))).is_ok());
    let target = command(address, code);
    assert_eq!(
        run(device, Op::Control(second_word(address, code))),
        Ok(Value::Control(ControlWrite::Command(target)))
    );
    assert_eq!(device.command(), target);
    assert_eq!(device.control().pending_word(), None);
}

fn status(run: &mut Run<'_>, device: &mut Vdp, busy: bool, empty: bool) {
    let Value::Word(word) = run(device, Op::Status).unwrap() else {
        panic!("status word")
    };
    assert_eq!(word & 2, u16::from(busy) * 2);
    assert_eq!(word & 0x200, u16::from(empty) * 0x200);
}

fn dma(error: VdpDmaError) -> Error {
    Error::Port(VdpPortError::Dma(error))
}

fn busy_ports(run: &mut Run<'_>, device: &mut Vdp, data: bool) {
    for op in [Op::Control(0x8f02), Op::Read, Op::ReadWait] {
        reject(run, device, op, dma(VdpDmaError::Busy));
    }
    if data {
        for op in [Op::Data(0x5678), Op::WriteWait] {
            reject(run, device, op, dma(VdpDmaError::Busy));
        }
    }
    for index in 0..24 {
        reject(
            run,
            device,
            Op::Register(index, 0),
            Error::Schedule(VdpScheduleError::Dma(VdpDmaError::Busy)),
        );
    }
}

pub fn copy_command_start_with(run: &mut Run<'_>) {
    for parity in [0, 1] {
        let mut device = fixture_enabled(2, 0, 0xe5, 1);
        let memory = device.memory().clone();
        let pointer = device.memory().vram().as_ptr();
        let first = slot_after(device.now().get());
        arm(run, &mut device, 0x2000 + parity, 0x30);
        counters(&device, 2, 0, 0xe5);
        assert_eq!(device.memory(), &memory);
        status(run, &mut device, true, true);
        advance(run, &mut device, first - 1);
        assert_eq!(device.memory(), &memory);
        advance(run, &mut device, first);
        assert_eq!(device.memory(), &memory);
        counters(&device, 2, 0, 0xe5);
        let mut read_clone = device.clone();
        busy_ports(run, &mut device, true);
        let second = slot_after(first);
        advance(run, &mut device, second);
        assert_eq!(device.memory().vram()[(0x2000 + parity) as usize ^ 1], 0x34);
        counters(&device, 1, 1, 0xe5);
        assert_eq!(device.command(), command(0x2001 + parity, 0x30));
        let third = slot_after(second);
        advance(run, &mut device, third);
        counters(&device, 1, 1, 0xe5);
        let fourth = slot_after(third);
        advance(run, &mut device, fourth);
        advance(run, &mut read_clone, fourth);
        assert_eq!(device, read_clone);
        assert_eq!(device.memory().vram()[(0x2001 + parity) as usize ^ 1], 0x12);
        counters(&device, 0, 2, 0xe5);
        assert_eq!(device.command(), command(0x2002 + parity, 0x10));
        status(run, &mut device, false, true);
        assert_eq!(device.memory().vram().as_ptr(), pointer);
    }
}

pub fn fill_wait_and_trigger_with(run: &mut Run<'_>) {
    for code in [0x21, 0x31] {
        for increment in [0, 1, 2] {
            for parity in [0, 1] {
                let address = 0x2000 + parity;
                let mut device = fixture_enabled(2, 0x12ff, 0xa5, increment);
                let memory = device.memory().clone();
                arm(run, &mut device, address, code);
                let waiting_end = slot_after(slot_after(device.now().get())) + 1;
                advance(run, &mut device, waiting_end);
                assert_eq!(device.memory(), &memory);
                assert_eq!(device.command(), command(address, code));
                counters(&device, 2, 0x12ff, 0xa5);
                status(run, &mut device, true, true);
                busy_ports(run, &mut device, false);
                assert_eq!(run(&mut device, Op::WriteWait), Ok(Value::Wait(None)));
                assert_eq!(run(&mut device, Op::Data(0xabcd)), Ok(Value::Unit));
                let start = address + u32::from(increment);
                assert_eq!(device.command(), command(start, code));
                let entry = device.fifo().front().unwrap();
                assert_eq!(entry.command, command(address, code & !0x20));
                assert_eq!(entry.data, 0xabcd);
                counters(&device, 2, 0x12ff, 0xa5);
                status(run, &mut device, true, false);
                busy_ports(run, &mut device, false);
                let first = slot_after(device.now().get());
                advance(run, &mut device, first - 1);
                assert_eq!(device.memory(), &memory);
                advance(run, &mut device, first);
                let first_byte = (address as usize & !1) + 1;
                assert_eq!(
                    device.memory().vram()[first_byte],
                    if parity == 0 { 0xcd } else { 0xab }
                );
                assert_eq!(device.memory().vram()[first_byte - 1], 0);
                counters(&device, 2, 0x12ff, 0xa5);
                let second = slot_after(first);
                advance(run, &mut device, second);
                assert!(device.fifo().is_empty());
                busy_ports(run, &mut device, true);
                assert_eq!(device.memory().vram()[address as usize], 0xab);
                assert_eq!(device.memory().vram()[address as usize ^ 1], 0xcd);
                counters(&device, 2, 0x12ff, 0xa5);
                assert_eq!(device.command(), command(start, code));
                let third = slot_after(second);
                advance(run, &mut device, third - 1);
                counters(&device, 2, 0x12ff, 0xa5);
                advance(run, &mut device, third);
                assert_eq!(device.memory().vram()[start as usize ^ 1], 0xab);
                counters(&device, 1, 0x1300, 0xa5);
                let fourth = slot_after(third);
                advance(run, &mut device, fourth);
                assert_eq!(
                    device.memory().vram()[(start + u32::from(increment)) as usize ^ 1],
                    0xab
                );
                counters(&device, 0, 0x1301, 0xa5);
                assert_eq!(
                    device.command(),
                    command(address + 3 * u32::from(increment), code & !0x20)
                );
                status(run, &mut device, false, true);
                assert_eq!(device.memory().cram(), memory.cram());
                assert_eq!(device.memory().vsram(), memory.vsram());
            }
        }
    }
}

pub fn fill_fifo_priority_with(run: &mut Run<'_>) {
    for trigger in [false, true] {
        let mut device = fixture_enabled(2, 0, 0x80, 2);
        arm(run, &mut device, 0x2100, 1);
        for word in [0x1234, 0x5678] {
            assert_eq!(run(&mut device, Op::Data(word)), Ok(Value::Unit));
        }
        arm(run, &mut device, 0x2000, 0x21);
        if trigger {
            assert_eq!(run(&mut device, Op::Data(0xabcd)), Ok(Value::Unit));
        }
        let fifo_slots: usize = if trigger { 6 } else { 4 };
        let mut at = device.now().get();
        for index in 0..fifo_slots {
            at = slot_after(at);
            advance(run, &mut device, at);
            counters(&device, 2, 0, 0x80);
            assert_eq!(device.memory().vram()[0x2003], 0);
            assert_eq!(device.fifo().len(), (fifo_slots - index - 1).div_ceil(2));
        }
        assert_eq!(
            &device.memory().vram()[0x2100..0x2104],
            &[0x12, 0x34, 0x56, 0x78]
        );
        assert_eq!(
            device.command(),
            command(if trigger { 0x2002 } else { 0x2000 }, 0x21)
        );
        status(run, &mut device, true, true);
        let mut expected = [0; 65536];
        expected.copy_from_slice(device.memory().vram());
        let destination = if trigger { 0x2002 } else { 0x2000 };
        let byte = if trigger { 0xab } else { 0x56 };
        expected[destination ^ 1] = byte;
        expected[(destination + 2) ^ 1] = byte;
        advance(run, &mut device, slot_after(slot_after(at)));
        assert_eq!(device.memory().vram(), &expected);
        counters(&device, 0, 2, 0x80);
        status(run, &mut device, false, true);
    }
}

pub fn command_guards_and_busy_with(run: &mut Run<'_>) {
    for (mode, code) in [(0x80, 0x21), (0xc0, 0x30)] {
        let mut disabled = fixture(2, 0, mode, 1);
        assert!(run(&mut disabled, Op::Control(first_word(0x2000, code))).is_ok());
        if mode == 0x80 {
            assert!(run(&mut disabled, Op::Control(second_word(0x2000, code))).is_ok());
            assert_eq!(disabled.command(), command(0x2000, 1));
        } else {
            reject(
                run,
                &mut disabled,
                Op::Control(second_word(0x2000, code)),
                Error::Port(VdpPortError::Control(VdpWriteError::UnsupportedCommand(
                    command(0x2000, 0x10),
                ))),
            );
        }
        status(run, &mut disabled, false, true);
        let base = fixture_enabled(2, 0, mode, 1);
        for invalid in 0x20..0x40 {
            if invalid & 3 == 2
                || (mode == 0x80 && matches!(invalid, 0x21 | 0x31))
                || (mode == 0xc0 && invalid == 0x30)
            {
                continue;
            }
            let mut device = base.clone();
            assert!(run(&mut device, Op::Control(first_word(0x2000, invalid))).is_ok());
            reject(
                run,
                &mut device,
                Op::Control(second_word(0x2000, invalid)),
                dma(VdpDmaError::UnsupportedCommand(command(0x2000, invalid))),
            );
        }
        let mut carry = base.clone();
        arm(run, &mut carry, 0x3ffe, code);
        if mode == 0x80 {
            assert_eq!(run(&mut carry, Op::Data(0xabcd)), Ok(Value::Unit));
        }
        let first = slot_after(carry.now().get());
        advance(run, &mut carry, slot_after(first));
        counters(
            &carry,
            if mode == 0x80 { 2 } else { 1 },
            if mode == 0x80 { 0 } else { 1 },
            mode,
        );
        busy_ports(run, &mut carry, true);
        advance(
            run,
            &mut carry,
            slot_after(slot_after(slot_after(slot_after(first)))),
        );
        assert!(carry.command().address >= 0x4000);
        status(run, &mut carry, false, true);
    }
    for index in [15, 19, 20, 21, 22, 23] {
        let mut unknown = known(0, START, 0x14, Some(index), false);
        assert!(run(&mut unknown, Op::Control(first_word(0x2000, 0x21))).is_ok());
        reject(
            run,
            &mut unknown,
            Op::Control(second_word(0x2000, 0x21)),
            dma(VdpDmaError::UnknownRegister(index)),
        );
    }
    for high in [0, 0x40, 0x7f] {
        let base = fixture_enabled(2, 0, high, 1);
        for code in [0x21, 0x23, 0x25, 0x31, 0x33, 0x35] {
            let mut external = base.clone();
            arm(run, &mut external, 0x2000, code);
            counters(&external, 2, 0, high);
            status(run, &mut external, true, true);
            busy_ports(run, &mut external, true);
            let first = slot_after(external.now().get());
            reject(
                run,
                &mut external,
                Op::Advance(first),
                Error::Schedule(VdpScheduleError::Dma(VdpDmaError::MissingSource)),
            );
        }
    }
    let mut copy_fifo = fixture_enabled(2, 0, 0xc0, 1);
    arm(run, &mut copy_fifo, 0x2100, 1);
    assert_eq!(run(&mut copy_fifo, Op::Data(0x1234)), Ok(Value::Unit));
    assert!(run(&mut copy_fifo, Op::Control(first_word(0x2000, 0x30))).is_ok());
    reject(
        run,
        &mut copy_fifo,
        Op::Control(second_word(0x2000, 0x30)),
        dma(VdpDmaError::PendingInputs),
    );
    let mut pending = fixture_enabled(2, 0, 0x80, 1);
    assert!(run(&mut pending, Op::Register(7, 9)).is_ok());
    assert!(run(&mut pending, Op::Control(first_word(0x2000, 0x21))).is_ok());
    reject(
        run,
        &mut pending,
        Op::Control(second_word(0x2000, 0x21)),
        dma(VdpDmaError::PendingInputs),
    );
    let mut ready = fixture_enabled(2, 0, 0x80, 2);
    arm(run, &mut ready, 0, 0);
    let end = slot_after(slot_after(ready.now().get()));
    advance(run, &mut ready, end);
    assert!(ready.read_cache().is_ready());
    assert!(run(&mut ready, Op::Control(first_word(0x2000, 0x21))).is_ok());
    assert!(!ready.read_cache().is_ready());
    assert_eq!(
        run(&mut ready, Op::Control(second_word(0x2000, 0x21))),
        Ok(Value::Control(ControlWrite::Command(command(0x2000, 0x21))))
    );
    status(run, &mut ready, true, true);
}

pub fn waiting_clones_refresh_and_zero_with(run: &mut Run<'_>) {
    for length in [0, 2] {
        let mut device = fixture_enabled(length, 0xffff, 0xbf, 1);
        advance(run, &mut device, 1141);
        arm(run, &mut device, 0x2000, 0x31);
        let mut waiting = device.clone();
        advance(run, &mut device, 1149);
        advance(run, &mut waiting, 1149);
        assert_eq!(device, waiting);
        reject(
            run,
            &mut device,
            Op::Capture(1150),
            Error::Line(VdpLineError::Schedule(VdpScheduleError::Interrupt(
                VdpInterruptError::Timing(VdpTimingError::UnsupportedMode {
                    index: 1,
                    value: 0x14,
                }),
            ))),
        );
        assert_eq!(run(&mut device, Op::Data(0xabcd)), Ok(Value::Unit));
        assert_eq!(run(&mut waiting, Op::Data(0xabcd)), Ok(Value::Unit));
        advance(run, &mut device, 1150);
        counters(&device, length, 0xffff, 0xbf);
        let mut low = device.clone();
        advance(run, &mut device, 1170);
        assert_eq!(device.memory().vram()[0x2000], 0);
        assert_eq!(device.fifo().len(), 1);
        counters(&device, length, 0xffff, 0xbf);
        advance(run, &mut device, 1190);
        assert_eq!(&device.memory().vram()[0x2000..0x2002], &[0xab, 0xcd]);
        counters(&device, length, 0xffff, 0xbf);
        let mut retired = device.clone();
        advance(run, &mut device, 1210);
        counters(&device, length.wrapping_sub(1), 0, 0xbf);
        advance(run, &mut device, 1230);
        for clone in [&mut waiting, &mut low, &mut retired] {
            advance(run, clone, 1230);
            assert_eq!(device, *clone);
        }
        assert_eq!(
            &device.memory().vram()[0x2000..0x2004],
            &[0xab, 0xcd, 0, 0xab]
        );
        counters(&device, length.wrapping_sub(2), 1, 0xbf);
        assert_eq!(
            device.command(),
            command(0x2003, if length == 0 { 0x31 } else { 0x11 })
        );
        status(run, &mut device, length == 0, true);
    }
}
