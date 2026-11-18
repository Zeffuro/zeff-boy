use super::*;
#[path = "m5_vertical/performance.rs"]
mod performance;

const V30: u8 = MODE5_ENABLE | V30_ENABLE | DMA_ENABLE | VINT_ENABLE_MASK;

fn main_word(vdp: &mut Vdp, region: Region, word: u16) {
    let begin = vdp.now().get() + 40;
    vdp.advance_to(tick(begin), region).unwrap();
    vdp.begin_timed_control(tick(begin), tick(begin + 28), region)
        .unwrap();
    let mut other = restored(vdp, region);
    for machine in [&mut *vdp, &mut other] {
        machine.advance_to(tick(begin + 28), region).unwrap();
        machine.write_control(word, region).unwrap();
        machine
            .finish_timed_control(tick(begin + 28), region)
            .unwrap();
        if let Some(write) = machine.registers().pending() {
            machine.advance_to(write.publish_at, region).unwrap();
        }
    }
    assert_eq!(*vdp, other);
}

fn publication(vdp: &mut Vdp, region: Region, at: u64, value: u8) {
    let begin = (at - 29) / 7 * 7;
    vdp.advance_to(tick(begin), region).unwrap();
    vdp.begin_timed_control(tick(begin), tick(begin + 28), region)
        .unwrap();
    vdp.advance_to(tick(begin + 28), region).unwrap();
    vdp.write_control(0x8100 | u16::from(value), region)
        .unwrap();
    vdp.finish_timed_control(tick(begin + 28), region).unwrap();
    assert_eq!(vdp.registers().pending().unwrap().publish_at, tick(at));
    let mut other = restored(vdp, region);
    for machine in [&mut *vdp, &mut other] {
        machine.advance_to(tick(at), region).unwrap();
    }
    assert_eq!(*vdp, other);
}

fn seed(region: Region) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    publication(&mut vdp, region, 100, V30);
    assert!(!vdp.requires_timed_ports());
    assert!(
        vdp.state
            .interrupts
            .blank_history()
            .unwrap()
            .vertical
            .is_some()
    );
    main_word(&mut vdp, region, 0x8a00);
    vdp
}

pub(super) fn checks() {
    performance::checks();
    for region in [Region::NtscUs, Region::NtscJapan] {
        recurrence(region);
        restoration(region);
        width_and_scroll(region);
        ports_and_memory(region);
        external_port_horizon(region);
        dma_slots(region);
        publication_ledger(region);
        malformed(region);
        hc0_clock_selection(region);
    }
    retained_samples();
}

fn recurrence(region: Region) {
    let mut vdp = seed(region);
    let sound = vdp.interrupts().sound_interrupt(vdp.registers(), region);
    let onset = 240 * H32_LINE_TICKS + 10;
    for (at, expected) in [
        (onset - 1, false),
        (onset, true),
        (onset + 2579, true),
        (onset + 2580, false),
    ] {
        assert_eq!(sound.asserted_at(at), Ok(expected));
    }
    for row in 1..=512 {
        vdp.advance_to(tick(row * H32_LINE_TICKS), region).unwrap();
        let history = vdp.state.interrupts.blank_history().unwrap();
        assert_eq!(history.core.v, (row & 511) as u16);
        assert!(!history.core.reload && !history.core.prepared.reload);
        assert_eq!(vdp.field_count(), row / 512);
        if [224, 234, 240, 300, 511, 512].contains(&row) {
            assert_eq!(restored(&vdp, region), vdp);
            assert!(vdp.field_position(region).unwrap().projected_end.is_none());
        }
    }
    assert!(vdp.interrupts().horizontal_pending());
    assert!(vdp.interrupts().vertical_pending());
    let mut ack = seed(region);
    ack.advance_to(tick(onset + 20), region).unwrap();
    assert_eq!(
        ack.state
            .interrupts
            .complete_acknowledge(&ack.state.registers)
            .unwrap(),
        VINT_LEVEL
    );
    assert!(!ack.interrupts().vertical_pending());
    assert!(
        ack.interrupts()
            .sound_interrupt(ack.registers(), region)
            .asserted_at(onset + 30)
            .unwrap()
    );
}

