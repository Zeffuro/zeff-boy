use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use crate::hardware::vdp::{VdpDmaSource, VdpWrite};

fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

fn register(vdp: &mut Vdp, index: u8, value: u8, region: Region) {
    let mut at = vdp.now().get() + 1;
    if at.is_multiple_of(H32_INTERNAL_CLOCK_TICKS) {
        at += 1;
    }
    vdp.advance_to(tick(at), region).unwrap();
    let write = vdp.queue_register(index, value, region).unwrap();
    vdp.advance_to(write.publish_at, region).unwrap();
}

fn fixture(region: Region, size: u8, display: bool, latch: bool, v30: bool) -> Vdp {
    let mut vdp = Vdp::bootstrap_mode5();
    for (index, value) in [
        (DISPLAY_SIZE_REGISTER, size),
        (HINT_RELOAD_REGISTER, 2),
        (AUTO_INCREMENT_REGISTER, 2),
        (
            HINT_ENABLE_REGISTER,
            HINT_ENABLE_MASK | if latch { HV_COUNTER_LATCH_MASK } else { 0 },
        ),
        (
            MODE_REGISTER,
            MODE5_ENABLE
                | VINT_ENABLE_MASK
                | DMA_ENABLE
                | if display { DISPLAY_ENABLE } else { 0 }
                | if v30 { V30_ENABLE } else { 0 },
        ),
    ] {
        register(&mut vdp, index, value, region);
    }
    let at = tick(vdp.now().get() + 1);
    vdp.advance_to(at, region).unwrap();
    assert!(vdp.interval_hint.is_some());
    vdp
}

fn equal(candidate: &Vdp, oracle: &Vdp) {
    assert_eq!(candidate.state, oracle.state);
    assert_eq!(candidate.memory, oracle.memory);
    assert_eq!(candidate, oracle);
}

fn advance(candidate: &mut Vdp, oracle: &mut Vdp, at: MasterTicks, region: Region) {
    oracle.interval_hint = None;
    assert_eq!(
        candidate.advance_to(at, region),
        oracle.advance_to(at, region)
    );
    equal(candidate, oracle);
}

fn bytes(vdp: &Vdp, region: Region) -> Vec<u8> {
    let mut writer = StateWriter::new();
    vdp.write_state_in_region(&mut writer, region).unwrap();
    writer.into_bytes()
}

fn through_deadline(candidate: &mut Vdp, oracle: &mut Vdp, region: Region) {
    let deadline = candidate
        .state
        .interrupts
        .timing_next_edge(region, &candidate.state.registers)
        .unwrap()
        .unwrap()
        .at;
    for at in [deadline.get() - 1, deadline.get(), deadline.get() + 1] {
        advance(candidate, oracle, tick(at), region);
    }
}

#[test]
fn interval_sequence_crosses_every_counter_vint_and_field_boundary() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for size in [0, H40_CLOCK_MASK] {
            for display in [false, true] {
                for latch in [false, true] {
                    for v30 in [false, true] {
                        if v30 && region != Region::Pal {
                            continue;
                        }
                        let mut candidate = fixture(region, size, display, latch, v30);
                        let mut oracle = candidate.clone();
                        while candidate.field_count() < 2 {
                            let edge = candidate
                                .interrupts()
                                .timing_next_edge(region, candidate.registers())
                                .unwrap()
                                .unwrap()
                                .at;
                            let field = candidate
                                .interrupts()
                                .next_field_end(region, candidate.registers())
                                .unwrap();
                            let end = edge.min(field).get();
                            let interior = candidate.now().get() + 28;
                            if interior < end - 1 {
                                advance(&mut candidate, &mut oracle, tick(interior), region);
                            }
                            for at in [end - 1, end, end + 1] {
                                advance(&mut candidate, &mut oracle, tick(at), region);
                            }
                        }
                        assert_eq!(bytes(&candidate, region), bytes(&oracle, region));
                    }
                }
            }
        }
    }
}

