use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, Region, Vdp, VdpInterruptError, VdpInterrupts, VdpPortError, VdpRegisterError,
    VdpRegisterPipeline, VdpScheduleError, VdpTimingError, VdpWriteError,
};

type Data = Result<(), VdpPortError>;
type Advance = Result<(), VdpScheduleError>;
const REGION: Region = Region::NtscUs;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}
fn at(now: u64) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    device.advance_to(tick(now), REGION).unwrap();
    device
}
fn command(device: &mut Vdp, address: u32, code: u8) {
    device
        .write_control(
            (address as u16 & 0x3fff) | (u16::from(code & 3) << 14),
            REGION,
        )
        .unwrap();
    device
        .write_control(
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
    let write = device.queue_register(index, value, REGION).unwrap();
    device.advance_to(write.publish_at, REGION).unwrap();
    device
        .advance_to(tick(device.now().get() + 1), REGION)
        .unwrap();
}
fn timing(error: VdpTimingError) -> VdpPortError {
    VdpPortError::Schedule(VdpInterruptError::Timing(error).into())
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

pub fn byte_order_and_split_service_with(
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
) {
    for region in [REGION, Region::NtscJapan] {
        for (address, code, first, second) in [
            (0, 1, 0x34, 0x12),
            (1, 1, 0x12, 0x34),
            (0xfffe, 0x11, 0x34, 0x12),
            (0xffff, 0x11, 0x12, 0x34),
        ] {
            let mut device = at(1);
            command(&mut device, address, code);
            assert_eq!(data(&mut device, 0x1234, region), Ok(()));
            assert_eq!(device.fifo().len(), 1);
            assert_eq!(device.fifo().front().unwrap().command.address, address);
            let base = address as usize & !1;
            assert_eq!(advance(&mut device, tick(9), region), Ok(()));
            assert_eq!(&device.memory().vram()[base..base + 2], &[0, 0]);
            assert_eq!(advance(&mut device, tick(10), region), Ok(()));
            assert_eq!(&device.memory().vram()[base..base + 2], &[0, first]);
            assert_eq!(device.fifo().len(), 1);
            assert_eq!(advance(&mut device, tick(29), region), Ok(()));
            assert_eq!(&device.memory().vram()[base..base + 2], &[0, first]);
            assert_eq!(advance(&mut device, tick(30), region), Ok(()));
            assert_eq!(&device.memory().vram()[base..base + 2], &[second, first]);
            assert!(device.fifo().is_empty());
        }
    }
    for arrival in [9, 10, 11] {
        let mut device = at(arrival);
        command(&mut device, 0, 1);
        assert_eq!(data(&mut device, 0x1234, REGION), Ok(()));
        let first = if arrival == 9 { 10 } else { 30 };
        assert_eq!(advance(&mut device, tick(first), REGION), Ok(()));
        assert_eq!(&device.memory().vram()[..2], &[0, 0x34]);
        assert_eq!(advance(&mut device, tick(first + 20), REGION), Ok(()));
        assert_eq!(&device.memory().vram()[..2], &[0x12, 0x34]);
    }
}
pub fn byte_order_and_split_service() {
    byte_order_and_split_service_with(Vdp::write_data, Vdp::advance_to);
}

pub fn color_masks_and_address_aliases_with(
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
) {
    for code in [3, 0x13, 5, 0x15] {
        let mut device = at(1);
        for address in 0..=u32::from(u16::MAX) {
            if address & 0x7f == 0 {
                device = at(1);
            }
            command(&mut device, address, code);
            let index = ((address & 0x7e) >> 1) as usize;
            let word = (address as u16).wrapping_mul(73) ^ 0xffff;
            if code & 0xf == 5 && index >= 40 {
                let saved = device.memory().clone();
                assert_eq!(data(&mut device, word, REGION), Ok(()));
                assert_eq!(device.fifo().len(), 1);
                let slot = slot_after(device.now().get());
                assert_eq!(advance(&mut device, tick(slot + 1), REGION), Ok(()));
                assert!(device.fifo().is_empty());
                assert_eq!(device.memory(), &saved);
                continue;
            }
            let before = if code & 0xf == 3 {
                device.memory().cram()[index]
            } else {
                device.memory().vsram()[index]
            };
            assert_eq!(data(&mut device, word, REGION), Ok(()));
            assert_eq!(device.fifo().len(), 1);
            let current = if code & 0xf == 3 {
                device.memory().cram()[index]
            } else {
                device.memory().vsram()[index]
            };
            assert_eq!(current, before);
            let slot = slot_after(device.now().get());
            assert_eq!(advance(&mut device, tick(slot + 1), REGION), Ok(()));
            assert!(device.fifo().is_empty());
            let (actual, mask) = if code & 0xf == 3 {
                (device.memory().cram()[index], 0x0eee)
            } else {
                (device.memory().vsram()[index], 0x07ff)
            };
            assert_eq!(actual, word & mask);
        }
    }
}
pub fn color_masks_and_address_aliases() {
    color_masks_and_address_aliases_with(Vdp::write_data, Vdp::advance_to);
}

pub fn captured_target_increment_and_parser_with(
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
) {
    let mut device = at(1);
    register(&mut device, 15, 2);
    command(&mut device, 0x100, 1);
    assert_eq!(data(&mut device, 0x1234, REGION), Ok(()));
    assert_eq!(device.command().address, 0x102);
    let captured = device.fifo().front().unwrap();
    let pending = device.queue_register(15, 4, REGION).unwrap();
    command(&mut device, 2, 3);
    assert_eq!(data(&mut device, 0xffff, REGION), Ok(()));
    assert_eq!(device.command().address, 4);
    assert_eq!(device.fifo().front(), Some(captured));
    assert_eq!(advance(&mut device, pending.publish_at, REGION), Ok(()));
    assert_eq!(device.registers().auto_increment(), Some(4));
    device
        .advance_to(tick(pending.publish_at.get() + 1), REGION)
        .unwrap();
    assert_eq!(data(&mut device, 0x0246, REGION), Ok(()));
    assert_eq!(device.command().address, 8);
    command(&mut device, 6, 5);
    assert_eq!(advance(&mut device, tick(91), REGION), Ok(()));
    assert_eq!(&device.memory().vram()[0x100..0x102], &[0x12, 0x34]);
    assert_eq!(device.memory().cram()[1], 0x0eee);
    assert_eq!(device.memory().cram()[2], 0x0246);
    assert_eq!(device.memory().vsram()[3], 0);
    device.write_control(0x4009, REGION).unwrap();
    assert!(device.control().pending_word().is_some());
    assert_eq!(data(&mut device, 0x5678, REGION), Ok(()));
    assert_eq!(device.control().pending_word(), None);
    assert_eq!(
        device.command(),
        ControlCommand {
            address: 13,
            code: 0x15
        }
    );
    assert_eq!(advance(&mut device, tick(111), REGION), Ok(()));
    assert_eq!(device.memory().vsram()[4], 0x0678);
}
pub fn captured_target_increment_and_parser() {
    captured_target_increment_and_parser_with(Vdp::write_data, Vdp::advance_to);
}

pub fn full_ring_and_retirement_wait_with(
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
    mut wait: impl FnMut(&Vdp, Region) -> Result<Option<MasterTicks>, VdpPortError>,
) {
    for code in [1, 3, 5] {
        let mut device = at(1);
        command(&mut device, 0, code);
        assert_eq!(wait(&device, REGION), Ok(None));
        for word in 0x1000..0x1004 {
            assert_eq!(data(&mut device, word, REGION), Ok(()));
        }
        assert!(device.fifo().is_full());
        let deadline = if code == 1 { 30 } else { 10 };
        assert_eq!(wait(&device, REGION), Ok(Some(tick(deadline))));
        device.write_control(0x4001, REGION).unwrap();
        let saved = device.clone();
        assert_eq!(
            data(&mut device, 0xaaaa, REGION),
            Err(VdpPortError::Data(VdpWriteError::Full))
        );
        assert_eq!(device, saved);
        assert_eq!(advance(&mut device, tick(deadline - 1), REGION), Ok(()));
        assert!(device.fifo().is_full());
        assert_eq!(wait(&device, REGION), Ok(Some(tick(deadline))));
        assert_eq!(advance(&mut device, tick(deadline), REGION), Ok(()));
        assert_eq!(device.fifo().len(), 3);
        assert_eq!(device.fifo().front().unwrap().data, 0x1001);
        assert_eq!(wait(&device, REGION), Ok(None));
        device.advance_to(tick(deadline + 1), REGION).unwrap();
        assert_eq!(data(&mut device, 0xaaaa, REGION), Ok(()));
        assert_eq!(device.control().pending_word(), None);
        assert!(device.fifo().is_full());
        assert_eq!(advance(&mut device, tick(201), REGION), Ok(()));
        assert!(device.fifo().is_empty());
        assert_eq!(data(&mut device, 0xbbbb, REGION), Ok(()));
        assert_eq!(device.fifo().front().unwrap().data, 0xbbbb);
    }
}
pub fn full_ring_and_retirement_wait() {
    full_ring_and_retirement_wait_with(Vdp::write_data, Vdp::advance_to, Vdp::data_write_wait);
}

pub fn refresh_and_large_horizon_with(
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
) {
    // Literal software slot baseline; this does not establish physical DE-off timing.
    for refresh in [530, 1170, 1810, 2450, 3310] {
        for arrival in [refresh - 1, refresh, refresh + 1] {
            let mut device = at(arrival);
            command(&mut device, 0, 1);
            assert_eq!(data(&mut device, 0x1234, REGION), Ok(()));
            assert_eq!(
                advance(&mut device, tick(arrival.max(refresh)), REGION),
                Ok(())
            );
            assert_eq!(&device.memory().vram()[..2], &[0, 0]);
            assert_eq!(advance(&mut device, tick(refresh + 20), REGION), Ok(()));
            assert_eq!(&device.memory().vram()[..2], &[0, 0x34]);
            assert_eq!(advance(&mut device, tick(refresh + 40), REGION), Ok(()));
            assert_eq!(&device.memory().vram()[..2], &[0x12, 0x34]);
        }
    }
    let epoch = 1000;
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
    for (index, value) in [(0, 0), (1, 4), (10, 0), (12, 0), (15, 0)] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let pending = registers.queue_register(index, value).unwrap();
        registers.advance_to(pending.publish_at).unwrap();
    }
    registers.advance_to(tick(epoch)).unwrap();
    let interrupts = VdpInterrupts::at_counters_zero(tick(epoch), 0, false, false);
    let mut translated = Vdp::from_known_state(interrupts, registers, REGION).unwrap();
    translated.advance_to(tick(epoch + 509), REGION).unwrap();
    command(&mut translated, 0, 1);
    assert_eq!(data(&mut translated, 0x1234, REGION), Ok(()));
    assert_eq!(advance(&mut translated, tick(epoch + 510), REGION), Ok(()));
    assert_eq!(&translated.memory().vram()[..2], &[0, 0x34]);
    assert_eq!(advance(&mut translated, tick(epoch + 530), REGION), Ok(()));
    assert_eq!(&translated.memory().vram()[..2], &[0, 0x34]);
    assert_eq!(translated.fifo().len(), 1);
    assert_eq!(advance(&mut translated, tick(epoch + 550), REGION), Ok(()));
    assert_eq!(&translated.memory().vram()[..2], &[0x12, 0x34]);
    assert!(translated.fifo().is_empty());
    let mut device = at(1);
    register(&mut device, 15, 2);
    command(&mut device, 0, 1);
    for word in [0x1234, 0x5678, 0x9abc, 0xdef0] {
        assert_eq!(data(&mut device, word, REGION), Ok(()));
    }
    let mut chunked = device.clone();
    assert_eq!(advance(&mut device, tick(u64::MAX), REGION), Ok(()));
    for endpoint in [30, 50, 70, 90, 110, 130, 150, 170, 2660, 766090, u64::MAX] {
        assert_eq!(advance(&mut chunked, tick(endpoint), REGION), Ok(()));
    }
    assert_eq!(device, chunked);
    assert_eq!(
        &device.memory().vram()[..8],
        &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
    );
    assert_eq!(device.now(), tick(u64::MAX));
    assert!(device.fifo().is_empty());
}
pub fn refresh_and_large_horizon() {
    refresh_and_large_horizon_with(Vdp::write_data, Vdp::advance_to);
}

pub fn mode_address_and_clock_guards_with(
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
) {
    let mut device = at(1);
    let saved = device.clone();
    assert_eq!(
        data(&mut device, 0x1234, REGION),
        Err(VdpPortError::Data(VdpWriteError::UnsupportedCommand(
            saved.command()
        )))
    );
    assert_eq!(device, saved);
    command(&mut device, 0, 1);
    let saved = device.clone();
    let mut pal = device.clone();
    let mut ntsc = device.clone();
    assert_eq!(data(&mut pal, 0x1234, Region::Pal), Ok(()));
    assert_eq!(data(&mut ntsc, 0x1234, REGION), Ok(()));
    assert_eq!(pal, ntsc);
    assert_eq!(device, saved);
    for index in [1, 12] {
        let mut candidate = device.clone();
        let value = if index == 1 { 0x44 } else { 0x81 };
        candidate.queue_register(index, value, REGION).unwrap();
        let saved = candidate.clone();
        assert_eq!(
            data(&mut candidate, 0x1234, REGION),
            Err(timing(VdpTimingError::PendingModeChange))
        );
        assert_eq!(candidate, saved);
    }
    register(&mut device, 1, 0x44);
    assert_eq!(data(&mut device, 0x1234, REGION), Ok(()));
    assert_eq!(device.fifo().front().unwrap().data, 0x1234);
    let deadline = tick(device.now().get() + 3420);
    advance(&mut device, deadline, REGION).unwrap();
    assert!(device.fifo().is_empty());
    register(&mut device, 1, 0x84);
    let saved = device.clone();
    assert_eq!(
        data(&mut device, 0x1234, REGION),
        Err(timing(VdpTimingError::UnsupportedMode {
            index: 1,
            value: 0x84
        }))
    );
    assert_eq!(device, saved);
    for address in [0x3fff, 0x7fff, 0xbfff, 0xffff, 0x10000, 0x1ffff] {
        let mut candidate = at(1);
        register(&mut candidate, 15, 1);
        command(&mut candidate, address, 1);
        assert_eq!(data(&mut candidate, 0x1234, REGION), Ok(()));
        assert_eq!(candidate.command().address, (address + 1) & 0x1ffff);
        assert_eq!(candidate.fifo().front().unwrap().command.address, address);
        advance(&mut candidate, tick(101), REGION).unwrap();
        let base = (address as usize & 0xffff) & !1;
        let expected = if address & 1 == 0 {
            [0x12, 0x34]
        } else {
            [0x34, 0x12]
        };
        assert_eq!(&candidate.memory().vram()[base..base + 2], &expected);
        assert!(candidate.fifo().is_empty());
    }
    let mut candidate = at(10);
    command(&mut candidate, 0, 1);
    assert_eq!(data(&mut candidate, 0x1234, REGION), Ok(()));
    advance(&mut candidate, tick(50), REGION).unwrap();
    assert_eq!(&candidate.memory().vram()[..2], &[0x12, 0x34]);
    let mut candidate = at(u64::MAX);
    command(&mut candidate, 0, 1);
    let saved = candidate.clone();
    assert_eq!(
        data(&mut candidate, 0x1234, REGION),
        Err(timing(VdpTimingError::ClockOverflow))
    );
    assert_eq!(candidate, saved);
    let mut candidate = at(1);
    command(&mut candidate, 0, 1);
    assert_eq!(data(&mut candidate, 0x1234, REGION), Ok(()));
    let saved = candidate.clone();
    for (index, value) in [(1, 0x84), (1, 0x0c), (12, 0x81)] {
        assert_eq!(
            candidate.queue_register(index, value, REGION),
            Err(VdpRegisterError::UnsupportedModeChange { index, value }.into())
        );
        assert_eq!(candidate, saved);
    }
    let mut pal = candidate.clone();
    let mut ntsc = candidate.clone();
    assert_eq!(advance(&mut pal, tick(100), Region::Pal), Ok(()));
    assert_eq!(advance(&mut ntsc, tick(100), REGION), Ok(()));
    assert_eq!(pal, ntsc);
    assert_eq!(&pal.memory().vram()[..2], &[0x12, 0x34]);
    assert_eq!(candidate, saved);
    assert_eq!(
        advance(&mut candidate, tick(0), REGION),
        Err(VdpInterruptError::TimeReversal {
            current: tick(1),
            requested: tick(0)
        }
        .into())
    );
    assert_eq!(candidate, saved);
    for mode in [4, 0x84] {
        let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
        for (index, value) in [(0, 0), (1, mode), (10, 0), (12, 0)] {
            registers
                .advance_to(tick(registers.now().get() + 1))
                .unwrap();
            let pending = registers.queue_register(index, value).unwrap();
            registers.advance_to(pending.publish_at).unwrap();
        }
        let interrupts = VdpInterrupts::at_counters_zero(registers.now(), 0, false, false);
        let mut candidate = Vdp::from_known_state(interrupts, registers, REGION).unwrap();
        candidate
            .advance_to(tick(candidate.now().get() + 1), REGION)
            .unwrap();
        command(&mut candidate, 0, 1);
        let saved = candidate.clone();
        assert_eq!(
            data(&mut candidate, 0x1234, REGION),
            Err(if mode == 4 {
                VdpPortError::Data(VdpWriteError::UnknownIncrement)
            } else {
                timing(VdpTimingError::UnsupportedMode {
                    index: 1,
                    value: mode,
                })
            })
        );
        assert_eq!(candidate, saved);
    }
}
pub fn mode_address_and_clock_guards() {
    mode_address_and_clock_guards_with(Vdp::write_data, Vdp::advance_to);
}

pub fn publication_and_acknowledge_atomicity_with(
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
    mut acknowledge: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<u8, VdpScheduleError>,
) {
    let mut device = at(1);
    command(&mut device, 0, 1);
    assert_eq!(data(&mut device, 0x1234, REGION), Ok(()));
    let saved = device.clone();
    assert_eq!(
        acknowledge(&mut device, tick(30), REGION),
        Err(VdpScheduleError::CoincidentMemory(tick(30)))
    );
    assert_eq!(device, saved);
    assert_eq!(acknowledge(&mut device, tick(31), REGION), Ok(0));
    assert_eq!(&device.memory().vram()[..2], &[0x12, 0x34]);
    let mut device = at(766051);
    command(&mut device, 0, 1);
    device.write_control(0x4000, REGION).unwrap();
    assert_eq!(data(&mut device, 0x9abc, REGION), Ok(()));
    assert_eq!(device.control().pending_word(), None);
    assert_eq!(advance(&mut device, tick(766131), REGION), Ok(()));
    assert_eq!(&device.memory().vram()[..2], &[0x9a, 0xbc]);
    assert!(device.fifo().is_empty());
    assert!(device.interrupts().vertical_pending());
    let mut device = at(766011);
    command(&mut device, 0, 1);
    assert_eq!(data(&mut device, 0x1234, REGION), Ok(()));
    device.write_control(0x4002, REGION).unwrap();
    assert_eq!(data(&mut device, 0x9abc, REGION), Ok(()));
    assert_eq!(advance(&mut device, tick(766171), REGION), Ok(()));
    assert_eq!(&device.memory().vram()[..4], &[0x12, 0x34, 0x9a, 0xbc]);
    assert!(device.fifo().is_empty());
    assert!(device.interrupts().vertical_pending());
    let mut device = at(u64::MAX - 6);
    command(&mut device, 0, 1);
    device.write_control(0x4000, REGION).unwrap();
    let saved = device.clone();
    assert_eq!(
        data(&mut device, 0xdef0, REGION),
        Err(timing(VdpTimingError::ClockOverflow))
    );
    assert_eq!(device, saved);
    assert_eq!(device.control().pending_word(), Some(0x4000));
    assert!(device.fifo().is_empty());
    assert_eq!(advance(&mut device, tick(u64::MAX), REGION), Ok(()));
    assert_eq!(&device.memory().vram()[..2], &[0, 0]);
    let mut device = at(u64::MAX - 46);
    command(&mut device, 0, 1);
    assert_eq!(data(&mut device, 0xdef0, REGION), Ok(()));
    assert_eq!(advance(&mut device, tick(u64::MAX - 45), REGION), Ok(()));
    assert_eq!(&device.memory().vram()[..2], &[0, 0xf0]);
    assert_eq!(device.fifo().len(), 1);
    assert_eq!(advance(&mut device, tick(u64::MAX - 25), REGION), Ok(()));
    assert_eq!(&device.memory().vram()[..2], &[0xde, 0xf0]);
    assert!(device.fifo().is_empty());
    let mut device = at(2651);
    command(&mut device, 0, 1);
    assert_eq!(data(&mut device, 0x5678, REGION), Ok(()));
    assert_eq!(advance(&mut device, tick(2691), REGION), Ok(()));
    assert!(device.interrupts().horizontal_pending());
    assert_eq!(&device.memory().vram()[..2], &[0x56, 0x78]);
    let mut device = at(1);
    command(&mut device, 0, 1);
    assert_eq!(data(&mut device, 0x1234, REGION), Ok(()));
    let publish = device.queue_register(15, 2, REGION).unwrap();
    let saved = device.clone();
    assert_eq!(
        acknowledge(&mut device, publish.publish_at, REGION),
        Err(VdpScheduleError::CoincidentAcknowledge(publish.publish_at))
    );
    assert_eq!(device, saved);
    assert_eq!(advance(&mut device, tick(31), REGION), Ok(()));
    assert_eq!(device.registers().auto_increment(), Some(2));
    assert_eq!(&device.memory().vram()[..2], &[0x12, 0x34]);
}
pub fn publication_and_acknowledge_atomicity() {
    publication_and_acknowledge_atomicity_with(
        Vdp::write_data,
        Vdp::advance_to,
        Vdp::acknowledge_to,
    );
}
