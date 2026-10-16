use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, ControlWrite, Region, Vdp, VdpInterruptError, VdpInterrupts, VdpPortError,
    VdpReadError, VdpRegisterError, VdpRegisterPipeline, VdpScheduleError, VdpTimingError,
};

const REGION: Region = Region::NtscUs;
const VRAM_READ: u8 = 0;
const VRAM_WRITE: u8 = 1;
const CRAM_READ: u8 = 8;
const CRAM_WRITE: u8 = 3;
const VSRAM_READ: u8 = 4;
const VSRAM_WRITE: u8 = 5;
const INCREMENT: u8 = 15;

fn tick(at: u64) -> MasterTicks {
    MasterTicks::new(at)
}

fn at(now: u64) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    device.advance_to(tick(now), REGION).unwrap();
    device
}

fn first_word(address: u32, code: u8) -> u16 {
    (address as u16 & 0x3fff) | (u16::from(code & 3) << 14)
}

fn second_word(address: u32, code: u8) -> u16 {
    ((address >> 14) as u16 & 7) | (u16::from(code & 0x3c) << 2)
}

fn command(device: &mut Vdp, address: u32, code: u8) {
    command_with(device, address, code, &mut Vdp::write_control);
}

fn command_with(
    device: &mut Vdp,
    address: u32,
    code: u8,
    control: &mut impl FnMut(&mut Vdp, u16, Region) -> Result<ControlWrite, VdpPortError>,
) {
    control(device, first_word(address, code), REGION).unwrap();
    control(device, second_word(address, code), REGION).unwrap();
    assert_eq!(device.command(), ControlCommand { address, code });
}

fn register(device: &mut Vdp, index: u8, value: u8) {
    device
        .advance_to(tick(device.now().get() + 1), REGION)
        .unwrap();
    let pending = device.queue_register(index, value, REGION).unwrap();
    device.advance_to(pending.publish_at, REGION).unwrap();
    device
        .advance_to(tick(device.now().get() + 1), REGION)
        .unwrap();
}

fn read_error(error: VdpReadError) -> VdpPortError {
    VdpPortError::Read(error)
}

fn timing_error(error: VdpTimingError) -> VdpPortError {
    VdpPortError::Schedule(VdpInterruptError::Timing(error).into())
}

fn rejected_command(device: &mut Vdp, address: u32, code: u8, error: VdpPortError) {
    rejected_command_with(device, address, code, error, &mut Vdp::write_control);
}

fn rejected_command_with(
    device: &mut Vdp,
    address: u32,
    code: u8,
    error: VdpPortError,
    control: &mut impl FnMut(&mut Vdp, u16, Region) -> Result<ControlWrite, VdpPortError>,
) {
    control(device, first_word(address, code), REGION).unwrap();
    let saved = device.clone();
    assert_eq!(
        control(device, second_word(address, code), REGION),
        Err(error)
    );
    assert_eq!(*device, saved);
}

pub fn fifo_forwarding_and_segmented_vram_fetch() {
    fifo_forwarding_and_segmented_vram_fetch_with(
        Vdp::write_control,
        Vdp::advance_to,
        Vdp::read_data,
        Vdp::data_read_wait,
    );
}

pub fn fifo_forwarding_and_segmented_vram_fetch_with(
    mut control: impl FnMut(&mut Vdp, u16, Region) -> Result<ControlWrite, VdpPortError>,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<(), VdpScheduleError>,
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
    mut wait: impl FnMut(&Vdp, Region) -> Result<Option<MasterTicks>, VdpPortError>,
) {
    for region in [REGION, Region::NtscJapan] {
        let mut device = at(1);
        command_with(&mut device, 0, VRAM_WRITE, &mut control);
        device.write_data(0x1234, region).unwrap();
        command_with(&mut device, 0, VRAM_READ, &mut control);
        assert_eq!(wait(&device, region), Ok(Some(tick(70))));
        let mut segmented = device.clone();
        advance(&mut device, tick(71), region).unwrap();
        advance(&mut segmented, tick(10), region).unwrap();
        assert_eq!(&segmented.memory().vram()[..2], &[0, 0x34]);
        assert!(!segmented.read_cache().is_ready());
        advance(&mut segmented, tick(30), region).unwrap();
        assert_eq!(&segmented.memory().vram()[..2], &[0x12, 0x34]);
        assert!(segmented.fifo().is_empty());
        advance(&mut segmented, tick(50), region).unwrap();
        assert!(!segmented.read_cache().is_ready());
        assert_eq!(segmented.command().address, 0);
        assert_eq!(wait(&segmented, region), Ok(Some(tick(70))));
        advance(&mut segmented, tick(71), region).unwrap();
        assert_eq!(device, segmented);
        assert_eq!(device.read_cache().buffered_word(), Some(0x1234));
        assert_eq!(wait(&device, region), Ok(None));
        assert_eq!(read(&mut device, region), Ok(0x1234));
        assert!(!device.read_cache().is_ready());
        assert_eq!(device.read_cache().buffered_word(), Some(0x1234));
        assert_eq!(wait(&device, region), Ok(Some(tick(110))));
    }
}

