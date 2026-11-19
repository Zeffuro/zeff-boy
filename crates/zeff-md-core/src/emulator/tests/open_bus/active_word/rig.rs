use super::*;

const NEXT: u16 = 0x0240;
const TRANSFER_TICKS: u64 = TRANSFER_CLOCKS as u64 * MAIN_CPU_DIVISOR;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Crossing,
    Pending,
    Granted,
    Reset,
}

pub(super) fn value(phase: Phase) -> u16 {
    (NEXT & 0xfeff) | (u16::from(matches!(phase, Phase::Pending | Phase::Reset)) << 8)
}

pub(super) fn fixture(region: Region, phase: Phase, padding: usize) -> Emulator {
    let mut program = vec![0x4e71; padding];
    program.extend([0x34bc, 0x0100, 0x3012, NEXT, 0xffff, 0x60fe, 0x4e71]);
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut machine = Emulator::new(
        image,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
    )
    .unwrap();
    machine.cpu.registers.a[2] = SOUND_BUS_REQUEST;
    machine.cpu.registers.d[0] = 0x1234_beef;
    for _ in 0..padding {
        machine.step_instruction().unwrap();
    }
    assert_eq!(machine.queued_words(), Some([0x34bc, 0x0100]));
    if phase != Phase::Reset {
        machine.bus.sound.set_reset(false);
    }
    match phase {
        Phase::Crossing | Phase::Pending => {
            let sound: &[u8] = if phase == Phase::Crossing {
                &[0x3e, 0x5a, 0x76]
            } else {
                &[0x01, 0x34, 0x12, 0x76]
            };
            machine.bus.sound.ram[..sound.len()].copy_from_slice(sound);
            let pending = PendingSound::prepare(
                &machine.sound_cpu,
                &machine.bus.sound,
                &machine.bus.cartridge,
                machine.bus.now.get(),
            )
            .unwrap();
            assert!(pending.bank_access.is_none());
            assert!(!pending.any_bank_access);
            assert!(pending.reservation().is_none());
            machine.sound_schedule.pending = Some(pending);
        }
        Phase::Granted => {
            machine.bus.sound.request(true);
            machine.bus.sound.complete_boundary();
        }
        Phase::Reset => {}
    }
    let mut control = machine.clone();
    control.step_instruction().unwrap();
    let capture = Capture::begin().unwrap();
    machine.step_instruction().unwrap();
    let report = capture.finish(&machine).unwrap();
    assert!(report.complete);
    assert_eq!(machine, control);
    let records: Vec<_> = report.history.iter().flatten().collect();
    let write = records
        .iter()
        .position(|record| {
            record.callback == Callback::Write
                && record
                    .access
                    .is_some_and(|access| access.bus_address == SOUND_BUS_REQUEST)
        })
        .unwrap();
    assert_eq!(records[write].write_value, Some(0x0100));
    let fetched = records[write + 1];
    assert_eq!(fetched.callback, Callback::Read);
    assert_eq!(fetched.access.unwrap().kind, AccessKind::Prefetch);
    assert_eq!(fetched.access.unwrap().lanes, ByteLanes::Both);
    assert_eq!(fetched.outcome, Some(Outcome::Read(NEXT)));
    assert_eq!(
        fetched.returned.unwrap().bus.prefetch,
        Observation::Full(NEXT)
    );
    assert_eq!(machine.queued_words(), Some([0x3012, NEXT]));
    assert_eq!(machine.bus.prefetch, Prefetch::Full(NEXT));
    assert!(machine.bus.sound.requested);
    assert_eq!(machine.bus.sound.granted, phase == Phase::Granted);
    if let Some(pending) = &machine.sound_schedule.pending {
        let begin = machine.bus.now.get();
        assert!(pending.deadline > begin);
        assert_eq!(
            pending.deadline <= begin + TRANSFER_TICKS,
            phase == Phase::Crossing
        );
    } else {
        assert!(matches!(phase, Phase::Granted | Phase::Reset));
    }
    machine
}

pub(super) fn busreq_record(report: &Report) -> &BusRecord {
    assert!(report.complete);
    report
        .history
        .iter()
        .flatten()
        .find(|record| {
            record.callback == Callback::Read
                && record
                    .access
                    .is_some_and(|access| access.bus_address == SOUND_BUS_REQUEST)
        })
        .unwrap()
}