#[test]
fn interval_long_28_tick_sequence_has_real_hits_and_canonical_restore() {
    let region = Region::NtscUs;
    let mut candidate = fixture(region, H40_CLOCK_MASK, true, true, false);
    let mut oracle = candidate.clone();
    COUNTS.with(|cell| cell.set(Counts::default()));
    for _ in 0..128 {
        candidate
            .advance_to(tick(candidate.now().get() + 28), region)
            .unwrap();
    }
    let counts = COUNTS.with(|cell| cell.get());
    eprintln!("interval-counts {counts:?}");
    assert!(counts.values[Metric::Hit as usize] > 100);
    assert!(counts.longest > 50);
    oracle.interval_hint = None;
    oracle.advance_to(candidate.now(), region).unwrap();
    equal(&candidate, &oracle);
    for _ in 0..(region.frame_lines() * H32_LINE_TICKS / 28 + 128) {
        let at = tick(candidate.now().get() + 28);
        advance(&mut candidate, &mut oracle, at, region);
    }
    let warm = candidate.clone();
    let mut cold = candidate.clone();
    cold.interval_hint = None;
    assert_eq!(warm, cold);
    assert_eq!(format!("{warm:?}"), format!("{cold:?}"));
    assert_eq!(bytes(&warm, region), bytes(&cold, region));
    let encoded = bytes(&warm, region);
    cold.read_state(&mut StateReader::new(&encoded), region)
        .unwrap();
    assert!(cold.interval_hint.is_none());
    equal(&warm, &cold);
    through_deadline(&mut candidate, &mut cold, region);
    let checkpoint = candidate.checkpoint();
    candidate
        .advance_to(tick(candidate.now().get() + 28), region)
        .unwrap();
    candidate.restore(checkpoint);
    assert!(candidate.interval_hint.is_none());
    equal(&candidate, &cold);
    let checkpoint = candidate.clone();
    assert!(
        candidate
            .read_state(&mut StateReader::new(&encoded[..5]), region)
            .is_err()
    );
    equal(&candidate, &checkpoint);
    assert_eq!(
        candidate
            .interval_hint
            .map(|hint| (hint.region, hint.expected_now, hint.end)),
        checkpoint
            .interval_hint
            .map(|hint| (hint.region, hint.expected_now, hint.end))
    );
    candidate.power_on();
    oracle.power_on();
    assert!(candidate.interval_hint.is_none());
    equal(&candidate, &oracle);
    through_deadline(&mut candidate, &mut oracle, region);
}

#[test]
fn interval_register_publications_and_interlace_transitions_use_walker() {
    let region = Region::Pal;
    let mut candidate = fixture(region, 0, false, false, false);
    let mut oracle = candidate.clone();
    for (index, value) in [
        (HINT_RELOAD_REGISTER, 0),
        (MODE_REGISTER, MODE5_ENABLE | DISPLAY_ENABLE | V30_ENABLE),
        (DISPLAY_SIZE_REGISTER, H40_CLOCK_MASK),
        (DISPLAY_SIZE_REGISTER, H40_CLOCK_MASK | INTERLACE_MODE_MASK),
        (DISPLAY_SIZE_REGISTER, H40_CLOCK_MASK),
        (MODE_REGISTER, MODE5_ENABLE),
    ] {
        let at = tick(candidate.now().get() + 1);
        advance(&mut candidate, &mut oracle, at, region);
        assert_eq!(
            candidate.queue_register(index, value, region),
            oracle.queue_register(index, value, region)
        );
        assert!(candidate.interval_hint.is_none());
        equal(&candidate, &oracle);
        let at = candidate.registers().pending().unwrap().publish_at;
        advance(&mut candidate, &mut oracle, tick(at.get() - 1), region);
        assert!(candidate.interval_hint.is_none());
        advance(&mut candidate, &mut oracle, at, region);
        let end = candidate
            .interrupts()
            .next_field_end(region, candidate.registers())
            .unwrap();
        for at in [end.get() - 1, end.get(), end.get() + 1] {
            advance(&mut candidate, &mut oracle, tick(at), region);
        }
    }
}

#[test]
fn interval_mutable_ports_and_failure_invalidate_before_continuation() {
    let region = Region::NtscUs;
    let source_bytes = [0; 16];
    let source = VdpDmaSource::new(&source_bytes, &[0; 65536]);
    let initial = fixture(region, 0, false, false, false);
    macro_rules! operation {
        ($body:expr) => {{
            let mut candidate = initial.clone();
            let mut oracle = initial.clone();
            oracle.interval_hint = None;
            let apply = $body;
            assert_eq!(apply(&mut candidate), apply(&mut oracle));
            assert!(candidate.interval_hint.is_none());
            equal(&candidate, &oracle);
            through_deadline(&mut candidate, &mut oracle, region);
        }};
    }
    operation!(|v: &mut Vdp| v.read_data(region));
    operation!(|v: &mut Vdp| v.read_status(region));
    operation!(|v: &mut Vdp| v.read_cpu_status(region));
    operation!(|v: &mut Vdp| v.queue_register(REGISTER_COUNT, 0, region));
    operation!(|v: &mut Vdp| v.write_control(0x8000, region));
    operation!(|v: &mut Vdp| v.write_data(0xcafe, region));
    operation!(|v: &mut Vdp| v.write_control_from(0x4000, region, source));
    operation!(|v: &mut Vdp| v.resume_dma_from_known_phase(v.command(), None, region));
    operation!(|v: &mut Vdp| v.acknowledge_to(v.now(), region));
    operation!(|v: &mut Vdp| v.acknowledge_to(tick(v.now().get() + 1), region));
    operation!(|v: &mut Vdp| v.capture_line_at(v.now(), region));
    operation!(|v: &mut Vdp| v.advance_with_line_from(v.now(), v.now(), false, region, None));
    operation!(|v: &mut Vdp| v.advance_dma_from(v.now(), region, false, None));
    operation!(|v: &mut Vdp| v.advance_dma_with_line_from(v.now(), v.now(), false, region, None));
    operation!(|v: &mut Vdp| v.capture_dma_line_at(v.now(), region));
    operation!(|v: &mut Vdp| v.acknowledge_cpu());
    operation!(|v: &mut Vdp| v.publish(data::Prepared {
        state: v.state.clone(),
        writes: [None; WRITE_BATCH_CAPACITY],
        level: None,
        dma_activity: false,
    }));
    let mut candidate = initial.clone();
    let mut oracle = initial.clone();
    let end = candidate.interval_hint.unwrap().end;
    assert_eq!(
        candidate.acknowledge_to(end, region),
        oracle.acknowledge_to(end, region)
    );
    equal(&candidate, &oracle);
    through_deadline(&mut candidate, &mut oracle, region);
}

