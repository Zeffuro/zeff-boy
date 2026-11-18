use super::*;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use zeff_emu_common::audio_trace::AudioTraceTiming;

#[derive(Clone)]
struct Rig {
    cpu: Cpu,
    memory: SoundMemory,
    schedule: SoundSchedule,
    cartridge: Cartridge,
    audio: Audio,
    ram: Vec<u8>,
}

impl Rig {
    fn new(cached: bool, fault: bool) -> Self {
        let mut rom = vec![0; 0x400];
        rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
        let mut program = vec![0; 8];
        if fault {
            program.extend([0x3a, 0x00, 0x7f]);
        } else {
            program.extend([
                0x3e, 0x73, 0x32, 0x00, 0x01, 0x32, 0x00, 0x80, 0x3a, 0x00, 0x80, 0x32, 0x11, 0x7f,
                0x76,
            ]);
        }
        let mut memory = SoundMemory::default();
        memory.set_reset(false);
        memory.bank = 0x1fe;
        memory.ram[..program.len()].copy_from_slice(&program);
        memory.ram[0x100] = 0xde;
        let mut schedule = SoundSchedule::default();
        if cached {
            schedule.begin_epoch_field();
        }
        let mut audio = Audio::default();
        for value in [0x84, 1, 0x90] {
            audio.write_psg(0, value);
        }
        audio.begin_batch();
        Self {
            cpu: Cpu::new(),
            memory,
            schedule,
            cartridge: Cartridge::new(rom).unwrap(),
            audio,
            ram: vec![0; WORK_RAM_SIZE],
        }
    }

    fn trace(&mut self) {
        self.audio.trace = self
            .audio
            .trace
            .prepare(
                64,
                crate::Region::NtscUs.master_hz() as u32,
                crate::hardware::audio::trace::trace_chip(crate::Region::NtscUs),
                AudioTraceTiming::CpuBusCycleBoundary,
            )
            .unwrap();
    }

    fn advance(&mut self, from: u64, to: u64, occupied: &[Reservation]) -> bool {
        let mut interval = SoundInterval::new(from..to, &mut self.audio);
        interval.occupied = occupied;
        self.schedule.advance_observed(
            &mut self.cpu,
            &mut self.memory,
            &mut self.cartridge,
            &mut self.ram,
            interval,
        )
    }

    fn canonical(&self) -> Self {
        let mut result = self.clone();
        result.schedule.finish_epoch_field(
            &mut result.cpu,
            &result.memory,
            &mut crate::sound_trace::NoSoundCapture,
        );
        if let Some(pending) = &mut result.schedule.pending
            && pending.bank_access.is_none()
            && !pending.any_bank_access
            && pending.replay.is_none()
        {
            // Epoch materialization snapshots current control flags; local publication ignores them.
            pending.memory.requested = result.memory.requested;
            pending.memory.granted = result.memory.granted;
            pending.memory.reset_asserted = result.memory.reset_asserted;
        }
        result
    }
}

fn equal(actual: &Rig, expected: &Rig) {
    let actual = actual.canonical();
    let expected = expected.canonical();
    assert_eq!(actual.cpu, expected.cpu);
    assert_eq!(actual.memory, expected.memory);
    assert_eq!(actual.schedule, expected.schedule);
    assert_eq!(actual.cartridge, expected.cartridge);
    assert_eq!(actual.ram, expected.ram);
    assert_eq!(actual.audio, expected.audio);
}

struct Fingerprint(Sha256);

impl std::fmt::Write for Fingerprint {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0.update(value.as_bytes());
        Ok(())
    }
}

impl Fingerprint {
    fn record(&mut self, rig: &Rig, local: bool) {
        // SoundSchedule's Debug omits the epoch; include its complete records separately.
        self.0.update(&rig.ram);
        write!(
            self,
            "{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{local};",
            rig.cpu, rig.memory, rig.schedule, rig.schedule.epoch, rig.cartridge, rig.audio
        )
        .unwrap();
    }
}