pub fn automatic_next_fetch_advances_once_and_retains_old_word() {
    let mut device = at(1);
    register(&mut device, INCREMENT, 2);
    command(&mut device, 0, VRAM_WRITE);
    for word in [0x1234, 0x5678, 0x9abc, 0xdef0] {
        device.write_data(word, REGION).unwrap();
    }
    command(&mut device, 0, VRAM_READ);
    assert_eq!(device.data_read_wait(REGION), Ok(Some(tick(210))));
    device.advance_to(tick(211), REGION).unwrap();
    for (address, word, end) in [(2, 0x1234, 250), (4, 0x5678, 290), (6, 0x9abc, 330)] {
        assert_eq!(device.command().address, address);
        assert_eq!(device.read_data(REGION), Ok(word));
        assert_eq!(device.command().address, address);
        assert_eq!(device.read_cache().buffered_word(), Some(word));
        let saved = device.clone();
        assert_eq!(
            device.read_data(REGION),
            Err(read_error(VdpReadError::Empty))
        );
        assert_eq!(device, saved);
        assert_eq!(device.data_read_wait(REGION), Ok(Some(tick(end))));
        device.advance_to(tick(end - 20), REGION).unwrap();
        assert_eq!(device.command().address, address);
        assert!(!device.read_cache().is_ready());
        device.advance_to(tick(end + 1), REGION).unwrap();
        assert_eq!(device.command().address, address + 2);
    }
    assert_eq!(device.read_cache().buffered_word(), Some(0xdef0));
}

pub fn color_read_uses_raw_retained_fifo_tail_and_one_slot() {
    for (write, read, mask) in [
        (CRAM_WRITE, CRAM_READ, 0x0eee),
        (VSRAM_WRITE, VSRAM_READ, 0x07ff),
    ] {
        let mut device = at(1);
        register(&mut device, INCREMENT, 2);
        command(&mut device, 0, write);
        for word in [0xa135, 0x2468, 0x3579, 0xffff] {
            device.write_data(word, REGION).unwrap();
        }
        command(&mut device, 0x81, read);
        assert_eq!(device.data_read_wait(REGION), Ok(Some(tick(110))));
        device.advance_to(tick(111), REGION).unwrap();
        assert!(device.fifo().is_empty());
        assert_eq!(device.fifo().next_slot_data(), Some(0xa135));
        assert_eq!(device.read_cache().buffered_word(), Some(0xa135));
        assert_eq!(device.command().address, 0x83);
        assert_eq!(device.read_data(REGION), Ok(0xa135));
        assert_eq!(device.data_read_wait(REGION), Ok(Some(tick(130))));
        device.advance_to(tick(131), REGION).unwrap();
        assert_eq!(
            device.read_cache().buffered_word(),
            Some((0x2468 & mask) | (0xa135 & !mask))
        );
        command(&mut device, 0, VRAM_WRITE);
        device.write_data(0xfedc, REGION).unwrap();
        command(&mut device, 4, read);
        device.advance_to(tick(191), REGION).unwrap();
        assert_eq!(device.fifo().next_slot_data(), Some(0x2468));
        assert_eq!(
            device.read_cache().buffered_word(),
            Some((0x3579 & mask) | (0x2468 & !mask))
        );
    }
}

