use super::*;

fn main_word(vdp: &mut Vdp, region: Region, word: u16) {
    let begin = vdp.now().get() + 40;
    vdp.advance_to(tick(begin), region).unwrap();
    vdp.begin_timed_control(tick(begin), tick(begin + 28), region)
        .unwrap();
    vdp.advance_to(tick(begin + 28), region).unwrap();
    vdp.write_control(word, region).unwrap();
    vdp.finish_timed_control(tick(begin + 28), region).unwrap();
    if let Some(write) = vdp.registers().pending() {
        vdp.advance_to(write.publish_at, region).unwrap();
    }
}

fn main_memory_history(region: Region) {
    let mut vdp = Vdp::bootstrap_mode5();
    main_word(&mut vdp, region, 0x8f02);
    main_word(&mut vdp, region, 0x4000);
    main_word(&mut vdp, region, 0);
    let mut legacy = vdp.clone();
    legacy.state.interrupts.set_blank_history(None);
    let mut waited = false;
    for index in 0..24 {
        let begin = vdp.now();
        vdp.begin_main_data(begin, true).unwrap();
        let held = vdp.clone();
        assert!(
            vdp.begin_timed_status(begin, tick(begin.get() + 28), region)
                .is_err()
        );
        assert_eq!(vdp, held);
        assert!(vdp.read_main_data(region).is_err());
        assert_eq!(vdp, held);
        let unfinished = restored(&vdp, region);
        assert_eq!(unfinished, vdp);
        let mut end = tick(begin.get() + 28);
        vdp.advance_to(end, region).unwrap();
        legacy.advance_to(end, region).unwrap();
        if let Some(deadline) = vdp.data_write_wait(region).unwrap() {
            waited = true;
            end = tick(end.get() + (deadline.get() - end.get()).div_ceil(7) * 7);
            vdp.advance_to(end, region).unwrap();
            legacy.advance_to(end, region).unwrap();
        }
        let data = 0xa000 | index;
        vdp.write_main_data(data, region).unwrap();
        legacy.write_data(data, region).unwrap();
        let history = vdp.state.interrupts.blank_history().unwrap();
        assert!(history.data_inflight.is_none() && !history.port_ready);
        assert_eq!(history.quiet_since, Some(end));
        let mut stripped = vdp.state.clone();
        stripped.interrupts.set_blank_history(None);
        assert_eq!(stripped, legacy.state);
        assert_eq!(vdp.memory, legacy.memory);
    }
    assert!(waited);
    let end = vdp.now();
    let before = vdp.clone();
    assert!(
        vdp.begin_timed_control(end, tick(end.get() + 28), region)
            .is_err()
    );
    assert_eq!(vdp, before);
    let mut other = restored(&vdp, region);
    let begin = tick(end.get() + 1);
    advance_pair(&mut vdp, &mut other, region, begin.get());
    for machine in [&mut vdp, &mut other] {
        machine
            .begin_timed_control(begin, tick(begin.get() + 28), region)
            .unwrap();
    }
    other = restored(&other, region);
    advance_pair(&mut vdp, &mut other, region, begin.get() + 28);
    for machine in [&mut vdp, &mut other] {
        machine.write_control(0x8f02, region).unwrap();
        machine
            .finish_timed_control(tick(begin.get() + 28), region)
            .unwrap();
    }
    assert_eq!(vdp, other);
    let publication = vdp.registers().pending().unwrap().publish_at.get();
    advance_pair(&mut vdp, &mut other, region, publication);
    while !vdp.state.fifo.is_empty() {
        let next = vdp
            .state
            .interrupts
            .next_write_slot(vdp.now(), region, vdp.registers())
            .unwrap();
        vdp.advance_to(next, region).unwrap();
    }
    main_word(&mut vdp, region, 0x8120);
    let mut rejected = vdp.clone();
    let before = rejected.clone();
    assert!(rejected.begin_main_data(rejected.now(), true).is_err());
    assert_eq!(rejected, before);
    for word in [0x8700, 0x8b00, 0x8c81, 0x8f02, 0x9001, 0x9100, 0x9200] {
        main_word(&mut vdp, region, word);
    }
    assert_eq!(vdp.registers().registers()[12], Some(0));
    assert_eq!(vdp.registers().pending(), None);
    assert!(vdp.requires_timed_ports());
    assert!(!vdp.field_presentation_supported());
    let mut other = restored(&vdp, region);
    let at = vdp.now().get() + 64;
    advance_pair(&mut vdp, &mut other, region, at);
}