fn reservation(requester: Requester, begin: u64, end: u64) -> Reservation {
    Reservation {
        requester,
        ticks: begin..end,
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn mixed_reservation_guest_fences_and_future_state() {
    let empty = vec![];
    let non_vdp = vec![
        reservation(Requester::MAIN, 0, 0),
        reservation(Requester::SOUND, 50_000, 50_001),
        reservation(Requester::MAIN_RMW, 1, 1),
    ];
    let zero = reservation(Requester::VDP, 0, 0);
    let other = reservation(Requester::MAIN, 1, 1);
    let cases = [
        (false, empty),
        (false, non_vdp),
        (true, vec![zero.clone()]),
        (true, vec![zero.clone(), other.clone()]),
        (true, vec![other.clone(), zero.clone()]),
        (true, vec![zero.clone(), other, zero]),
        (true, vec![reservation(Requester::VDP, 50_000, 50_001)]),
        (
            true,
            vec![
                reservation(Requester::MAIN, 840, 900),
                reservation(Requester::VDP, 870, 1_000),
                reservation(Requester::MAIN_RMW, 990, 1_020),
            ],
        ),
    ];
    let mut fingerprint = Fingerprint(Sha256::new());
    let mut count = 0;
    for trace in [false, true] {
        for phase in 0..7 {
            for (case, (vdp, occupied)) in cases.iter().enumerate() {
                for cut in [59, 60, 61] {
                    let fault = phase == 3;
                    let mut actual = Rig::new(phase != 1, fault);
                    let mut expected = Rig::new(false, fault);
                    let begin = if phase == 0 {
                        0
                    } else if fault {
                        631
                    } else {
                        1
                    };
                    if begin != 0 {
                        actual.advance(0, begin, &[]);
                        expected.advance(0, begin, &[]);
                    }
                    if phase == 1 {
                        assert!(actual.schedule.pending.is_some());
                        actual.schedule.begin_epoch_field();
                    }
                    if phase == 2 {
                        assert!(actual.schedule.epoch_deadline().is_some());
                    }
                    if fault {
                        assert!(matches!(
                            actual.schedule.fault,
                            Some(SoundFault::UnsupportedAccess {
                                address: 0x7f00,
                                ..
                            })
                        ));
                    }
                    for rig in [&mut actual, &mut expected] {
                        if phase == 4 {
                            rig.memory.set_reset(true);
                        }
                        if phase == 5 {
                            rig.memory.request(true);
                        }
                        if phase == 6 {
                            rig.memory.irq_asserted = true;
                        }
                        if trace {
                            rig.trace();
                        }
                    }
                    let end = if fault { begin + cut } else { cut };
                    write!(&mut fingerprint, "{trace}:{phase}:{case}:{cut};").unwrap();
                    let a = actual.advance(begin, end, occupied);
                    let b = expected.advance(begin, end, occupied);
                    assert_eq!(a, b);
                    assert_eq!(a, !vdp && !fault);
                    equal(&actual, &expected);
                    fingerprint.record(&actual, a);
                    for rig in [&mut actual, &mut expected] {
                        rig.memory.set_reset(false);
                        rig.memory.request(false);
                        rig.memory.irq_asserted = false;
                    }
                    let mut from = end;
                    for to in [end + 1, 1_499, 1_500, 1_501, 30_501] {
                        let a = actual.advance(from, to, occupied);
                        let b = expected.advance(from, to, occupied);
                        assert_eq!(a, b);
                        equal(&actual, &expected);
                        fingerprint.record(&actual, a);
                        from = to;
                    }
                    if !fault {
                        assert_eq!(actual.memory.ram[0x100], 0x73);
                        assert_eq!(actual.ram[0], 0x73);
                        assert_eq!(actual.cpu.regs().a, SOUND_DISCONNECTED_BYTE);
                    }
                    if !fault {
                        let deadline = actual
                            .schedule
                            .epoch_deadline()
                            .or_else(|| actual.schedule.pending.as_ref().map(|p| p.deadline))
                            .unwrap();
                        let expected_deadline =
                            expected.schedule.pending.as_ref().unwrap().deadline;
                        assert_eq!(deadline, expected_deadline);
                        let a = actual.advance(from, deadline, occupied);
                        let b = expected.advance(from, deadline, occupied);
                        assert_eq!(a, b);
                        assert!(actual.schedule.pending.is_none());
                        assert!(actual.schedule.epoch_deadline().is_none());
                        assert!(expected.schedule.pending.is_none());
                        equal(&actual, &expected);
                        assert_eq!(actual.canonical().schedule, expected.canonical().schedule);
                        fingerprint.record(&actual, a);
                    }
                    actual.audio.finish_batch();
                    expected.audio.finish_batch();
                    let mut a = vec![];
                    let mut b = vec![];
                    actual.audio.drain_into(&mut a);
                    expected.audio.drain_into(&mut b);
                    let a: Vec<_> = a.into_iter().map(f32::to_bits).collect();
                    let b: Vec<_> = b.into_iter().map(f32::to_bits).collect();
                    assert_eq!(a, b);
                    assert!(a.iter().any(|v| v & 0x7fff_ffff != 0));
                    equal(&actual, &expected);
                    fingerprint.record(&actual, false);
                    write!(&mut fingerprint, "{a:?};").unwrap();
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 336);
    let hash: String = fingerprint
        .0
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("md-mixed-reservation-v2 {hash} cases={count}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("md-mixed-reservation-v2 {hash} cases={count}");
}
