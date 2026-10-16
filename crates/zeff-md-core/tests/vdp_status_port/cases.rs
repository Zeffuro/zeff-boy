use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, ControlWrite, Region, Vdp, VdpInterruptError, VdpInterrupts, VdpPortError,
    VdpScheduleError, VdpStatusError, VdpTimingError,
};

const REGION: Region = Region::NtscUs;
const EMPTY: u16 = 0x0208;

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn at(now: u64) -> Vdp {
    let mut device = Vdp::bootstrap_mode5();
    device.advance_to(tick(now), REGION).unwrap();
    device
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
}

fn timing_error(error: VdpTimingError) -> VdpPortError {
    VdpPortError::Schedule(VdpInterruptError::Timing(error).into())
}

fn unchanged(
    device: &mut Vdp,
    region: Region,
    expected: Result<u16, VdpPortError>,
    read: &mut impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
) {
    let saved = device.clone();
    for _ in 0..2 {
        assert_eq!(read(device, region), expected);
        assert_eq!(*device, saved);
    }
}

macro_rules! read_case {
    ($name:ident, $with:ident) => {
        pub fn $name() {
            $with(Vdp::read_status);
        }
    };
}

read_case!(
    bootstrap_driven_bits_and_ntsc_regions,
    bootstrap_driven_bits_and_ntsc_regions_with
);
pub fn bootstrap_driven_bits_and_ntsc_regions_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
) {
    for region in [REGION, Region::NtscJapan] {
        let mut device = Vdp::bootstrap_mode5();
        unchanged(&mut device, region, Ok(EMPTY | 4), &mut read);
        device.advance_to(tick(101), region).unwrap();
        unchanged(&mut device, region, Ok(EMPTY), &mut read);
    }
}

pub fn hblank_edges_and_slot_boundary() {
    hblank_edges_and_slot_boundary_with(Vdp::read_status, Vdp::advance_to);
}
pub fn hblank_edges_and_slot_boundary_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<(), VdpScheduleError>,
) {
    let mut device = Vdp::bootstrap_mode5();
    for (now, blank) in [
        (10, true),
        (99, true),
        (100, false),
        (101, false),
        (2939, false),
        (2940, true),
        (2941, true),
        (3419, true),
        (3420, true),
        (3519, true),
        (3520, false),
    ] {
        advance(&mut device, tick(now), REGION).unwrap();
        unchanged(
            &mut device,
            REGION,
            Ok(EMPTY | if blank { 4 } else { 0 }),
            &mut read,
        );
    }
}

pub fn status_cancels_only_parser_half_and_restarts_command() {
    status_cancels_only_parser_half_and_restarts_command_with(Vdp::read_status, Vdp::write_control);
}
pub fn status_cancels_only_parser_half_and_restarts_command_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
    mut control: impl FnMut(&mut Vdp, u16, Region) -> Result<ControlWrite, VdpPortError>,
) {
    let mut device = at(101);
    command(&mut device, 0, 1);
    device.write_data(0xabcd, REGION).unwrap();
    control(&mut device, 0x4123, REGION).unwrap();
    assert_eq!(device.control().pending_word(), Some(0x4123));
    let before = device.clone();
    assert_eq!(read(&mut device, REGION), Ok(8));
    assert_eq!(device.control().pending_word(), None);
    assert_eq!(device.command(), before.command());
    assert_eq!(device.fifo(), before.fifo());
    assert_eq!(device.read_cache(), before.read_cache());
    assert_eq!(device.memory(), before.memory());
    assert_eq!(device.interrupts(), before.interrupts());
    assert_eq!(device.registers(), before.registers());
    control(&mut device, 0x4567, REGION).unwrap();
    assert_eq!(device.control().pending_word(), Some(0x4567));
    assert_eq!(
        device.command(),
        ControlCommand {
            address: 0x567,
            code: 0x11
        }
    );
    assert_eq!(
        control(&mut device, 2, REGION),
        Ok(ControlWrite::Command(ControlCommand {
            address: 0x8567,
            code: 1,
        }))
    );
    assert_eq!(device.control().pending_word(), None);
    assert_eq!(
        device.command(),
        ControlCommand {
            address: 0x8567,
            code: 1
        }
    );
    unchanged(&mut device, REGION, Ok(8), &mut read);
}