pub(super) fn assert_real_read(report: &Report, machine: &Emulator, phase: Phase) {
    let read = busreq_record(report);
    let access = read.access.unwrap();
    assert_eq!(
        access,
        super::access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Both)
    );
    assert_eq!(read.entry.bus.prefetch, Observation::Full(NEXT));
    assert_eq!(read.entry.bus.sound_granted, phase == Phase::Granted);
    let service = read.service.unwrap();
    assert_eq!(
        service.master_ticks - read.entry.bus.master_ticks,
        TRANSFER_TICKS
    );
    assert_eq!(read.wait_clocks, 0);
    assert_eq!(
        service.sound_granted,
        matches!(phase, Phase::Crossing | Phase::Granted)
    );
    assert_eq!(service.sound_reset, phase == Phase::Reset);
    if matches!(phase, Phase::Crossing | Phase::Pending) {
        let pending = read.entry.schedule.unwrap().pending.unwrap();
        assert!(pending.bank_access.is_none());
        assert!(!pending.any_bank_access);
        assert!(pending.reservation.is_none());
        assert!(pending.deadline > read.entry.bus.master_ticks);
        assert_eq!(
            pending.deadline <= service.master_ticks,
            phase == Phase::Crossing
        );
    }
    assert!(!service.vdp.dma_active);
    if read.outcome
        == Some(Outcome::UnsupportedAccess {
            address: SOUND_BUS_REQUEST,
            direction: Direction::Read,
        })
    {
        assert_eq!(machine.registers().d[0], 0x1234_beef);
        assert_eq!(service.prefetch, Observation::Empty);
        assert_eq!(report.first_failed_sequence, Some(read.sequence));
        let entry = report.entry.unwrap();
        assert_eq!(entry.boundary, read.boundary);
        assert_eq!(entry.snapshot.registers.pc, machine.registers().pc);
        assert!(matches!(
            entry.snapshot.queued_words,
            Some([0x3012, NEXT] | [0x3039, 0x00a1])
        ));
    }
    assert_eq!(service.prefetch, Observation::Full(NEXT));
    assert_eq!(read.outcome, Some(Outcome::Read(value(phase))));
    assert_eq!(read.returned.unwrap().bus.prefetch, Observation::Empty);
    assert_eq!(report.first_failed_sequence, None);
}

pub(super) fn emit_record(phase: Phase, padding: usize, record: &BusRecord) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!(
        "busreq-active-witness-v1 {phase:?} padding={padding} {record:?}"
    );
    #[cfg(not(target_arch = "wasm32"))]
    println!("busreq-active-witness-v1 {phase:?} padding={padding} {record:?}");
}

pub(super) fn compare(actual: &mut Emulator, control: &mut Emulator, hash: &mut Sha256) {
    assert_eq!(actual, control);
    assert_eq!(actual.timing_snapshot(), control.timing_snapshot());
    assert_eq!(actual.framebuffer(), control.framebuffer());
    assert_eq!(
        actual.framebuffer_dimensions(),
        control.framebuffer_dimensions()
    );
    assert_eq!(actual.video_error(), control.video_error());
    let a = actual.encode_state().map_err(|error| error.to_string());
    let b = control.encode_state().map_err(|error| error.to_string());
    assert_eq!(a, b);
    hash.update(format!("{a:?}{:?}", actual.timing_snapshot()).as_bytes());
    let a = actual
        .encode_frontend_state()
        .map_err(|error| error.to_string());
    let b = control
        .encode_frontend_state()
        .map_err(|error| error.to_string());
    assert_eq!(a, b);
    match a {
        Ok(bytes) => {
            assert_eq!(&bytes[..8], b"ZMDFRONT");
            assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1);
            hash.update([1]);
            hash.update(bytes);
        }
        Err(error) => {
            hash.update([0]);
            hash.update(error.as_bytes());
        }
    }
    let a: Vec<_> = actual
        .drain_audio_samples()
        .into_iter()
        .map(f32::to_bits)
        .collect();
    let b: Vec<_> = control
        .drain_audio_samples()
        .into_iter()
        .map(f32::to_bits)
        .collect();
    assert_eq!(a, b);
    for bits in a {
        hash.update(bits.to_le_bytes());
    }
    assert_eq!(actual.take_vdp_capture(), control.take_vdp_capture());
    assert_eq!(actual, control);
}

pub(super) fn emit(label: &str, hash: Sha256, cases: usize) {
    let receipt: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{label} {receipt} cases={cases}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{label} {receipt} cases={cases}");
}
