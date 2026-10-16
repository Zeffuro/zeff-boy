use super::support::*;
use zeff_md_core::{Vdp, VdpDmaError, VdpPortError, VdpScheduleError};

const TIE: u64 = 224 * 3420 + 10;
const FIELD: u64 = 262 * 3420;

fn operation(fill: Option<u16>) -> (u8, u8, usize) {
    if fill.is_some() {
        (0xbf, 0x31, 1)
    } else {
        (0xff, 0x30, 2)
    }
}

fn irq(device: &Vdp, vertical: bool, h_enable: bool, v_enable: bool) {
    assert!(device.interrupts().horizontal_pending());
    assert_eq!(device.interrupts().vertical_pending(), vertical);
    assert_eq!(
        device.interrupts().status_bits(),
        if vertical { 0x80 } else { 0 }
    );
    assert_eq!(device.interrupts().line_counter(), 0);
    assert_eq!(
        device.interrupt_level(),
        if vertical && v_enable {
            6
        } else if h_enable {
            4
        } else {
            0
        }
    );
}

fn configured(length: u16, fill: Option<u16>, h_enable: bool, v_enable: bool) -> Vdp {
    let (mode, _, _) = operation(fill);
    let mut device = fixture(length, 0, mode, 1);
    register(&mut device, 0, if h_enable { 0x10 } else { 0 });
    register(&mut device, 1, if v_enable { 0x24 } else { 4 });
    device
}

pub fn irq_tie_phases_with(run: &mut Run<'_>) {
    for phase in 0..3 {
        let fill = (phase == 0).then_some(0x90ff);
        let (mode, code, _) = operation(fill);
        let start = if phase == 2 { TIE - 21 } else { TIE - 1 };
        for length in [1, 2] {
            for h_enable in [false, true] {
                for v_enable in [false, true] {
                    let mut device = configured(length, fill, h_enable, v_enable);
                    advance(run, &mut device, start);
                    let before = device.memory().clone();
                    let pointer = device.memory().vram().as_ptr();
                    seed(run, &mut device, 0x2000, code, fill);
                    let mut whole = device.clone();
                    reject(
                        run,
                        &mut device,
                        Op::Acknowledge(TIE),
                        Error::Schedule(VdpScheduleError::CoincidentMemory(tick(TIE))),
                    );
                    advance(run, &mut device, TIE - 1);
                    assert_eq!(device.memory(), &before);
                    counters(&device, length, 0, mode);
                    irq(&device, false, h_enable, v_enable);
                    advance(run, &mut device, TIE);
                    let writes = u16::from(phase != 1);
                    counters(&device, length - writes, writes, mode);
                    let terminal = writes == length;
                    assert_eq!(
                        device.command(),
                        command(
                            0x2000 + u32::from(writes),
                            if terminal { code & !0x20 } else { code }
                        )
                    );
                    let mut expected = [0; 65536];
                    expected.copy_from_slice(before.vram());
                    if writes != 0 {
                        expected[0x2001] = if fill.is_some() { 0x90 } else { 0x34 };
                    }
                    assert_eq!(device.memory().vram(), &expected);
                    assert_eq!(device.memory().cram(), before.cram());
                    assert_eq!(device.memory().vsram(), before.vsram());
                    assert_eq!(device.memory().vram().as_ptr(), pointer);
                    irq(&device, true, h_enable, v_enable);
                    busy_status(run, &mut device, !terminal);
                    irq(&device, true, h_enable, v_enable);
                    if !terminal {
                        reject(
                            run,
                            &mut device,
                            Op::Data(0x1234),
                            Error::Port(VdpPortError::Dma(VdpDmaError::Busy)),
                        );
                    }
                    advance(run, &mut device, TIE + 1);
                    advance(run, &mut whole, TIE + 1);
                    assert_eq!(device, whole);
                    let level = if v_enable {
                        6
                    } else if h_enable {
                        4
                    } else {
                        0
                    };
                    assert_eq!(
                        run(&mut device, Op::Acknowledge(TIE + 2)),
                        Ok(Value::Level(level))
                    );
                    assert_eq!(device.interrupts().vertical_pending(), level != 6);
                    assert_eq!(device.interrupts().horizontal_pending(), level != 4);
                    assert_eq!(device.memory(), whole.memory());
                    let finish = if phase == 0 {
                        TIE + 20 * u64::from(length - 1)
                    } else if phase == 1 {
                        TIE + 20 * (2 * u64::from(length) - 1)
                    } else {
                        TIE + 40 * u64::from(length - 1)
                    };
                    advance(run, &mut whole, finish.max(TIE + 1));
                    counters(&whole, 0, length, mode);
                    expected[0x2001] = if fill.is_some() { 0x90 } else { 0x34 };
                    if length == 2 {
                        expected[0x2000] = if fill.is_some() { 0x90 } else { 0x12 };
                    }
                    assert_eq!(whole.memory().vram(), &expected);
                    assert_eq!(
                        whole.command(),
                        command(0x2000 + u32::from(length), code & !0x20)
                    );
                    irq(&whole, true, h_enable, v_enable);
                }
            }
        }
    }
}