pub fn fifo_flags_and_raw_history_are_read_only() {
    fifo_flags_and_raw_history_are_read_only_with(Vdp::read_status, Vdp::advance_to);
}
pub fn fifo_flags_and_raw_history_are_read_only_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<(), VdpScheduleError>,
) {
    let mut device = at(1);
    command(&mut device, 0, 1);
    unchanged(&mut device, REGION, Ok(EMPTY | 4), &mut read);
    for (index, word) in [0xa135, 0x2468, 0x3579, 0xfedc].into_iter().enumerate() {
        device.write_data(word, REGION).unwrap();
        unchanged(
            &mut device,
            REGION,
            Ok(12 | if index == 3 { 0x100 } else { 0 }),
            &mut read,
        );
    }
    for (now, count, expected) in [
        (10, 4, 0x10c),
        (30, 3, 12),
        (70, 2, 12),
        (110, 1, 8),
        (150, 0, EMPTY),
    ] {
        advance(&mut device, tick(now), REGION).unwrap();
        assert_eq!(device.fifo().len(), count);
        unchanged(&mut device, REGION, Ok(expected), &mut read);
    }
    assert_eq!(device.fifo().next_slot_data(), Some(0xa135));
}

pub fn vertical_pending_survives_status_until_acknowledge() {
    vertical_pending_survives_status_until_acknowledge_with(Vdp::read_status, Vdp::advance_to);
}
pub fn vertical_pending_survives_status_until_acknowledge_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<(), VdpScheduleError>,
) {
    let mut disabled = Vdp::bootstrap_mode5();
    advance(&mut disabled, tick(224 * 3420 + 11), REGION).unwrap();
    assert_eq!(disabled.interrupt_level(), 0);
    assert!(disabled.interrupts().vertical_pending());
    unchanged(&mut disabled, REGION, Ok(EMPTY | 0x84), &mut read);
    assert_eq!(disabled.interrupt_level(), 0);
    assert!(disabled.interrupts().vertical_pending());

    let mut device = at(101);
    register(&mut device, 1, 0x24);
    advance(&mut device, tick(224 * 3420 + 9), REGION).unwrap();
    unchanged(&mut device, REGION, Ok(EMPTY | 4), &mut read);
    advance(&mut device, tick(224 * 3420 + 10), REGION).unwrap();
    assert_eq!(device.interrupt_level(), 6);
    unchanged(&mut device, REGION, Ok(EMPTY | 0x84), &mut read);
    advance(&mut device, tick(224 * 3420 + 11), REGION).unwrap();
    assert_eq!(
        device.acknowledge_to(tick(device.now().get() + 1), REGION),
        Ok(6)
    );
    assert!(!device.interrupts().vertical_pending());
    unchanged(&mut device, REGION, Ok(EMPTY | 4), &mut read);
}

read_case!(
    ready_cache_rejection_is_atomic,
    ready_cache_rejection_is_atomic_with
);
pub fn ready_cache_rejection_is_atomic_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
) {
    let mut device = at(1);
    command(&mut device, 0, 0);
    device.advance_to(tick(31), REGION).unwrap();
    assert!(device.read_cache().is_ready());
    unchanged(
        &mut device,
        REGION,
        Err(VdpPortError::Status(VdpStatusError::ReadPending)),
        &mut read,
    );
}

