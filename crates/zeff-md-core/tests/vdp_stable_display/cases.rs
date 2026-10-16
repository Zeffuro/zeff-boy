use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, ControlWrite, Region, Vdp, VdpInterruptError, VdpInterrupts, VdpPortError,
    VdpRegisterPipeline, VdpScheduleError, VdpTimingError, VdpWriteError,
};

type Control = Result<ControlWrite, VdpPortError>;
type Data = Result<(), VdpPortError>;
type Advance = Result<(), VdpScheduleError>;
type Wait = Result<Option<MasterTicks>, VdpPortError>;
const REGION: Region = Region::NtscUs;

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn inputs(now: u64, epoch: u64) -> (VdpInterrupts, VdpRegisterPipeline) {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(epoch));
    for (index, value) in [(0, 0), (1, 0x44), (12, 0), (10, 0xff), (15, 2), (5, 0x10)] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    let mut irq = VdpInterrupts::at_counters_zero(tick(epoch), 0xff, false, false);
    assert_eq!(
        irq.advance_to(registers.now(), REGION, &registers),
        Ok(None)
    );
    while irq
        .advance_registers_to(tick(now), REGION, &mut registers)
        .unwrap()
        .is_some()
    {}
    (irq, registers)
}

fn at(now: u64) -> Vdp {
    let (irq, registers) = inputs(now, 0);
    Vdp::from_stable_display_state(irq, registers, REGION).unwrap()
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

fn schedule(error: VdpTimingError) -> VdpScheduleError {
    VdpInterruptError::Timing(error).into()
}

fn port(error: VdpTimingError) -> VdpPortError {
    VdpPortError::Schedule(schedule(error))
}

pub fn active_fifo_and_captured_targets() {
    active_fifo_and_captured_targets_with(
        Vdp::write_control,
        Vdp::write_data,
        Vdp::advance_to,
        Vdp::data_write_wait,
    );
}

pub fn active_fifo_and_captured_targets_with(
    mut control: impl FnMut(&mut Vdp, u16, Region) -> Control,
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
    mut wait: impl FnMut(&Vdp, Region) -> Wait,
) {
    let mut device = at(121);
    command(&mut control, &mut device, 0x2000, 1);
    for word in [0x1234, 0x5678, 0x9abc, 0xdef0] {
        data(&mut device, word, REGION).unwrap();
    }
    assert_eq!(device.command().address, 0x2008);
    assert!(device.fifo().is_full());
    let full = device.clone();
    assert_eq!(
        data(&mut device, 0xffff, REGION),
        Err(VdpPortError::Data(VdpWriteError::Full))
    );
    assert_eq!(device, full);
    assert_eq!(wait(&device, REGION), Ok(Some(tick(370))));
    let mut split = device.clone();
    advance(&mut device, tick(1651), REGION).unwrap();
    for (end, bytes) in [(210, [0, 0x34]), (370, [0x12, 0x34])] {
        advance(&mut split, tick(end), REGION).unwrap();
        assert_eq!(&split.memory().vram()[0x2000..0x2002], &bytes);
    }
    for end in [690, 850, 1010, 1330, 1490, 1651] {
        advance(&mut split, tick(end), REGION).unwrap();
    }
    assert_eq!(device, split);
    assert!(device.fifo().is_empty());
    assert_eq!(
        &device.memory().vram()[0x2000..0x2008],
        &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
    );
    assert_eq!(
        device.sprite_cache().unwrap().entry_words(0).unwrap(),
        [Some(0x0234), Some(0x0678)]
    );
    for (code, word, end) in [(3, 0xffff, 1971), (5, 0x1234, 2131)] {
        command(&mut control, &mut device, 0, code);
        data(&mut device, word, REGION).unwrap();
        command(&mut control, &mut device, 6, 1);
        advance(&mut device, tick(end), REGION).unwrap();
        assert_eq!(device.command().address, 6);
        assert!(device.fifo().is_empty());
    }
    assert_eq!(device.memory().cram()[0], 0x0eee);
    assert_eq!(device.memory().vsram()[0], 0x0234);
}

pub fn active_reads_and_automatic_fetch() {
    active_reads_and_automatic_fetch_with(
        Vdp::write_control,
        Vdp::write_data,
        Vdp::advance_to,
        Vdp::read_data,
        Vdp::data_read_wait,
    );
}

pub fn active_reads_and_automatic_fetch_with(
    mut control: impl FnMut(&mut Vdp, u16, Region) -> Control,
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
    mut wait: impl FnMut(&Vdp, Region) -> Wait,
) {
    let mut device = at(121);
    command(&mut control, &mut device, 0, 1);
    for word in [0x1234, 0x5678, 0x9abc, 0xdef0] {
        data(&mut device, word, REGION).unwrap();
    }
    command(&mut control, &mut device, 0, 0);
    assert_eq!(wait(&device, REGION), Ok(Some(tick(2130))));
    let mut split = device.clone();
    advance(&mut device, tick(2131), REGION).unwrap();
    for end in [210, 370, 690, 850, 1010, 1330, 1490, 1650, 1970] {
        advance(&mut split, tick(end), REGION).unwrap();
        assert!(!split.read_cache().is_ready());
        assert_eq!(split.command().address, 0);
    }
    advance(&mut split, tick(2131), REGION).unwrap();
    assert_eq!(device, split);
    assert_eq!(wait(&device, REGION), Ok(None));
    assert_eq!(device.command().address, 2);
    assert_eq!(read(&mut device, REGION), Ok(0x1234));
    assert_eq!(device.read_cache().buffered_word(), Some(0x1234));
    assert_eq!(wait(&device, REGION), Ok(Some(tick(2590))));
    advance(&mut device, tick(2290), REGION).unwrap();
    assert_eq!(device.command().address, 2);
    advance(&mut device, tick(2591), REGION).unwrap();
    assert_eq!(read(&mut device, REGION), Ok(0x5678));
    assert_eq!(device.command().address, 4);
    for (write_code, read_code, word, expected, end) in
        [(3, 8, 0xffff, 0x5efe, 2890), (5, 4, 0xa135, 0x9935, 3470)]
    {
        command(&mut control, &mut device, 0, write_code);
        data(&mut device, word, REGION).unwrap();
        command(&mut control, &mut device, 0, read_code);
        assert_eq!(wait(&device, REGION), Ok(Some(tick(end))));
        advance(&mut device, tick(end + 1), REGION).unwrap();
        assert_eq!(read(&mut device, REGION), Ok(expected));
        assert_eq!(device.command().address, 2);
    }
}

pub fn nonactive_fifo_and_reads() {
    nonactive_fifo_and_reads_with(
        Vdp::write_control,
        Vdp::write_data,
        Vdp::advance_to,
        Vdp::read_data,
        Vdp::data_read_wait,
    );
}

pub fn nonactive_fifo_and_reads_with(
    mut control: impl FnMut(&mut Vdp, u16, Region) -> Control,
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
    mut wait: impl FnMut(&Vdp, Region) -> Wait,
) {
    let mut device = at(769501);
    command(&mut control, &mut device, 0, 1);
    data(&mut device, 0xabcd, REGION).unwrap();
    command(&mut control, &mut device, 0, 0);
    assert_eq!(wait(&device, REGION), Ok(Some(tick(769570))));
    let mut split = device.clone();
    advance(&mut device, tick(769571), REGION).unwrap();
    advance(&mut split, tick(769510), REGION).unwrap();
    assert_eq!(&split.memory().vram()[..2], &[0, 0xcd]);
    advance(&mut split, tick(769530), REGION).unwrap();
    assert_eq!(&split.memory().vram()[..2], &[0xab, 0xcd]);
    advance(&mut split, tick(769550), REGION).unwrap();
    assert!(!split.read_cache().is_ready());
    advance(&mut split, tick(769571), REGION).unwrap();
    assert_eq!(device, split);
    assert_eq!(read(&mut device, REGION), Ok(0xabcd));
    assert_eq!(wait(&device, REGION), Ok(Some(tick(769610))));
    let mut refresh = at(770021);
    command(&mut control, &mut refresh, 0, 1);
    data(&mut refresh, 0x1234, REGION).unwrap();
    advance(&mut refresh, tick(770050), REGION).unwrap();
    assert_eq!(&refresh.memory().vram()[..2], &[0, 0x34]);
    advance(&mut refresh, tick(770071), REGION).unwrap();
    assert_eq!(&refresh.memory().vram()[..2], &[0x12, 0x34]);
}

pub fn margins_and_atomic_preflight() {
    margins_and_atomic_preflight_with(Vdp::write_control, Vdp::write_data, Vdp::advance_to);
}

pub fn margins_and_atomic_preflight_with(
    mut control: impl FnMut(&mut Vdp, u16, Region) -> Control,
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
) {
    let mut fitting = at(758709);
    command(&mut control, &mut fitting, 0x2000, 1);
    data(&mut fitting, 0x1234, REGION).unwrap();
    advance(&mut fitting, tick(758710), REGION).unwrap();
    assert_eq!(&fitting.memory().vram()[0x2000..0x2002], &[0, 0x34]);
    advance(&mut fitting, tick(758990), REGION).unwrap();
    assert_eq!(&fitting.memory().vram()[0x2000..0x2002], &[0x12, 0x34]);
    advance(&mut fitting, tick(769501), REGION).unwrap();
    for now in [758711, 885751] {
        let mut device = at(now);
        command(&mut control, &mut device, 0x2000, 1);
        control(&mut device, 0x4567, REGION).unwrap();
        data(&mut device, 0xabcd, REGION).unwrap();
        assert_eq!(device.control().pending_word(), None);
        advance(&mut device, tick(now + 1000), REGION).unwrap();
        assert_eq!(&device.memory().vram()[0x566..0x568], &[0xcd, 0xab]);
        assert!(device.fifo().is_empty());
        let mut device = at(now);
        control(&mut device, 0, REGION).unwrap();
        control(&mut device, 0, REGION).unwrap();
        advance(&mut device, tick(now + 1000), REGION).unwrap();
        assert!(device.read_cache().is_ready());
        assert_eq!(device.command().address, 2);
    }
    let mut last = at(885751);
    command(&mut control, &mut last, 0, 3);
    data(&mut last, 0xffff, REGION).unwrap();
    advance(&mut last, tick(885770), REGION).unwrap();
    assert_eq!(last.memory().cram()[0], 0x0eee);
    advance(&mut last, tick(896041), REGION).unwrap();
    assert_eq!(last.now(), tick(896041));
    let mut short = at(758711);
    advance(&mut short, tick(759239), REGION).unwrap();
    command(&mut control, &mut short, 0, 1);
    data(&mut short, 0, REGION).unwrap();
    advance(&mut short, tick(769501), REGION).unwrap();
}

pub fn constructor_and_legacy_guards() {
    constructor_and_legacy_guards_with(Vdp::write_control, Vdp::write_data);
}

pub fn constructor_and_legacy_guards_with(
    mut control: impl FnMut(&mut Vdp, u16, Region) -> Control,
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
) {
    for epoch in [0, 47] {
        let (irq, registers) = inputs(epoch + 121, epoch);
        for region in [REGION, Region::NtscJapan, Region::Pal] {
            let device = Vdp::from_stable_display_state(irq, registers, region).unwrap();
            assert_eq!(device.now(), tick(epoch + 121));
            assert_eq!(device.interrupts(), &irq);
            assert_eq!(device.registers(), &registers);
        }
    }
    let (irq, registers) = inputs(121, 0);
    let mut legacy = Vdp::from_known_state(irq, registers, REGION).unwrap();
    command(&mut control, &mut legacy, 0, 1);
    data(&mut legacy, 0x1234, REGION).unwrap();
    legacy.advance_to(tick(1000), REGION).unwrap();
    assert_eq!(&legacy.memory().vram()[..2], &[0x12, 0x34]);
    assert_eq!(
        Vdp::from_stable_display_state(irq, registers, Region::Pal),
        Vdp::from_stable_display_state(irq, registers, REGION)
    );
    for index in [1, 12] {
        let mut pending = registers;
        pending
            .queue_register(index, if index == 1 { 0x04 } else { 0x10 })
            .unwrap();
        assert_eq!(
            Vdp::from_stable_display_state(irq, pending, REGION),
            Err(schedule(VdpTimingError::PendingModeChange))
        );
    }
    let mut late = registers;
    late.advance_to(tick(122)).unwrap();
    assert_eq!(
        Vdp::from_stable_display_state(irq, late, REGION),
        Err(VdpScheduleError::ClockMismatch {
            interrupt: tick(121),
            registers: tick(122)
        })
    );
    let wrong_phase = VdpInterrupts::at_counters_zero(tick(121), 0xff, false, false);
    assert_eq!(
        Vdp::from_stable_display_state(wrong_phase, registers, REGION),
        Err(VdpScheduleError::PhaseMismatch)
    );
    for now in [759240, 769499, 885780, 896039] {
        let (irq, registers) = inputs(now, 0);
        assert_eq!(
            Vdp::from_stable_display_state(irq, registers, REGION),
            Err(schedule(VdpTimingError::UnqualifiedMemoryInterval(tick(
                now
            ))))
        );
    }
    for (index, value) in [(1, 0xc4), (1, 0x4c), (12, 2)] {
        let (mut irq, mut registers) = inputs(121, 0);
        let baseline = registers;
        let write = registers.queue_register(index, value).unwrap();
        assert_eq!(
            irq.advance_to(write.publish_at, REGION, &baseline),
            Ok(None)
        );
        registers.advance_to(write.publish_at).unwrap();
        assert!(Vdp::from_stable_display_state(irq, registers, REGION).is_err());
    }
}

pub fn publication_history_and_interrupts() {
    publication_history_and_interrupts_with(
        Vdp::write_control,
        Vdp::write_data,
        Vdp::advance_to,
        Vdp::acknowledge_to,
    );
}

pub fn publication_history_and_interrupts_with(
    mut control: impl FnMut(&mut Vdp, u16, Region) -> Control,
    mut data: impl FnMut(&mut Vdp, u16, Region) -> Data,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Advance,
    mut acknowledge: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<u8, VdpScheduleError>,
) {
    for (index, value) in [(1, 0x64), (1, 0x44), (12, 8)] {
        let mut device = at(121);
        command(&mut control, &mut device, 0, 1);
        device.queue_register(index, value, REGION).unwrap();
        data(&mut device, 0x1234, REGION).unwrap();
        advance(&mut device, tick(141), REGION).unwrap();
        data(&mut device, 0x1234, REGION).unwrap();
        advance(&mut device, tick(371), REGION).unwrap();
        assert_eq!(&device.memory().vram()[..2], &[0x12, 0x34]);
    }
    let mut device = at(121);
    command(&mut control, &mut device, 0, 1);
    for value in [0x04, 0x44] {
        let write = device.queue_register(1, value, REGION).unwrap();
        let saved = device.clone();
        assert_eq!(
            data(&mut device, 0, REGION),
            Err(port(VdpTimingError::PendingModeChange))
        );
        assert_eq!(device, saved);
        advance(&mut device, tick(write.publish_at.get() + 1), REGION).unwrap();
        data(&mut device, 0, REGION).unwrap();
        let end = tick(device.now().get() + 1000);
        advance(&mut device, end, REGION).unwrap();
        assert!(device.fifo().is_empty());
    }
    let mut device = at(121);
    command(&mut control, &mut device, 0, 1);
    for value in [0xc4, 0x44] {
        let write = device.queue_register(1, value, REGION).unwrap();
        advance(&mut device, tick(write.publish_at.get() + 1), REGION).unwrap();
        if value == 0xc4 {
            let saved = device.clone();
            assert_eq!(
                data(&mut device, 0, REGION),
                Err(port(VdpTimingError::UnsupportedMode { index: 1, value }))
            );
            assert_eq!(device, saved);
        } else {
            data(&mut device, 0, REGION).unwrap();
        }
    }
    let mut irq_device = at(769501);
    let write = irq_device.queue_register(1, 0x64, REGION).unwrap();
    advance(&mut irq_device, tick(write.publish_at.get() + 1), REGION).unwrap();
    assert!(irq_device.interrupts().vertical_pending());
    let end = tick(irq_device.now().get() + 1);
    assert_eq!(acknowledge(&mut irq_device, end, REGION), Ok(6));
    command(&mut control, &mut irq_device, 0, 1);
    data(&mut irq_device, 0x5678, REGION).unwrap();
}