pub fn unknown_history_rejects_entire_command_and_keeps_pending_writes() {
    for read in [CRAM_READ, VSRAM_READ] {
        for queued in 0..4 {
            let mut device = at(1);
            command(&mut device, 0, VRAM_WRITE);
            for word in 0..queued {
                device.write_data(0x1234 + word, REGION).unwrap();
            }
            rejected_command(
                &mut device,
                0,
                read,
                VdpPortError::Schedule(VdpScheduleError::Read(VdpReadError::UnknownFifoHistory)),
            );
            assert_eq!(device.fifo().len(), usize::from(queued));
            assert_eq!(&device.memory().vram()[..2], &[0, 0]);
            assert!(!device.read_cache().is_ready());
        }
    }
}

pub fn control_invalidation_and_interrupted_second_phase_are_atomic() {
    let mut device = at(1);
    command(&mut device, 0, VRAM_READ);
    device.advance_to(tick(10), REGION).unwrap();
    for word in [0x4000, 0x8f02, 0x8000] {
        let saved = device.clone();
        assert_eq!(
            device.write_control(word, REGION),
            Err(read_error(VdpReadError::PrefetchPending))
        );
        assert_eq!(device, saved);
    }
    for (index, value) in [(1, 0x44), (12, 0x81), (INCREMENT, 2)] {
        let saved = device.clone();
        assert_eq!(
            device.queue_register(index, value, REGION),
            Err(VdpRegisterError::UnsupportedModeChange { index, value }.into())
        );
        assert_eq!(device, saved);
    }
    device.advance_to(tick(31), REGION).unwrap();
    assert!(device.read_cache().is_ready());
    device.write_control(0x4000, REGION).unwrap();
    assert!(!device.read_cache().is_ready());
    assert_eq!(device.read_cache().buffered_word(), Some(0));
    assert_eq!(device.control().pending_word(), Some(0x4000));
    device.write_control(0, REGION).unwrap();
    device.write_data(0x9876, REGION).unwrap();
    assert_eq!(device.control().pending_word(), None);
    command(&mut device, 0, VRAM_READ);
    device.advance_to(tick(111), REGION).unwrap();
    assert_eq!(device.read_cache().buffered_word(), Some(0x9876));
}

pub fn phase_zero_command_transition_cancels_pending_fetch() {
    let mut device = at(1);
    command(&mut device, 0, VRAM_READ);
    command(&mut device, 2, VRAM_WRITE);
    device.write_data(0x4567, REGION).unwrap();
    device.advance_to(tick(71), REGION).unwrap();
    assert_eq!(&device.memory().vram()[2..4], &[0x45, 0x67]);
    assert!(!device.read_cache().is_ready());
    assert_eq!(device.read_cache().buffered_word(), None);
    assert_eq!(
        device.data_read_wait(REGION),
        Err(read_error(VdpReadError::UnsupportedCommand(
            device.command()
        )))
    );
}

pub fn refresh_and_strict_next_slot_boundaries() {
    // Literal software nonactive-slot baseline, without a physical timing claim.
    for (arrival, end) in [
        (9, 30),
        (10, 50),
        (11, 50),
        (509, 550),
        (510, 570),
        (529, 570),
        (530, 570),
        (531, 570),
    ] {
        let mut device = at(arrival);
        command(&mut device, 0, VRAM_READ);
        assert_eq!(device.data_read_wait(REGION), Ok(Some(tick(end))));
        device.advance_to(tick(end - 1), REGION).unwrap();
        assert!(!device.read_cache().is_ready());
        device.advance_to(tick(end), REGION).unwrap();
        assert!(device.read_cache().is_ready());
        assert_eq!(device.read_data(REGION), Ok(0));
        assert!(!device.read_cache().is_ready());
        assert_eq!(device.data_read_wait(REGION), Ok(Some(tick(end + 40))));
    }
    for refresh in [530, 1170, 1810, 2450, 3310] {
        let mut device = at(refresh - 21);
        command(&mut device, 0, VRAM_READ);
        assert_eq!(device.data_read_wait(REGION), Ok(Some(tick(refresh + 20))));
        device.advance_to(tick(refresh), REGION).unwrap();
        assert!(!device.read_cache().is_ready());
        device.advance_to(tick(refresh + 20), REGION).unwrap();
        assert!(device.read_cache().is_ready());
    }
}