fn literal_fixture() -> [u8; 65536] {
    let mut expected = [0; 65536];
    for (address, bytes) in [
        (0, [0x12, 0x34]),
        (2, [0x56, 0x78]),
        (0xfffe, [0xab, 0xcd]),
        (0x100, [0x10, 0x20]),
        (0x102, [0x30, 0x40]),
        (0x104, [0x50, 0x60]),
    ] {
        expected[address..address + 2].copy_from_slice(&bytes);
    }
    expected
}

pub fn zero_count_completion_with(run: &mut Run<'_>) {
    for fill in [Some(0x90ff), None] {
        let (mode, code, stride) = operation(fill);
        let mut device = fixture(0, 0xffff, mode, 0);
        register(&mut device, 0, 0x10);
        register(&mut device, 1, 0x24);
        let pointer = device.memory().vram().as_ptr();
        let before = device.memory().clone();
        assert_eq!(before.vram(), &literal_fixture());
        seed(run, &mut device, 0x2000, code, fill);
        let mut split = device.clone();
        let requested = (0..65536 * stride).fold(device.now().get(), |at, _| slot_after(at));
        assert!(requested > FIELD);
        advance(run, &mut device, requested);
        for at in (FIELD..requested).step_by(FIELD as usize) {
            advance(run, &mut split, at);
            assert!(split.interrupts().vertical_pending());
        }
        advance(run, &mut split, requested);
        assert_eq!(device, split);
        let mut expected = literal_fixture();
        expected[0x2001] = if fill.is_some() { 0x90 } else { 0xcd };
        assert_eq!(device.memory().vram(), &expected);
        assert_eq!(device.memory().cram(), before.cram());
        assert_eq!(device.memory().vsram(), before.vsram());
        assert_eq!(device.memory().vram().as_ptr(), pointer);
        counters(&device, 0, 0xffff, mode);
        assert_eq!(device.command(), command(0x2000, code & !0x20));
        irq(&device, true, true, true);
        busy_status(run, &mut device, false);
        assert_eq!(
            run(&mut device, Op::Acknowledge(requested + 1)),
            Ok(Value::Level(6))
        );
        assert!(!device.interrupts().vertical_pending());
        assert!(device.interrupts().horizontal_pending());
        assert_eq!(device.interrupt_level(), 4);
    }
}

pub fn irq_tie_chunk_failure_with(run: &mut Run<'_>) {
    for fill in [Some(0x90ff), None] {
        let (mode, code, stride) = operation(fill);
        let mut slots = vec![TIE];
        let mut at = TIE - 1;
        while slots.len() < 10 * stride {
            at -= 1;
            if slot_after(at - 1) == at {
                slots.push(at);
            }
        }
        slots.reverse();
        let start = slots[slots.len() - 9 * stride] - 1;
        let mut device = configured(10, fill, true, true);
        advance(run, &mut device, start);
        seed(run, &mut device, 0x3ff6, code, fill);
        let before = device.memory().clone();
        let pointer = device.memory().vram().as_ptr();
        let error_at = slot_after(slot_after(TIE));
        let mut split = device.clone();
        if fill.is_none() {
            advance(run, &mut split, TIE - 20);
            counters(&split, 2, 8, mode);
            assert_eq!(split.command(), command(0x3ffe, code));
        }
        advance(run, &mut device, TIE);
        advance(run, &mut split, TIE);
        assert_eq!(device, split);
        counters(&device, 1, 9, mode);
        assert_eq!(device.command(), command(0x3fff, code));
        let mut expected = [0; 65536];
        expected.copy_from_slice(before.vram());
        for (index, byte) in [0x34, 0x12, 0x78, 0x56, 0, 0, 0, 0, 0]
            .into_iter()
            .enumerate()
        {
            expected[(0x3ff6 + index) ^ 1] = if fill.is_some() { 0x90 } else { byte };
        }
        assert_eq!(device.memory().vram(), &expected);
        assert_eq!(device.memory().vram().as_ptr(), pointer);
        irq(&device, true, true, true);
        advance(run, &mut device, error_at);
        advance(run, &mut split, error_at);
        assert_eq!(device, split);
        counters(&device, 0, 10, mode);
        assert_eq!(device.command(), command(0x4000, code & !0x20));
        expected[0x3fff ^ 1] = if fill.is_some() { 0x90 } else { 0 };
        assert_eq!(device.memory().vram(), &expected);
        irq(&device, true, true, true);
    }
}
