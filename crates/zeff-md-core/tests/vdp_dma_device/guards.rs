use super::support::*;
use zeff_md_core::{
    ControlWrite, VdpDmaError, VdpInterruptError, VdpLineError, VdpPortError, VdpRenderError,
    VdpScheduleError, VdpTimingError, VdpVerticalTiming,
};

fn dma(error: VdpDmaError) -> Error {
    Error::Port(VdpPortError::Dma(error))
}

fn timing(error: VdpTimingError) -> VdpScheduleError {
    VdpScheduleError::Interrupt(VdpInterruptError::Timing(error))
}

pub fn seed_validation_with(run: &mut Run<'_>) {
    for fill in [Some(0xabcd), None] {
        let (mode, valid) = if fill.is_some() {
            (0x80, 0x21)
        } else {
            (0xc0, 0x30)
        };
        let base = fixture(2, 0, mode, 1);
        for code in 0..64u8 {
            if (fill.is_some() && matches!(code, 0x21 | 0x31)) || (fill.is_none() && code == 0x30) {
                continue;
            }
            let mut device = base.clone();
            let target = command(0x2000, code);
            reject(
                run,
                &mut device,
                Op::Seed(target, fill),
                dma(VdpDmaError::UnsupportedCommand(target)),
            );
        }
        for address in [0x20000, 0x20001, u32::MAX] {
            let mut device = base.clone();
            let target = command(address, valid);
            reject(
                run,
                &mut device,
                Op::Seed(target, fill),
                dma(VdpDmaError::UnsupportedCommand(target)),
            );
        }
        for address in [0x3fff, 0x7fff, 0xbfff, 0xffff] {
            let mut device = base.clone();
            seed(run, &mut device, address, valid, fill);
            assert_eq!(device.command(), command(address, valid));
            advance(run, &mut device, slot(if fill.is_some() { 2 } else { 4 }));
            assert_eq!(
                device.command(),
                command((address + 2) & 0x1ffff, valid & !0x20)
            );
            counters(&device, 0, 2, mode);
        }
        let mut device = base.clone();
        reject(
            run,
            &mut device,
            Op::Seed(
                command(0x2000, valid),
                if fill.is_some() { None } else { Some(0xabcd) },
            ),
            dma(VdpDmaError::WrongOperation(mode)),
        );
    }
    let mut unsupported = fixture(2, 0, 0x7f, 1);
    reject(
        run,
        &mut unsupported,
        Op::Seed(command(0x2000, 0x21), Some(0xabcd)),
        dma(VdpDmaError::WrongOperation(0x7f)),
    );
    for index in [15, 19, 20, 21, 22, 23] {
        let mut unknown = known(0, START, 4, Some(index), false);
        reject(
            run,
            &mut unknown,
            Op::Seed(command(0x2000, 0x21), Some(0xabcd)),
            dma(VdpDmaError::UnknownRegister(index)),
        );
    }
    let mut mode = fixture(2, 0, 0x80, 1);
    register(&mut mode, 1, 0x84);
    reject(
        run,
        &mut mode,
        Op::Seed(command(0x2000, 0x21), Some(0xabcd)),
        Error::Port(VdpPortError::Schedule(timing(
            VdpTimingError::UnsupportedMode {
                index: 1,
                value: 0x84,
            },
        ))),
    );
    let mut history = known(0, START, 0x44, None, false);
    seed(run, &mut history, 0x2000, 0x21, Some(0xabcd));
    advance(run, &mut history, 2 * 3420);
    counters(&history, 0, 2, 0x80);
    advance(run, &mut history, 2 * 3420 + 1);
    register(&mut history, 1, 4);
    register(&mut history, 19, 2);
    register(&mut history, 21, 0);
    seed(run, &mut history, 0x2000, 0x21, Some(0xabcd));
    advance(run, &mut history, 3 * 3420);
    counters(&history, 0, 2, 0x80);
}

