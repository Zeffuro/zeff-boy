use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};
mod cases;
mod original;
mod shifted;

#[derive(Default)]
struct Report {
    comparisons: u64,
    writes: u64,
    slots: u64,
    codecs: u64,
    cases: u64,
    digest: u64,
}

impl Report {
    fn finish(self) -> u64 {
        assert!(self.comparisons > self.slots);
        assert!(self.writes > 0 && self.slots > 0);
        self.digest
            .wrapping_add(self.comparisons.rotate_left(7))
            .wrapping_add(self.writes.rotate_left(19))
            .wrapping_add(self.slots.rotate_left(31))
            .wrapping_add(self.codecs.rotate_left(43))
            .wrapping_add(self.cases.rotate_left(53))
    }
}

struct Pair<'a> {
    actual: Opn2,
    original: Opn2,
    report: &'a mut Report,
}

impl<'a> Pair<'a> {
    fn new(chip: Opn2, report: &'a mut Report) -> Self {
        report.cases += 1;
        let mut pair = Self {
            actual: chip,
            original: chip,
            report,
        };
        pair.check();
        pair
    }

    fn check(&mut self) {
        self.report.comparisons += 1;
        assert_eq!(
            self.actual, self.original,
            "FM geometry comparison {}",
            self.report.comparisons
        );
        let sample = self.actual.sample_with_mutes([false; CHANNELS]);
        assert_eq!(sample, self.original.sample_with_mutes([false; CHANNELS]));
        self.report.digest = self.report.digest.rotate_left(9).wrapping_add(
            (self.actual.scheduler.step as u64)
                ^ (u64::from(self.actual.flags) << 32)
                ^ (sample[0] as u64).rotate_left(17)
                ^ (sample[1] as u64).rotate_left(39),
        );
    }

    fn port(&mut self, port: u8, value: u8) {
        self.actual.write_port(port, value);
        self.original.write_port(port, value);
        self.report.writes += 1;
        self.check();
    }

    fn write(&mut self, bank: u8, address: u8, value: u8) {
        self.port(bank, address);
        self.port(bank + 1, value);
    }

    fn advance(&mut self, clocks: u64) {
        assert!(clocks <= SLOT_CLOCKS);
        let total = clocks / SLOT_CLOCKS
            + (clocks % SLOT_CLOCKS + self.original.phase % SLOT_CLOCKS) / SLOT_CLOCKS;
        self.actual.advance(clocks);
        self.original.busy = self.original.busy.saturating_sub(clocks);
        self.original.phase = (clocks % FM_CLOCK_DIVIDER + self.original.phase) % FM_CLOCK_DIVIDER;
        for _ in 0..total {
            self.original.clock_slot_original();
            self.report.slots += 1;
        }
        self.check();
    }

    fn slots(&mut self, slots: u64) {
        for _ in 0..slots {
            self.advance(SLOT_CLOCKS);
        }
    }

    fn bulk(&mut self, clocks: u64) {
        let mut whole = self.actual;
        whole.advance(clocks);
        for _ in 0..clocks / SLOT_CLOCKS {
            self.advance(SLOT_CLOCKS);
        }
        self.advance(clocks % SLOT_CLOCKS);
        assert_eq!(whole, self.actual);
    }

    fn codec(&mut self) {
        let encode = |chip: &Opn2| {
            let mut writer = StateWriter::new();
            chip.write_state(&mut writer);
            writer.into_bytes()
        };
        let actual = encode(&self.actual);
        let original = encode(&self.original);
        assert_eq!(actual, original);
        let decode = |bytes: &[u8]| {
            let mut reader = StateReader::new(bytes);
            let chip = Opn2::read_state(&mut reader).unwrap();
            assert!(reader.is_exhausted());
            chip
        };
        let decoded = decode(&actual);
        {
            assert_eq!(decoded, self.actual);
        }
        self.actual = decoded;
        self.original = decode(&original);
        self.report.codecs += 1;
        self.check();
    }
}