pub fn pending_prefetch_rejection_is_atomic() {
    pending_prefetch_rejection_is_atomic_with(Vdp::read_status, Vdp::advance_to);
}
pub fn pending_prefetch_rejection_is_atomic_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
    mut advance: impl FnMut(&mut Vdp, MasterTicks, Region) -> Result<(), VdpScheduleError>,
) {
    let mut device = at(1);
    command(&mut device, 0, 0);
    for now in [1, 9, 10, 29] {
        advance(&mut device, tick(now), REGION).unwrap();
        assert!(!device.read_cache().is_ready());
        unchanged(
            &mut device,
            REGION,
            Err(VdpPortError::Status(VdpStatusError::ReadPending)),
            &mut read,
        );
    }
}

read_case!(
    invalid_cache_keeps_buffered_word,
    invalid_cache_keeps_buffered_word_with
);
pub fn invalid_cache_keeps_buffered_word_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
) {
    let mut device = at(1);
    command(&mut device, 0, 1);
    device.write_data(0xbeef, REGION).unwrap();
    command(&mut device, 0, 0);
    device.advance_to(tick(71), REGION).unwrap();
    assert_eq!(device.read_cache().buffered_word(), Some(0xbeef));
    command(&mut device, 0, 1);
    assert!(!device.read_cache().is_ready());
    unchanged(&mut device, REGION, Ok(EMPTY | 4), &mut read);
    assert_eq!(device.read_cache().buffered_word(), Some(0xbeef));
}

read_case!(
    display_history_remains_unknown_after_disable,
    display_history_remains_unknown_after_disable_with
);
pub fn display_history_remains_unknown_after_disable_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
) {
    let mut device = at(101);
    register(&mut device, 1, 0x44);
    unchanged(
        &mut device,
        REGION,
        Err(VdpPortError::Status(VdpStatusError::UnknownHistory)),
        &mut read,
    );
    register(&mut device, 1, 4);
    device.write_control(0x4002, REGION).unwrap();
    unchanged(
        &mut device,
        REGION,
        Err(VdpPortError::Status(VdpStatusError::UnknownHistory)),
        &mut read,
    );
    assert_eq!(device.control().pending_word(), Some(0x4002));
}

read_case!(
    unknown_snapshot_and_mode_guards_are_atomic,
    unknown_snapshot_and_mode_guards_are_atomic_with
);
pub fn unknown_snapshot_and_mode_guards_are_atomic_with(
    mut read: impl FnMut(&mut Vdp, Region) -> Result<u16, VdpPortError>,
) {
    let registers = *Vdp::bootstrap_mode5().registers();
    let interrupts = VdpInterrupts::at_counters_zero(tick(0), 0, true, true);
    let mut unknown = Vdp::from_known_state(interrupts, registers, REGION).unwrap();
    unknown.advance_to(tick(1), REGION).unwrap();
    unknown.write_control(0x4002, REGION).unwrap();
    unchanged(
        &mut unknown,
        REGION,
        Err(VdpPortError::Status(VdpStatusError::UnknownHistory)),
        &mut read,
    );
    let mut device = at(101);
    device.write_control(0x4002, REGION).unwrap();
    let mut pal = device.clone();
    let mut ntsc = device.clone();
    let expected = read(&mut ntsc, REGION).unwrap() | 1;
    assert_eq!(read(&mut pal, Region::Pal), Ok(expected));
    assert_eq!(pal, ntsc);
    for (index, value) in [(1, 0x44), (12, 0x81)] {
        let mut pending = device.clone();
        pending.queue_register(index, value, REGION).unwrap();
        unchanged(
            &mut pending,
            REGION,
            Err(timing_error(VdpTimingError::PendingModeChange)),
            &mut read,
        );
    }
    for mode in [0x44, 0x84] {
        let mut unsupported = device.clone();
        register(&mut unsupported, 1, mode);
        unchanged(
            &mut unsupported,
            REGION,
            Err(if mode == 0x44 {
                VdpPortError::Status(VdpStatusError::UnknownHistory)
            } else {
                timing_error(VdpTimingError::UnsupportedMode {
                    index: 1,
                    value: mode,
                })
            }),
            &mut read,
        );
    }
}