fn restoration(region: Region) {
    for (row, active) in [
        (223, false),
        (225, true),
        (233, true),
        (235, true),
        (241, false),
    ] {
        let mut vdp = seed(region);
        publication(
            &mut vdp,
            region,
            row * H32_LINE_TICKS + 100,
            MODE5_ENABLE | DISPLAY_ENABLE | VINT_ENABLE_MASK,
        );
        let mut other = restored(&vdp, region);
        let at = tick(250 * H32_LINE_TICKS);
        vdp.advance_to(at, region).unwrap();
        other.advance_to(at, region).unwrap();
        assert_eq!(vdp, other);
        assert_eq!(
            vdp.state.interrupts.blank_history().unwrap().core.active,
            active
        );
        let slot = vdp
            .interrupts()
            .next_write_slot(at, region, vdp.registers())
            .unwrap();
        assert_eq!(slot.get() - at.get(), if active { 50 } else { 10 });
        assert!(
            vdp.interrupts()
                .is_machine_write_slot(slot, region, vdp.registers())
                .unwrap()
        );
    }
    for phase in [2640, 2660, 2680] {
        let mut vdp = Vdp::bootstrap_mode5();
        let at = 234 * H32_LINE_TICKS + phase;
        publication(&mut vdp, region, at, V30);
        let mut other = restored(&vdp, region);
        vdp.advance_to(tick(at + 40), region).unwrap();
        other.advance_to(tick(at + 40), region).unwrap();
        assert_eq!(vdp, other);
        let v = vdp.state.interrupts.blank_history().unwrap().core.v;
        assert_eq!(v, if phase == 2640 { 235 } else { 485 });
    }
    let mut vdp = seed(region);
    publication(
        &mut vdp,
        region,
        223 * H32_LINE_TICKS + 100,
        MODE5_ENABLE | DISPLAY_ENABLE | VINT_ENABLE_MASK,
    );
    vdp.advance_to(tick(260 * H32_LINE_TICKS), region).unwrap();
    let fetch = vdp.next_scroll_deadline(region).unwrap().unwrap();
    vdp.advance_to(fetch, region).unwrap();
    assert!(vdp.scroll_preparation(region));
    let next = vdp.next_row_deadline(region).unwrap().unwrap();
    let slot = vdp
        .interrupts()
        .next_write_slot(vdp.now(), region, vdp.registers())
        .unwrap();
    assert_eq!(slot.get(), next.get() + 50);
    vdp.advance_to(next, region).unwrap();
    let fetch = vdp.next_scroll_deadline(region).unwrap().unwrap();
    vdp.advance_to(fetch, region).unwrap();
    assert_eq!(vdp.scroll_fetch_target(region), Some((1, 0)));
}

fn width_and_scroll(region: Region) {
    let mut vdp = seed(region);
    vdp.advance_to(tick(300 * H32_LINE_TICKS), region).unwrap();
    main_word(&mut vdp, region, 0x8c01);
    assert_eq!(vdp.scroll_fetch_phase(region), Ok(3190));
    let next = vdp.next_row_deadline(region).unwrap().unwrap();
    let mut other = restored(&vdp, region);
    for machine in [&mut vdp, &mut other] {
        machine.advance_to(next, region).unwrap();
        assert_eq!(machine.effective_line_ticks(), 3360);
        assert_eq!(machine.scroll_fetch_phase(region), Ok(3152));
    }
    assert_eq!(vdp, other);
    let start = next.get();
    let onset = start + (240 + 512 - 301) * 3360 + 8;
    let sound = vdp.interrupts().sound_interrupt(vdp.registers(), region);
    for (at, expected) in [
        (onset - 1, false),
        (onset, true),
        (onset + 2575, true),
        (onset + 2576, false),
    ] {
        assert_eq!(sound.asserted_at(at), Ok(expected));
    }
    let last = start + (510 - 301) * 3360;
    vdp.advance_to(tick(last), region).unwrap();
    let fetch = vdp.next_scroll_deadline(region).unwrap().unwrap();
    vdp.advance_to(fetch, region).unwrap();
    assert!(vdp.scroll_preparation(region));
    let at = vdp.next_row_deadline(region).unwrap().unwrap();
    vdp.advance_to(at, region).unwrap();
    let fetch = vdp.next_scroll_deadline(region).unwrap().unwrap();
    vdp.advance_to(fetch, region).unwrap();
    assert_eq!(vdp.scroll_fetch_target(region), Some((1, 0)));
    let begin = vdp.now();
    vdp.begin_timed_status(begin, tick(begin.get() + 28), region)
        .unwrap();
    assert_eq!(vdp.field_position(region).unwrap().projected_end, None);
    assert_eq!(restored(&vdp, region), vdp);
}