fn pending_data_and_unknown_status(region: Region) {
    let mut pending = Vdp::bootstrap_mode5();
    let begin = pending.now();
    pending
        .begin_timed_control(begin, tick(begin.get() + 28), region)
        .unwrap();
    pending.advance_to(tick(28), region).unwrap();
    pending.write_control(0x8120, region).unwrap();
    pending.finish_timed_control(tick(28), region).unwrap();
    assert!(pending.registers().pending().is_some());
    let before = pending.clone();
    assert!(pending.begin_main_data(pending.now(), true).is_err());
    assert_eq!(pending, before);

    let mut unknown = Vdp::bootstrap_mode5();
    main_word(&mut unknown, region, 0x8144);
    unknown
        .advance_to(tick(unknown.now().get() + 40), region)
        .unwrap();
    assert!(!unknown.state.status_known);
    let mut pending_unknown = unknown.clone();
    let begin = pending_unknown.now();
    pending_unknown
        .begin_timed_control(begin, tick(begin.get() + 28), region)
        .unwrap();
    pending_unknown
        .advance_to(tick(begin.get() + 28), region)
        .unwrap();
    pending_unknown.write_control(0x8120, region).unwrap();
    pending_unknown
        .finish_timed_control(tick(begin.get() + 28), region)
        .unwrap();
    assert!(pending_unknown.registers().pending().is_some());
    let before = pending_unknown.clone();
    let begin = pending_unknown.now();
    assert!(
        pending_unknown
            .begin_timed_status(begin, tick(begin.get() + 28), region)
            .is_err()
    );
    assert_eq!(pending_unknown, before);
    main_word(&mut unknown, region, 0x8120);
    assert!(unknown.requires_timed_ports());
    let begin = unknown.now();
    let before = unknown.clone();
    assert!(
        unknown
            .begin_timed_status(begin, tick(begin.get() + 28), region)
            .is_err()
    );
    assert_eq!(unknown, before);
}

fn main_read_history(region: Region) {
    let mut vdp = Vdp::bootstrap_mode5();
    main_word(&mut vdp, region, 0x8f02);
    main_word(&mut vdp, region, 0x4000);
    main_word(&mut vdp, region, 0);
    let begin = vdp.now();
    vdp.begin_main_data(begin, true).unwrap();
    vdp.advance_to(tick(begin.get() + 28), region).unwrap();
    vdp.write_main_data(0xcafe, region).unwrap();
    vdp.advance_to(tick(vdp.now().get() + 512), region).unwrap();
    main_word(&mut vdp, region, 0);
    main_word(&mut vdp, region, 0);
    assert_eq!(vdp.state.read_phase, Some(false));
    assert!(vdp.state.interrupts.blank_history().is_some());
    let first = vdp
        .state
        .interrupts
        .next_write_slot(vdp.now(), region, vdp.registers())
        .unwrap();
    vdp.advance_to(first, region).unwrap();
    assert_eq!(vdp.state.read_phase, Some(true));
    let mut other = restored(&vdp, region);
    let second = vdp
        .state
        .interrupts
        .next_write_slot(vdp.now(), region, vdp.registers())
        .unwrap();
    advance_pair(&mut vdp, &mut other, region, second.get());
    assert!(vdp.read_cache().is_ready());
    let before = vdp.clone();
    let mut busy = vdp.clone();
    busy.begin_timed_status(second, tick(second.get() + 28), region)
        .unwrap();
    assert!(busy.state.interrupts.blank_history().is_none());
    assert_eq!(vdp, before);
    let begin = vdp.now();
    vdp.begin_main_data(begin, false).unwrap();
    vdp.advance_to(tick(begin.get() + 28), region).unwrap();
    assert_eq!(vdp.read_main_data(region).unwrap(), 0xcafe);
    assert_eq!(vdp.state.read_phase, Some(false));
    let history = vdp.state.interrupts.blank_history().unwrap();
    assert_eq!(history.quiet_since, Some(vdp.now()));
    assert!(history.data_inflight.is_none());
    main_word(&mut vdp, region, 0x8f02);
    assert!(vdp.state.read_phase.is_none());
    assert!(!vdp.read_cache().is_ready());
    assert!(vdp.state.interrupts.blank_history().is_some());
}