pub fn unsupported_address_increment_and_carry_reject_before_fetch() {
    for (address, code) in [(1, VRAM_READ), (0x50, VSRAM_READ)] {
        let mut device = at(1);
        rejected_command(
            &mut device,
            address,
            code,
            read_error(VdpReadError::UnsupportedCommand(ControlCommand {
                address,
                code,
            })),
        );
    }
    let mut device = at(1);
    register(&mut device, INCREMENT, 1);
    rejected_command(
        &mut device,
        0,
        VRAM_READ,
        read_error(VdpReadError::UnsupportedIncrement(1)),
    );
    for address in [0x3ffe, 0x7ffe, 0xbffe, 0xfffe] {
        let mut device = at(1);
        register(&mut device, INCREMENT, 2);
        command(&mut device, address, VRAM_READ);
        device.advance_to(tick(51), REGION).unwrap();
        assert_eq!(device.command().address, address + 2);
        assert_eq!(device.read_data(REGION), Ok(0));
    }
    let mut device = at(1);
    register(&mut device, INCREMENT, 2);
    command(&mut device, 0x3ffc, VRAM_READ);
    device.advance_to(tick(51), REGION).unwrap();
    assert_eq!(device.command().address, 0x3ffe);
    assert_eq!(device.read_data(REGION), Ok(0));
    device.advance_to(tick(91), REGION).unwrap();
    assert_eq!(device.command().address, 0x4000);
    assert!(device.read_cache().is_ready());
    let mut device = at(1);
    register(&mut device, INCREMENT, 2);
    command(&mut device, 0x4e, VSRAM_WRITE);
    device.write_data(0xa357, REGION).unwrap();
    command(&mut device, 0, VRAM_WRITE);
    for word in [0x1234, 0x5678, 0x9abc] {
        device.write_data(word, REGION).unwrap();
    }
    command(&mut device, 0x4e, VSRAM_READ);
    assert_eq!(device.data_read_wait(REGION), Ok(Some(tick(170))));
    device.advance_to(tick(171), REGION).unwrap();
    assert_eq!(device.command().address, 0x50);
    assert_eq!(device.memory().vsram()[39], 0x0357);
    assert_eq!(device.read_cache().buffered_word(), Some(0xa357));
    assert!(device.read_cache().is_ready());
    let saved = device.clone();
    assert_eq!(
        device.read_data(REGION),
        Err(read_error(VdpReadError::UnsupportedCommand(
            ControlCommand {
                address: 0x50,
                code: VSRAM_READ,
            }
        )))
    );
    assert_eq!(device, saved);
}