fn ports_and_memory(region: Region) {
    let mut vdp = seed(region);
    for word in [0x8f02, 0x4000, 0] {
        main_word(&mut vdp, region, word);
    }
    for value in 0..8_u16 {
        let begin = vdp.now();
        vdp.begin_main_data(begin, true).unwrap();
        let mut end = tick(begin.get() + 28);
        vdp.advance_to(end, region).unwrap();
        if let Some(wait) = vdp.data_write_wait(region).unwrap() {
            end = tick(end.get() + (wait.get() - end.get()).div_ceil(7) * 7);
            vdp.advance_to(end, region).unwrap();
        }
        vdp.write_main_data(0xa000 | value, region).unwrap();
        assert_eq!(restored(&vdp, region), vdp);
    }
    let begin = tick(vdp.now().get() + 40);
    vdp.advance_to(begin, region).unwrap();
    vdp.begin_timed_status(begin, tick(begin.get() + 28), region)
        .unwrap();
    let origin = vdp.clone();
    let mut split = origin.clone();
    for offset in 1..=28 {
        let at = tick(begin.get() + offset);
        split.advance_to(at, region).unwrap();
        let mut whole = origin.clone();
        whole.advance_to(at, region).unwrap();
        assert_eq!(whole, split);
        assert_eq!(restored(&whole, region), whole);
    }
    let unknown = tick(begin.get() + 29);
    let boundary = Err(VdpScheduleError::Interrupt(
        crate::VdpInterruptError::Timing(crate::VdpTimingError::UnqualifiedMemoryInterval(unknown)),
    ));
    assert_eq!(
        split
            .interrupts()
            .next_write_slot(split.now(), region, split.registers()),
        boundary
    );
    assert_eq!(
        split
            .interrupts()
            .next_external_slot(split.now(), region, split.registers()),
        boundary
    );
    let before = split.clone();
    assert!(split.advance_to(unknown, region).is_err());
    assert_eq!(split, before);
    split.finish_timed_status(split.now(), region).unwrap();
    assert!(
        split
            .interrupts()
            .next_write_slot(split.now(), region, split.registers())
            .unwrap()
            > split.now()
    );
    assert!(
        split
            .interrupts()
            .next_external_slot(split.now(), region, split.registers())
            .unwrap()
            > split.now()
    );
    let mut before_capture = vdp.clone();
    before_capture
        .advance_to(tick(begin.get() + 24), region)
        .unwrap();
    let fifo_bits = before_capture.state.fifo.status_bits();
    let mut other = restored(&vdp, region);
    for at in [begin.get() + 7, begin.get() + 25, begin.get() + 28] {
        vdp.advance_to(tick(at), region).unwrap();
        other.advance_to(tick(at), region).unwrap();
        assert_eq!(vdp, other);
        other = restored(&other, region);
    }
    let end = vdp.now();
    let word = vdp.finish_timed_status(end, region).unwrap();
    assert_eq!(word & (FIFO_EMPTY_STATUS | FIFO_FULL_STATUS), fifo_bits);
    assert_eq!(word, other.finish_timed_status(end, region).unwrap());
    vdp.advance_to(tick(end.get() + 1000), region).unwrap();
    for value in 0..8_u16 {
        assert_eq!(
            vdp.memory
                .read_word(ControlCommand {
                    address: u32::from(value) * 2,
                    code: VRAM_READ_CODE
                })
                .unwrap(),
            0xa000 | value
        );
    }
    for word in [0, 0] {
        main_word(&mut vdp, region, word);
    }
    let begin = vdp.now();
    vdp.begin_main_data(begin, false).unwrap();
    let end = tick(begin.get() + 28);
    vdp.advance_to(end, region).unwrap();
    if let Some(wait) = vdp.data_read_wait(region).unwrap() {
        vdp.advance_to(wait, region).unwrap();
    }
    assert_eq!(vdp.read_main_data(region).unwrap(), 0xa000);
}

