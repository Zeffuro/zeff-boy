use super::*;
use crate::hardware::vdp::{VdpPortError, VdpReadError};
use zeff_emu_common::time::MasterTicks;

fn command(vdp: &mut Vdp, address: u16, code: u8) {
    vdp.write_control(address | (u16::from(code & 3) << 14), crate::Region::NtscUs)
        .unwrap();
    vdp.write_control(u16::from(code >> 2) << 4, crate::Region::NtscUs)
        .unwrap();
}

fn pair_at(at: u64, vdp: Vdp) -> (Machine, Machine) {
    let mut expected = Machine::new(false);
    expected.bus.now = MasterTicks::new(at);
    expected.bus.vdp = vdp;
    expected.bus.audio.advance_to(at);
    let mut actual = expected.clone();
    actual.schedule.begin_epoch_field();
    (actual, expected)
}

fn queued_read() -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    vdp.advance_to(MasterTicks::new(1), crate::Region::NtscUs)
        .unwrap();
    vdp.write_control(0x8f02, crate::Region::NtscUs).unwrap();
    vdp.advance_to(MasterTicks::new(56), crate::Region::NtscUs)
        .unwrap();
    command(&mut vdp, 0x2000, 1);
    for word in [0x1234, 0x5678, 0x9abc, 0xdef0] {
        vdp.write_data(word, crate::Region::NtscUs).unwrap();
    }
    command(&mut vdp, 0x2000, 0);
    vdp
}

#[test]
fn status_and_hv_reads_keep_the_existing_cache_and_real_port_effects() {
    for address in [0xc00004, 0xc00006, 0xd8ff06, 0xc00008, 0xc0000e, 0xd8ff0e] {
        for lanes in [ByteLanes::Both, ByteLanes::Upper, ByteLanes::Lower] {
            let (mut actual, mut expected) = running_pair();
            for machine in [&mut actual, &mut expected] {
                machine
                    .bus
                    .vdp
                    .write_control(0x4567, crate::Region::NtscUs)
                    .unwrap();
            }
            let origin = actual.schedule.epoch.as_ref().unwrap().records[0]
                .as_ref()
                .unwrap()
                .at;
            let mut request = access(address);
            request.lanes = lanes;
            let transfer = |machine: &mut Machine| {
                let start = machine.bus.cpu_clock;
                ScheduledBus::new(&mut machine.bus, &mut machine.cpu, &mut machine.schedule)
                    .read_cycle(request, start)
            };
            let a = transfer(&mut actual);
            let b = transfer(&mut expected);
            assert_eq!(a, b);
            a.result.unwrap();
            assert!(actual.schedule.pending.is_none());
            assert!(actual.schedule.epoch_deadline().is_some());
            assert_eq!(
                actual.schedule.epoch.as_ref().unwrap().records[0]
                    .as_ref()
                    .unwrap()
                    .at,
                origin
            );
            assert_eq!(
                actual.bus.vdp.control().pending_word(),
                if address & VDP_PORT_MASK >= VDP_HV_START {
                    Some(0x4567)
                } else {
                    None
                }
            );
            equal_machine(&actual, &expected);
        }
    }
}

#[test]
fn data_read_waits_and_prefetch_effects_preserve_local_cache() {
    for address in [0xc00000, 0xc00002, 0xd8ff02] {
        let (mut actual, mut expected) = pair_at(56, queued_read());
        assert_eq!(actual.read(8, None), expected.read(8, None));
        assert!(actual.schedule.epoch_deadline().is_some());
        let a = actual.read(address, None);
        let b = expected.read(address, None);
        assert_eq!(a, b);
        assert_eq!(a.result, Ok(0x1234));
        assert!(a.wait_clocks > 0);
        assert!(actual.schedule.pending.is_none());
        assert!(actual.schedule.epoch_deadline().is_some());
        assert_eq!(actual.bus.vdp.command().address, 0x2002);
        assert!(!actual.bus.vdp.read_cache().is_ready());
        equal_machine(&actual, &expected);
    }
}

#[test]
fn irq_edge_during_vdp_read_keeps_pending_sample_then_uses_actual_next_sample() {
    let edge = 224 * VDP_LINE_TICKS + 40;
    let begin = edge - 52;
    let mut vdp = Vdp::bootstrap_mode5();
    vdp.advance_to(MasterTicks::new(begin), crate::Region::NtscUs)
        .unwrap();
    let (mut actual, mut expected) = pair_at(begin, vdp);
    assert_eq!(actual.read(8, None), expected.read(8, None));
    assert!(actual.bus.now.get() < edge);
    assert_eq!(actual.read(0xc00004, None), expected.read(0xc00004, None));
    assert!(actual.bus.now.get() > edge);
    assert_eq!(actual.schedule.epoch_deadline(), Some(edge + 5));
    assert!(
        !expected
            .schedule
            .pending
            .as_ref()
            .unwrap()
            .memory
            .irq_asserted
    );
    equal_machine(&actual, &expected);
    assert_eq!(actual.read(0xc00008, None), expected.read(0xc00008, None));
    assert!(actual.schedule.epoch_deadline().is_none());
    assert!(
        actual
            .schedule
            .pending
            .as_ref()
            .unwrap()
            .memory
            .irq_asserted
    );
    equal_machine(&actual, &expected);
}

#[test]
fn late_data_read_failure_materializes_the_completed_prefix_and_pending_cut() {
    let mut vdp = queued_read();
    vdp.advance_to(MasterTicks::new(482), crate::Region::NtscUs)
        .unwrap();
    let (mut actual, mut expected) = pair_at(482, vdp);
    assert_eq!(actual.read(8, None), expected.read(8, None));
    for machine in [&mut actual, &mut expected] {
        command(&mut machine.bus.vdp, 0x4e, 4);
    }
    let before = actual.bus.now;
    let a = actual.read(0xc00000, None);
    let b = expected.read(0xc00000, None);
    assert_eq!(a, b);
    assert!(matches!(
        a.result,
        Err(BusError::VdpPort(VdpPortError::Read(
            VdpReadError::UnsupportedCommand(_)
        )))
    ));
    assert!(a.wait_clocks > 0);
    assert!(actual.bus.now > before);
    assert!(actual.cpu.cycles() < expected.cpu.cycles());
    let actual = actual.canonical();
    assert!(actual.schedule.epoch.is_none());
    equal_machine(&actual, &expected);
}

#[test]
fn psg_and_unmapped_read_aliases_still_materialize_before_failure() {
    for address in [0xc00010, 0xd8ff10, 0xc00018, 0xc00020, 0xcf0004] {
        let (mut actual, mut expected) = running_pair();
        let a = actual.read(address, None);
        let b = expected.read(address, None);
        assert_eq!(a, b);
        assert!(matches!(a.result, Err(BusError::UnsupportedAccess { .. })));
        assert!(actual.schedule.epoch_deadline().is_none());
        assert!(actual.schedule.pending.is_some());
        equal_machine(&actual, &expected);
    }
}