fn data_preserves_clear_pipeline(region: Region) {
    let mut vdp = Vdp::bootstrap_mode5();
    main_word(&mut vdp, region, 0x4000);
    main_word(&mut vdp, region, 0);
    vdp.advance_to(tick(3514), region).unwrap();
    vdp.begin_timed_status(tick(3514), tick(3542), region)
        .unwrap();
    vdp.advance_to(tick(3542), region).unwrap();
    vdp.finish_timed_status(tick(3542), region).unwrap();
    let c = vdp.state.interrupts.blank_history().unwrap().core;
    assert!(c.t6 && c.a && !c.b && !c.clear);
    let mut quiet = vdp.clone();
    vdp.begin_main_data(tick(3542), true).unwrap();
    let mut saw_clear = false;
    for at in [3549, 3550, 3559, 3560, 3569, 3570] {
        vdp.advance_to(tick(at), region).unwrap();
        quiet.advance_to(tick(at), region).unwrap();
        let c = vdp.state.interrupts.blank_history().unwrap().core;
        assert_eq!(c, quiet.state.interrupts.blank_history().unwrap().core);
        saw_clear |= c.clear;
    }
    assert!(saw_clear);
    vdp.write_main_data(0x1234, region).unwrap();
    assert_eq!(
        vdp.state.interrupts.blank_history().unwrap().core,
        quiet.state.interrupts.blank_history().unwrap().core
    );
}

fn completed_dma_control(region: Region) {
    let mut vdp = Vdp::bootstrap_mode5();
    for word in [
        0x8114, 0x8f02, 0x9301, 0x9400, 0x9500, 0x9600, 0x9770, 0x4000,
    ] {
        main_word(&mut vdp, region, word);
    }
    let begin = tick(vdp.now().get() + 40);
    let end = tick(begin.get() + 28);
    vdp.advance_to(begin, region).unwrap();
    vdp.begin_timed_control(begin, end, region).unwrap();
    vdp.advance_to(end, region).unwrap();
    let mut ram = [0; 65536];
    ram[..2].copy_from_slice(&[0xca, 0xfe]);
    let before = vdp.clone();
    assert!(
        vdp.write_control_from(
            0x0080,
            region,
            crate::hardware::vdp::VdpDmaSource::new(&[], &[])
        )
        .is_err()
    );
    assert_eq!(vdp, before);
    assert!(vdp.has_timed_control());
    let rom: [u8; 0] = [];
    let source = crate::hardware::vdp::VdpDmaSource::new(&rom, &ram);
    vdp.write_control_from(0x0080, region, source).unwrap();
    let history = vdp.state.interrupts.blank_history().unwrap();
    assert!(history.port.is_none());
    assert_eq!(history.control_completed, Some(end));
    assert_eq!(history.last_release, Some(tick(end.get() - 3)));
    let before = vdp.clone();
    assert!(
        vdp.finish_timed_control(tick(end.get() + 1), region)
            .is_err()
    );
    assert_eq!(vdp, before);
    vdp.finish_timed_control(end, region).unwrap();
    assert!(!vdp.has_timed_control());
    let release = vdp.external_dma_release(region, source).unwrap().unwrap();
    assert!(release > end);
    vdp.advance_with_source(release, region, source).unwrap();
    while !vdp.state.fifo.is_empty() {
        let next = vdp
            .state
            .interrupts
            .next_write_slot(vdp.now(), region, vdp.registers())
            .unwrap();
        vdp.advance_with_source(next, region, source).unwrap();
    }
    assert_eq!(&vdp.memory.vram()[..2], &[0xca, 0xfe]);
    assert_eq!(
        vdp.state.interrupts.blank_history().unwrap().last_release,
        Some(tick(end.get() - 3))
    );
}

fn malformed_bridge_packets(region: Region) {
    use crate::hardware::vdp::timing::blank_mode::BlankHistory;
    let mut near_end = BlankHistory::new(tick(u64::MAX - 100));
    let mut registers = VdpRegisterPipeline::bootstrap_mode5();
    near_end = near_end
        .advance_projection(tick(u64::MAX - 10), region, &mut registers)
        .unwrap();
    near_end.data_inflight = Some((near_end.now, true));
    near_end.port_ready = false;
    let mut quiet = BlankHistory::new(tick(100));
    quiet.port_ready = false;
    quiet.quiet_since = Some(tick(99));
    let mut receipt = BlankHistory::new(tick(100));
    receipt.control_completed = Some(tick(100));
    receipt.last_release = Some(tick(97));
    for bad in [near_end, quiet, receipt] {
        let mut writer = StateWriter::new();
        bad.write_state(&mut writer);
        assert!(BlankHistory::read_state(&mut StateReader::new(&writer.into_bytes())).is_err());
    }
}

pub(super) fn checks(region: Region) {
    main_memory_history(region);
    main_read_history(region);
    pending_data_and_unknown_status(region);
    data_preserves_clear_pipeline(region);
    completed_dma_control(region);
    malformed_bridge_packets(region);
}