#[test]
fn interval_pending_transfers_refuse_all_dma_phases_and_reads() {
    let region = Region::NtscUs;
    let initial = fixture(region, 0, false, false, false);
    for phase in [
        None,
        Some(dma::Phase::External),
        Some(dma::Phase::FillWaiting),
        Some(dma::Phase::Fill(0x1234)),
        Some(dma::Phase::Copy(None)),
    ] {
        for read in [false, true] {
            let mut candidate = initial.clone();
            candidate.invalidate_interval();
            candidate.state.dma = phase;
            candidate.state.read_phase = read.then_some(false);
            if phase.is_none() && !read {
                candidate
                    .state
                    .fifo
                    .push(VdpWrite {
                        command: ControlCommand {
                            address: 0,
                            code: VRAM_WRITE_CODE,
                        },
                        data: 0xcafe,
                    })
                    .unwrap();
            }
            candidate.seed_interval(region);
            assert!(candidate.interval_hint.is_none());
            let saved = candidate.clone();
            let at = tick(candidate.now().get() + 28);
            assert!(!candidate.advance_interval(at, region));
            equal(&candidate, &saved);
        }
    }
}

#[test]
fn interval_reversal_equal_time_region_and_overflow_preserve_errors() {
    let region = Region::NtscUs;
    let mut candidate = fixture(region, 0, false, false, false);
    let mut oracle = candidate.clone();
    for (at, requested_region) in [
        (candidate.now().get(), region),
        (candidate.now().get() - 1, region),
        (candidate.now().get() + 1, Region::Pal),
    ] {
        advance(&mut candidate, &mut oracle, tick(at), requested_region);
    }
    let epoch = tick(u64::MAX - 195);
    let mut registers = VdpRegisterPipeline::at_h32_edge(epoch);
    for (index, value) in [(0, 0), (1, MODE5_ENABLE), (10, 0), (12, 0)] {
        registers
            .advance_to(tick(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    let mut interrupts = VdpInterrupts::at_counters_zero(epoch, 0, false, false);
    while interrupts
        .advance_in_mode(registers.now(), region, &registers, false)
        .unwrap()
        .is_some()
    {}
    let mut candidate = Vdp::from_known_state(interrupts, registers, region).unwrap();
    let mut oracle = candidate.clone();
    for at in [u64::MAX - 1, u64::MAX, u64::MAX - 1] {
        advance(&mut candidate, &mut oracle, tick(at), region);
        assert!(candidate.interval_hint.is_none());
    }
}

#[test]
fn interval_unknown_registers_and_clock_phase_mismatch_keep_first_error() {
    let region = Region::NtscUs;
    let mut initial = fixture(region, 0, false, false, false);
    initial.advance_to(tick(1001), region).unwrap();
    for missing in [Some(0), Some(1), Some(10), Some(12), None] {
        for epoch in [0, 1] {
            let mut candidate = initial.clone();
            candidate.invalidate_interval();
            let mut registers = VdpRegisterPipeline::at_h32_edge(tick(epoch));
            for index in 0..REGISTER_COUNT {
                if missing == Some(index) {
                    continue;
                }
                registers
                    .advance_to(tick(registers.now().get() + 1))
                    .unwrap();
                let write = registers
                    .queue_register(
                        index,
                        initial.registers().registers()[usize::from(index)].unwrap(),
                    )
                    .unwrap();
                registers.advance_to(write.publish_at).unwrap();
            }
            registers.advance_to(initial.now()).unwrap();
            candidate.state.registers = registers;
            if missing.is_none() && epoch == 0 {
                candidate.state.registers.advance_to(tick(1002)).unwrap();
            }
            candidate.seed_interval(region);
            assert!(candidate.interval_hint.is_none());
            let mut oracle = candidate.clone();
            for at in [1002, 1001, 3421] {
                advance(&mut candidate, &mut oracle, tick(at), region);
            }
        }
    }
}

fn command(vdp: &mut Vdp, address: u32, code: u8, region: Region) {
    vdp.write_control(
        (address as u16 & FIRST_ADDRESS_MASK) | (u16::from(code & 3) << FIRST_CODE_SHIFT),
        region,
    )
    .unwrap();
    vdp.write_control(
        ((address >> SECOND_ADDRESS_SHIFT) as u16 & SECOND_ADDRESS_MASK)
            | (u16::from(code & !3) << SECOND_CODE_SHIFT),
        region,
    )
    .unwrap();
}

#[test]
fn interval_real_dma_fifo_and_read_publications_match_original() {
    let region = Region::NtscUs;
    for mode in [0, DMA_FILL_MODE, DMA_COPY_MODE] {
        let mut candidate = fixture(region, 0, false, false, false);
        register(&mut candidate, DMA_LENGTH_LOW_REGISTER, 2, region);
        register(&mut candidate, DMA_SOURCE_REGISTER, mode, region);
        candidate.invalidate_interval();
        candidate
            .memory
            .write_word(
                ControlCommand {
                    address: 0,
                    code: VRAM_WRITE_CODE,
                },
                0x1234,
            )
            .unwrap();
        command(
            &mut candidate,
            0x2000,
            if mode == DMA_COPY_MODE { 0x30 } else { 0x21 },
            region,
        );
        assert!(candidate.interval_hint.is_none());
        let mut oracle = candidate.clone();
        if mode == DMA_FILL_MODE {
            let at = tick(candidate.now().get() + 1);
            advance(&mut candidate, &mut oracle, at, region);
            assert_eq!(candidate.state.dma, Some(dma::Phase::FillWaiting));
            assert!(candidate.interval_hint.is_none());
            assert_eq!(
                candidate.write_data(0xabcd, region),
                oracle.write_data(0xabcd, region)
            );
        }
        let source = VdpDmaSource::new(&[0xbe, 0xef, 0x12, 0x34], &[]);
        let mut saw_copy_latch = false;
        let mut saw_fill = false;
        for _ in 0..40 {
            let at = tick(candidate.now().get() + 20);
            oracle.interval_hint = None;
            assert_eq!(
                candidate.advance_with_source(at, region, source),
                oracle.advance_with_source(at, region, source)
            );
            equal(&candidate, &oracle);
            saw_copy_latch |= matches!(candidate.state.dma, Some(dma::Phase::Copy(Some(_))));
            saw_fill |= matches!(candidate.state.dma, Some(dma::Phase::Fill(_)));
            if candidate.state.dma.is_some() || !candidate.fifo().is_empty() {
                assert!(candidate.interval_hint.is_none());
            }
        }
        assert!(candidate.state.dma.is_none() && candidate.fifo().is_empty());
        match mode {
            0 => assert_eq!(
                &candidate.memory.vram()[0x2000..0x2004],
                &[0xbe, 0xef, 0x12, 0x34]
            ),
            DMA_FILL_MODE => {
                assert!(saw_fill);
                assert_eq!(candidate.memory.vram()[0x2003], 0xab);
            }
            _ => {
                assert!(saw_copy_latch);
                assert_eq!(candidate.memory.vram()[0x2001], 0x34);
            }
        }
        through_deadline(&mut candidate, &mut oracle, region);
    }
    let mut candidate = fixture(region, 0, false, false, false);
    let mut oracle = candidate.clone();
    command(&mut candidate, 0x100, VRAM_WRITE_CODE, region);
    command(&mut oracle, 0x100, VRAM_WRITE_CODE, region);
    assert_eq!(
        candidate.write_data(0xcafe, region),
        oracle.write_data(0xcafe, region)
    );
    for _ in 0..8 {
        let at = tick(candidate.now().get() + 20);
        advance(&mut candidate, &mut oracle, at, region);
    }
    assert_eq!(&candidate.memory.vram()[0x100..0x102], &[0xca, 0xfe]);
    command(&mut candidate, 0x100, VRAM_READ_CODE, region);
    command(&mut oracle, 0x100, VRAM_READ_CODE, region);
    for _ in 0..8 {
        let at = tick(candidate.now().get() + 20);
        advance(&mut candidate, &mut oracle, at, region);
    }
    assert_eq!(candidate.read_data(region), Ok(0xcafe));
    assert_eq!(oracle.read_data(region), Ok(0xcafe));
    equal(&candidate, &oracle);
    through_deadline(&mut candidate, &mut oracle, region);
}