pub fn mode_guards_preserve_ready_cache_and_pending_control() {
    for (index, value) in [(1, 0x44), (12, 0x81)] {
        let mut device = at(1);
        device.queue_register(index, value, REGION).unwrap();
        rejected_command(
            &mut device,
            0,
            VRAM_READ,
            timing_error(VdpTimingError::PendingModeChange),
        );
    }
    let mut device = at(1);
    device.queue_register(INCREMENT, 2, REGION).unwrap();
    rejected_command(
        &mut device,
        0,
        VRAM_READ,
        read_error(VdpReadError::PendingIncrement),
    );
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
    for (index, value) in [(0, 0), (1, 4), (10, 0), (12, 0)] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let pending = registers.queue_register(index, value).unwrap();
        registers.advance_to(pending.publish_at).unwrap();
    }
    let interrupts = VdpInterrupts::at_counters_zero(registers.now(), 0, false, false);
    let mut device = Vdp::from_known_state(interrupts, registers, REGION).unwrap();
    device
        .advance_to(tick(device.now().get() + 1), REGION)
        .unwrap();
    rejected_command(
        &mut device,
        0,
        VRAM_READ,
        read_error(VdpReadError::UnknownIncrement),
    );
    let mut active = at(1);
    register(&mut active, 1, 0x44);
    command(&mut active, 0x10000, VRAM_READ);
    let deadline = active.data_read_wait(REGION).unwrap().unwrap();
    active.advance_to(deadline, REGION).unwrap();
    assert_eq!(active.command().address, 0x10000);
    assert_eq!(active.read_data(REGION), Ok(0));
    assert!(!active.read_cache().is_ready());
    {
        let mode = 0x84;
        let mut device = at(1);
        register(&mut device, 1, mode);
        rejected_command(
            &mut device,
            0,
            VRAM_READ,
            timing_error(VdpTimingError::UnsupportedMode {
                index: 1,
                value: mode,
            }),
        );
    }
    let mut device = at(1);
    command(&mut device, 0, VRAM_READ);
    device.advance_to(tick(31), REGION).unwrap();
    let saved = device.clone();
    let mut pal = device.clone();
    let mut ntsc = device.clone();
    assert_eq!(pal.read_data(Region::Pal), Ok(0));
    assert_eq!(ntsc.read_data(REGION), Ok(0));
    assert_eq!(pal, ntsc);
    assert_eq!(device, saved);
    assert_eq!(
        device.data_read_wait(Region::Pal),
        device.data_read_wait(REGION)
    );
    assert!(device.data_read_wait(Region::Pal).is_ok());
    assert_eq!(device, saved);
    device.queue_register(INCREMENT, 255, REGION).unwrap();
    let saved = device.clone();
    assert_eq!(
        device.read_data(REGION),
        Err(read_error(VdpReadError::PendingIncrement))
    );
    assert_eq!(device, saved);
    assert!(device.read_cache().is_ready());
    assert_eq!(device.read_cache().buffered_word(), Some(0));
    assert_eq!(device.command().address, 0);
}

pub fn command_and_consumption_preflight_future_irq_and_clock_tail() {
    command_and_consumption_preflight_future_irq_and_clock_tail_with(
        Vdp::write_control,
        Vdp::advance_to,
        Vdp::read_data,
        Vdp::data_read_wait,
    );
}

pub fn command_and_consumption_preflight_future_irq_and_clock_tail_with(
    mut control: impl FnMut(&mut Vdp, u16, Region) -> Result<ControlWrite, VdpPortError>,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<(), VdpScheduleError>,
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
    mut wait: impl FnMut(&Vdp, Region) -> Result<Option<MasterTicks>, VdpPortError>,
) {
    let mut device = at(766051);
    command_with(&mut device, 0, VRAM_READ, &mut control);
    advance(&mut device, tick(766091), REGION).unwrap();
    assert!(device.interrupts().vertical_pending());
    assert_eq!(read(&mut device, REGION), Ok(0));
    assert!(!device.read_cache().is_ready());
    assert_eq!(wait(&device, REGION), Ok(Some(tick(766130))));
    for arrival in [u64::MAX - 6, u64::MAX] {
        let mut device = at(arrival);
        rejected_command_with(
            &mut device,
            0,
            VRAM_READ,
            timing_error(VdpTimingError::ClockOverflow),
            &mut control,
        );
    }
    let mut device = at(u64::MAX - 46);
    command_with(&mut device, 0, VRAM_READ, &mut control);
    advance(&mut device, tick(u64::MAX - 24), REGION).unwrap();
    let saved = device.clone();
    assert_eq!(
        read(&mut device, REGION),
        Err(timing_error(VdpTimingError::ClockOverflow))
    );
    assert_eq!(device, saved);
}

pub fn acknowledge_fetch_collision_leaves_cache_clock_and_memory_unchanged() {
    let mut device = at(1);
    command(&mut device, 0, VRAM_WRITE);
    device.write_data(0xbeef, REGION).unwrap();
    command(&mut device, 0, VRAM_READ);
    let saved = device.clone();
    assert_eq!(
        device.acknowledge_to(tick(70), REGION),
        Err(VdpScheduleError::CoincidentMemory(tick(70)))
    );
    assert_eq!(device, saved);
    assert_eq!(device.acknowledge_to(tick(71), REGION), Ok(0));
    assert_eq!(device.read_cache().buffered_word(), Some(0xbeef));
    assert_eq!(&device.memory().vram()[..2], &[0xbe, 0xef]);
}