fn external_port_horizon(region: Region) {
    let mut vdp = seed(region);
    for word in [
        0x8f02, 0x9308, 0x9400, 0x9500, 0x9600, 0x9700, 0x4040, 0x0080,
    ] {
        main_word(&mut vdp, region, word);
    }
    assert_eq!(vdp.state.dma, Some(dma::Phase::External));
    let rom: Vec<u8> = (0..8_u16)
        .flat_map(|value| (0xb000 | value).to_be_bytes())
        .collect();
    let source = crate::VdpDmaSource::new(&rom, &[]);
    let begin = vdp.now();
    let end = tick(begin.get() + 28);
    vdp.begin_timed_status(begin, end, region).unwrap();
    let mut whole = restored(&vdp, region);
    for offset in [7, 24, 25, 28] {
        vdp.advance_with_source(tick(begin.get() + offset), region, source)
            .unwrap();
    }
    whole.advance_with_source(end, region, source).unwrap();
    assert_eq!(vdp, whole);
    assert_eq!(restored(&vdp, region), vdp);
    let before = vdp.clone();
    assert!(
        vdp.advance_with_source(tick(end.get() + 1), region, source)
            .is_err()
    );
    assert_eq!(vdp, before);
    assert_ne!(
        vdp.finish_timed_status(end, region).unwrap() & DMA_BUSY_STATUS,
        0
    );
    let next = vdp
        .interrupts()
        .next_external_slot(end, region, vdp.registers())
        .unwrap();
    assert!(next > end);
    vdp.advance_with_source(tick(end.get() + 1000), region, source)
        .unwrap();
    assert!(vdp.state.dma.is_none());
    for value in 0..8_u16 {
        assert_eq!(
            vdp.memory
                .read_word(ControlCommand {
                    address: 0x40 + u32::from(value) * 2,
                    code: VRAM_READ_CODE
                })
                .unwrap(),
            0xb000 | value
        );
    }
}

fn malformed(region: Region) {
    let seed = seed(region);
    for variant in 0..8 {
        let mut bad = seed.clone();
        let mut b = bad.state.interrupts.blank_history().unwrap();
        match variant {
            0 => b.active = true,
            1 => b.core.prepared.v ^= 1,
            2 => b.vertical.as_mut().unwrap().h40 += 1,
            3 => b.vertical.as_mut().unwrap().prepared_at = tick(b.now.get() + 1),
            4 => b.vertical.as_mut().unwrap().prepared_mode ^= MODE5_ENABLE,
            5 => b.next_clock = b.now,
            6 => b.vertical.as_mut().unwrap().line_start = tick(1),
            _ => b.vertical = None,
        }
        bad.state.interrupts.set_blank_history(Some(b));
        let mut writer = StateWriter::new();
        assert!(bad.write_state_in_region(&mut writer, region).is_err());
        assert_eq!(writer.position(), 0);
    }
    for word in [0x8100, 0x81bc, 0x8c81, 0x8c03, 0x8001] {
        let mut bad = seed.clone();
        let before = bad.clone();
        assert!(bad.write_control(word, region).is_err());
        assert_eq!(bad, before);
    }
    let bytes = encoded(&seed, region);
    let mut target = seed.clone();
    for end in [bytes.len() - 1, bytes.len() / 2] {
        assert!(
            target
                .read_state(&mut StateReader::new(&bytes[..end]), region)
                .is_err()
        );
        assert_eq!(target, seed);
    }
}