pub fn pending_inputs_with(run: &mut Run<'_>) {
    let target = command(0x2000, 0x21);
    let mut parser = fixture(2, 0, 0x80, 1);
    run(&mut parser, Op::Control(0x2000)).unwrap();
    reject(
        run,
        &mut parser,
        Op::Seed(target, Some(0xabcd)),
        dma(VdpDmaError::PendingInputs),
    );
    let mut fifo = fixture(2, 0, 0x80, 1);
    port_command(&mut fifo, 0x2100, 1);
    assert_eq!(run(&mut fifo, Op::Data(0x5678)), Ok(Value::Unit));
    for phase in 0..2 {
        if phase != 0 {
            advance(run, &mut fifo, 1010);
        }
        assert!(!fifo.fifo().is_empty());
        reject(
            run,
            &mut fifo,
            Op::Seed(target, Some(0xabcd)),
            dma(VdpDmaError::PendingInputs),
        );
    }
    let mut read = fixture(2, 0, 0x80, 2);
    port_command(&mut read, 0, 0);
    for at in [START, 1010, 1030] {
        advance(run, &mut read, at);
        reject(
            run,
            &mut read,
            Op::Seed(target, Some(0xabcd)),
            dma(VdpDmaError::PendingInputs),
        );
    }
    assert!(read.read_cache().is_ready());
    for index in 0..24u8 {
        let mut pending = fixture(2, 0, 0x80, 1);
        let value = match index {
            1 => 4,
            12 => 0x81,
            _ => 0,
        };
        run(&mut pending, Op::Register(index, value)).unwrap();
        let error = if index == 12 {
            Error::Port(VdpPortError::Schedule(timing(
                VdpTimingError::PendingModeChange,
            )))
        } else {
            dma(VdpDmaError::PendingInputs)
        };
        reject(run, &mut pending, Op::Seed(target, Some(0xabcd)), error);
    }
}

pub fn chronological_capacity_with(run: &mut Run<'_>) {
    for increment in [0, 1] {
        for fill in [Some(0xabcd), None] {
            let (mode, code, opportunities) = if fill.is_some() {
                (0x80, 0x21, 9)
            } else {
                (0xc0, 0x30, 18)
            };
            let mut device = fixture(9, 0, mode, increment);
            seed(run, &mut device, 0x2000, code, fill);
            let mut complete = device.clone();
            advance(run, &mut complete, slot(opportunities));
            let permitted = if fill.is_some() { 8 } else { 17 };
            advance(run, &mut device, slot(permitted));
            counters(&device, 1, 8, mode);
            assert_eq!(
                device.command(),
                command(0x2000 + 8 * u32::from(increment), code)
            );
            busy_status(run, &mut device, true);
            if fill.is_some() {
                advance(run, &mut device, 1189);
                counters(&device, 1, 8, mode);
            }
            advance(run, &mut device, slot(opportunities));
            counters(&device, 0, 9, mode);
            busy_status(run, &mut device, false);
            assert_eq!(
                device.command(),
                command(0x2000 + 9 * u32::from(increment), code & !0x20)
            );
            assert_eq!(device, complete);
        }
    }
    for fill in [Some(0xabcd), None] {
        let (mode, code, opportunities) = if fill.is_some() {
            (0x80, 0x21, 8)
        } else {
            (0xc0, 0x30, 16)
        };
        let mut device = fixture(8, 0, mode, 1);
        seed(run, &mut device, 0x2000, code, fill);
        advance(run, &mut device, slot(opportunities));
        counters(&device, 0, 8, mode);
        busy_status(run, &mut device, false);
    }
}

pub fn staged_failures_and_iack_with(run: &mut Run<'_>) {
    for fill in [Some(0xabcd), None] {
        let (mode, code, opportunities) = if fill.is_some() {
            (0x80, 0x21, 2)
        } else {
            (0xc0, 0x30, 4)
        };
        let mut carry = fixture(2, 0, mode, 1);
        seed(run, &mut carry, 0x3ffe, code, fill);
        advance(run, &mut carry, slot(opportunities));
        counters(&carry, 0, 2, mode);
        assert_eq!(carry.command(), command(0x4000, code & !0x20));
        let mut device = fixture(2, 0, mode, 1);
        seed(run, &mut device, 0x2000, code, fill);
        for endpoint in [slot(1), slot(opportunities)] {
            reject(
                run,
                &mut device,
                Op::Acknowledge(endpoint),
                Error::Schedule(VdpScheduleError::CoincidentMemory(tick(endpoint))),
            );
        }
        reject(
            run,
            &mut device,
            Op::Acknowledge(2660),
            Error::Schedule(VdpScheduleError::CoincidentAcknowledge(tick(2660))),
        );
        assert_eq!(
            run(&mut device, Op::Acknowledge(slot(opportunities) + 1)),
            Ok(Value::Level(0))
        );
        counters(&device, 0, 2, mode);
        busy_status(run, &mut device, false);
    }
}