fn encoded_channel(channel: usize) -> u8 {
    (channel % CHANNELS_PER_BANK + channel / CHANNELS_PER_BANK * 4) as u8
}

fn voice_register(channel: usize, physical: usize, register: u8) -> (u8, u8) {
    (
        (channel / CHANNELS_PER_BANK * 2) as u8,
        register + (physical * 4 + channel % CHANNELS_PER_BANK) as u8,
    )
}

fn configured(report: &mut Report, algorithm: u8, feedback: u8, mode: u8, flavor: u8) -> Opn2 {
    let mut pair = Pair::new(Opn2::default(), report);
    pair.write(0, 0x22, if flavor == 0 { 0 } else { 8 | (flavor - 1) & 7 });
    pair.write(0, 0x24, 0xff);
    pair.write(0, 0x25, 3);
    pair.write(0, 0x26, 0xff);
    pair.write(0, 0x27, mode | 15);
    for channel in 0..CHANNELS {
        let (bank, local) = voice_register(channel, 0, 0);
        pair.write(bank, 0xa4 + local, ((channel as u8 + flavor) & 7) << 3 | 7);
        pair.write(bank, 0xa0 + local, [0, 1, 0x7f, 0xff][channel & 3]);
        pair.write(bank, 0xb0 + local, algorithm | feedback << 3);
        pair.write(
            bank,
            0xb4 + local,
            0xc0 | ((channel as u8 & 3) << 4) | (flavor & 7),
        );
        for physical in 0..constants::OPERATORS {
            let extreme = (flavor as usize + channel + physical) & 3;
            let values = [
                (((flavor as usize + physical) & 7) as u8) << 4 | [0, 1, 8, 15][extreme],
                [0, 1, 0x7e, 0x7f][extreme],
                [0, 1, 0x5e, 0xdf][extreme],
                [0, 0x81, 0x9e, 0x9f][extreme],
                [0, 1, 30, 31][extreme],
                [0, 1, 0xef, 0xff][extreme],
                (flavor + channel as u8 + physical as u8) & 15,
            ];
            for (index, value) in values.into_iter().enumerate() {
                let (bank, address) = voice_register(channel, physical, 0x30 + index as u8 * 0x10);
                pair.write(bank, address, value);
            }
        }
        pair.write(0, 0x28, encoded_channel(channel) | 0xf0);
        pair.slots(1);
    }
    for (high, low, value) in [(0xac, 0xa8, 0x0f), (0xad, 0xa9, 0x27), (0xae, 0xaa, 0x3f)] {
        pair.write(0, high, value);
        pair.write(0, low, 0xff);
    }
    pair.slots(144 - CHANNELS as u64);
    pair.actual
}

pub(super) fn run_all() -> u64 {
    let mut report = Report::default();
    shifted::small(&mut report);
    cases::alignment(&mut report);
    cases::algorithms(&mut report);
    cases::boundaries(&mut report);
    cases::commands(&mut report);
    cases::quiet_and_chunks(&mut report);
    cases::codecs(&mut report);
    report.finish()
}

#[test]
fn fm_geometry_small_original_slot_differential() {
    let mut report = Report::default();
    let base = configured(&mut report, 7, 7, 0x80, 15);
    for cycle in 0..SLOTS {
        for fraction in 0..SLOT_CLOCKS {
            let mut pair = Pair::new(base, &mut report);
            pair.bulk(cycle as u64 * SLOT_CLOCKS + fraction);
            pair.write(0, 0x28, 0xf2);
            pair.write(0, 0x27, 0xcf);
            pair.slots(2 * SLOTS as u64 + 7);
            pair.codec();
            pair.slots(SLOTS as u64);
        }
    }
    assert_ne!(report.finish(), 0);
}

#[test]
fn fm_geometry_all_original_slot_differential() {
    assert_ne!(run_all(), 0);
}