fn dma_slots(region: Region) {
    let mut vdp = seed(region);
    for word in [
        0x8f02, 0x9302, 0x9400, 0x9500, 0x9600, 0x9780, 0x4020, 0x0080,
    ] {
        main_word(&mut vdp, region, word);
    }
    assert_eq!(vdp.state.dma, Some(dma::Phase::FillWaiting));
    let begin = vdp.now();
    vdp.begin_main_data(begin, true).unwrap();
    vdp.advance_to(tick(begin.get() + 28), region).unwrap();
    vdp.write_main_data(0xabcd, region).unwrap();
    let mut other = restored(&vdp, region);
    let end = tick(vdp.now().get() + 1000);
    vdp.advance_to(end, region).unwrap();
    other.advance_to(end, region).unwrap();
    assert_eq!(vdp, other);
    assert!(vdp.state.dma.is_none());
    assert_ne!(&vdp.memory.vram()[0x20..0x26], &[0; 6]);
}

fn publication_ledger(region: Region) {
    let mut vdp = Vdp::bootstrap_mode5();
    vdp.advance_to(tick(70), region).unwrap();
    vdp.begin_timed_control(tick(70), tick(98), region).unwrap();
    vdp.advance_to(tick(98), region).unwrap();
    vdp.write_control(0x8100 | u16::from(V30), region).unwrap();
    vdp.finish_timed_control(tick(98), region).unwrap();
    vdp.advance_to(tick(99), region).unwrap();
    vdp.begin_timed_control(tick(99), tick(127), region)
        .unwrap();
    let before = vdp.clone();
    let history = vdp.state.interrupts.blank_history().unwrap();
    let sound = vdp.interrupts().sound_interrupt(vdp.registers(), region);
    assert_eq!(sound.asserted_at(110), Ok(false));
    assert_eq!(vdp, before);
    let mut other = restored(&vdp, region);
    for at in [100, 110, 113, 124, 127] {
        vdp.advance_to(tick(at), region).unwrap();
        other.advance_to(tick(at), region).unwrap();
        assert_eq!(vdp, other);
        assert_eq!(
            vdp.state.interrupts.blank_history().unwrap().last_release,
            history.last_release
        );
        other = restored(&other, region);
    }
    vdp.write_control(0x8a00, region).unwrap();
    vdp.finish_timed_control(tick(127), region).unwrap();
    let before = vdp.clone();
    assert!(vdp.finish_timed_control(tick(127), region).is_err());
    assert_eq!(vdp, before);
}

fn retained_samples() {
    let mut registers = VdpRegisterPipeline::bootstrap_mode5();
    registers.advance_to(tick(1)).unwrap();
    let write = registers.queue_control_register(1, V30).unwrap();
    registers.advance_to(write.publish_at).unwrap();
    let mut b = crate::hardware::vdp::timing::blank_mode::BlankHistory::new(MasterTicks::ZERO);
    b.core.v = 234;
    b.core.select = true;
    b.core.reload = true;
    b.core.prepare_clock(
        Region::NtscUs,
        &registers,
        265,
        Some(crate::hardware::vdp::timing::clock::LineClock::H32),
    );
    assert_eq!(b.core.prepared.v, 469);
    assert!(!b.core.prepared.reload);
    b.core.prepared.v = 485;
    b.core.commit_at(tick(2660), 266).unwrap();
    assert_eq!(b.core.v, 485);
}

fn hc0_clock_selection(region: Region) {
    let mut source = seed(region);
    for selected in [0, 1] {
        if selected != 0 {
            main_word(&mut source, region, 0x8c01);
        }
        let at = source.next_row_deadline(region).unwrap().unwrap();
        source.advance_to(at, region).unwrap();
        let mut bad = source.clone();
        let mut b = bad.state.interrupts.blank_history().unwrap();
        let v = b.vertical.as_mut().unwrap();
        v.clock = if selected == 0 {
            crate::hardware::vdp::timing::clock::LineClock::InternalH40
        } else {
            crate::hardware::vdp::timing::clock::LineClock::H32
        };
        b.next_clock = tick(at.get() + v.dot());
        bad.state.interrupts.set_blank_history(Some(b));
        let mut writer = StateWriter::new();
        assert!(bad.write_state_in_region(&mut writer, region).is_err());
        assert_eq!(writer.position(), 0);
        let mut pending = source.clone();
        pending
            .state
            .registers
            .queue_control_register(12, selected ^ 1)
            .unwrap();
        assert_eq!(restored(&pending, region), pending);
    }
}