pub fn clock_guards_with(run: &mut Run<'_>) {
    for fill in [Some(0xabcd), None] {
        let (mode, code) = if fill.is_some() {
            (0x80, 0x21)
        } else {
            (0xc0, 0x30)
        };
        let mut reversed = fixture(2, 0, mode, 1);
        seed(run, &mut reversed, 0x2000, code, fill);
        for at in [START, slot(1)] {
            advance(run, &mut reversed, at);
            reject(
                run,
                &mut reversed,
                Op::Advance(at - 1),
                Error::Schedule(VdpScheduleError::Interrupt(
                    VdpInterruptError::TimeReversal {
                        current: tick(at),
                        requested: tick(at - 1),
                    },
                )),
            );
        }
        let mut high = known(u64::MAX - 995, u64::MAX - 61, 4, None, false);
        if fill.is_none() {
            register(&mut high, 23, 0xc0);
        }
        high.advance_to(tick(u64::MAX - 21), REGION).unwrap();
        seed(run, &mut high, 0x2000, code, fill);
        let before = high.memory().clone();
        advance(run, &mut high, u64::MAX);
        let completed = u16::from(fill.is_some());
        counters(&high, 2 - completed, completed, mode);
        assert_eq!(high.command(), command(0x2000 + u32::from(completed), code));
        if fill.is_some() {
            assert_eq!(high.memory().vram()[0x2001], 0xab);
        } else {
            assert_eq!(high.memory(), &before);
        }
        reject(
            run,
            &mut high,
            Op::Seed(command(0x2100, code), fill),
            dma(VdpDmaError::Busy),
        );
        advance(run, &mut high, u64::MAX);
    }
    let mut no_next = known(u64::MAX - 995, u64::MAX - 1, 4, None, false);
    reject(
        run,
        &mut no_next,
        Op::Seed(command(0x2000, 0x21), Some(0xabcd)),
        Error::Port(VdpPortError::Schedule(timing(
            VdpTimingError::ClockOverflow,
        ))),
    );
}

pub fn capture_and_future_margin_with(run: &mut Run<'_>) {
    let mut capture = known(0, START, 0x44, None, true);
    let calendar = VdpVerticalTiming::at_counters_zero(tick(0));
    let first = calendar
        .next_memory_slot(capture.now(), REGION, capture.registers())
        .unwrap();
    seed(run, &mut capture, 0x2000, 0x21, Some(0xabcd));
    reject(
        run,
        &mut capture,
        Op::Capture(first.get() + 1),
        Error::Line(VdpLineError::Render(VdpRenderError::PendingLineInputs)),
    );
    advance(run, &mut capture, first.get());
    counters(&capture, 1, 1, 0x80);
    assert_eq!(capture.memory().vram()[0x2001], 0xab);
    let mut at = tick(221 * 3420);
    let bank = known(0, at.get(), 0x44, None, true);
    let mut previous = at;
    while let Ok(next) = calendar.next_memory_slot(at, REGION, bank.registers()) {
        previous = at;
        at = next;
    }
    let mut margin = bank;
    margin
        .advance_to(tick(margin.now().get() + 1), REGION)
        .unwrap();
    register(&mut margin, 19, 0);
    margin.advance_to(tick(previous.get() - 1), REGION).unwrap();
    seed(run, &mut margin, 0x2000, 0x21, Some(0xabcd));
    advance(run, &mut margin, 222 * 3420);
    assert_eq!(margin.now(), tick(222 * 3420));
    assert_eq!(margin.memory().vram()[0x2001], 0xab);
    assert_eq!(margin.command().code & 0x20, 0x20);
}

pub fn ordinary_cpu_dma_stays_guarded_with(run: &mut Run<'_>) {
    for (mode, code) in [(0x80, 0x21), (0xc0, 0x30)] {
        let mut device = fixture(2, 0, mode, 1);
        register(&mut device, 1, 0x14);
        let address = 0x2000;
        run(
            &mut device,
            Op::Control(address | (u16::from(code & 3) << 14)),
        )
        .unwrap();
        assert_eq!(
            run(&mut device, Op::Control(u16::from(code & 0x3c) << 2)),
            Ok(Value::Control(ControlWrite::Command(command(
                u32::from(address),
                code
            ))))
        );
        busy_status(run, &mut device, true);
    }
    let mut device = fixture(2, 0, 0x80, 1);
    let memory = device.memory().clone();
    assert_eq!(
        run(&mut device, Op::Control(0x9700)),
        Ok(Value::Control(ControlWrite::Register {
            index: 23,
            value: 0
        }))
    );
    assert_eq!(device.registers().registers()[23], Some(0x80));
    let pending = device.registers().pending().unwrap();
    assert_eq!((pending.index, pending.value), (23, 0));
    advance(run, &mut device, pending.publish_at.get());
    counters(&device, 2, 0, 0);
    assert_eq!(device.memory(), &memory);
}
