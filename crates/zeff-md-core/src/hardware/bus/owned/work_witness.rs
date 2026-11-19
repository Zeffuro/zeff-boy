use super::*;
use crate::hardware::vdp::owner::board::work_witness::Pulse;
use crate::hardware::vdp::owner::runtime::WorkCommit;

#[derive(Clone, Debug, Default)]
pub(super) struct Witness {
    pub(super) pulses: Vec<Pulse>,
    pub(super) companion_log: Option<Vec<Companion>>,
    pub(super) commits: Vec<WorkCommit>,
    pub(super) completion: Option<main_bus::Receipt>,
}

impl Witness {
    fn check_links(&self) {
        let mut commits = self.commits.iter();
        let mut previous_half = None;
        for pulse in &self.pulses {
            assert!(previous_half.is_none_or(|half| half < pulse.half));
            previous_half = Some(pulse.half);
            assert_eq!(pulse.pass, 1);
            assert!(pulse.row_strobe);
            assert!(!pulse.effects.is_empty());
            let word = pulse.address >> 1;
            assert_eq!(pulse.high, ((word & 0x7fff) * 2) as u16);
            assert_eq!(
                pulse.low,
                (((word & 0x5fff | if pulse.ram_address14 { 0x2000 } else { 0 }) ^ 0x2000) * 2 + 1)
                    as u16
            );
            let mut expected = Vec::new();
            if pulse.write_strobes[1] {
                expected.push((pulse.low, pulse.data as u8));
            }
            if pulse.write_strobes[0] {
                expected.push((pulse.high, (pulse.data >> 8) as u8));
            }
            assert_eq!(pulse.effects, expected);
            for (ordinal, effect) in pulse.effects.iter().enumerate() {
                let commit = commits
                    .next()
                    .expect("physical effect has chronological commit");
                assert_eq!(commit.half, pulse.half);
                assert_eq!(commit.ordinal, ordinal);
                assert_eq!((commit.address, commit.value), *effect);
            }
        }
        assert!(commits.next().is_none(), "commit has no physical pulse");
    }

    fn check_effects(pulse: &Pulse, access: BusAccess, value: u16) {
        let lanes = [
            access.lanes != ByteLanes::Lower,
            access.lanes != ByteLanes::Upper,
        ];
        assert_eq!(
            pulse.write_strobes, lanes,
            "unexpected or missing physical write lane"
        );
        assert!(!pulse.protocol && !pulse.protocol_drive);
        assert!(
            !pulse.cartridge_drive,
            "cartridge overwrote the selected data driver"
        );
        let mut expected = Vec::new();
        for lane in [1, 0] {
            if lanes[lane] {
                assert!(
                    !pulse.ram_read_lanes[lane],
                    "RAM OE overwrote the selected lane"
                );
                expected.push((
                    (access.bus_address as u16).wrapping_add(lane as u16),
                    value.to_be_bytes()[lane],
                ));
            }
        }
        assert_eq!(
            pulse.effects, expected,
            "wrong physical address/value or extra effect"
        );
    }

    fn check_main(pulse: &Pulse, completed: main_bus::Receipt, access: BusAccess, value: u16) {
        let driven = match access.lanes {
            ByteLanes::Upper => (value & 0xff00) | (value >> 8),
            ByteLanes::Lower => (value << 8) | (value & 0xff),
            ByteLanes::Both => value,
        };
        let pending = pulse
            .main
            .and_then(|main| main.pending)
            .expect("chronological MAIN pulse retains request identity");
        assert_eq!(pending.request, completed.request);
        assert_eq!(pending.start_half, completed.start_half);
        assert_eq!(pending.request.access, access);
        assert_eq!(
            pending.request.operation,
            main_bus::Operation::Write(driven)
        );
        assert!(pending.start_half <= pulse.half);
        assert!(pending.vpa.is_none());
        assert!(!pulse.released);
        assert!(
            pulse.main_address_selected && pulse.main_data_selected,
            "grant={}, sound_data={}, sound_low_address={}",
            pulse.grant_tristate,
            pulse.sound_data_drive,
            pulse.sound_low_address
        );
        assert_eq!(pulse.pins.address, Some(access.bus_address));
        assert_eq!(pulse.address, access.bus_address);
        assert!(!pulse.pins.read && pulse.pins.address_strobe);
        assert!(!pulse.pins.interrupt_acknowledge);
        assert_eq!(pulse.pins.write_lanes, Some(access.lanes));
        assert_eq!(pulse.pins.write_word, Some(driven));
        assert_eq!(
            [pulse.pins.upper_strobe, pulse.pins.lower_strobe],
            pulse.write_strobes
        );
        Self::check_effects(pulse, access, value);
    }

    fn check_main_suffix(&self, first_pulse: usize, access: BusAccess, value: u16, before: u16) {
        assert!(
            first_pulse < self.pulses.len(),
            "no chronological MAIN pulse"
        );
        let completed = self.completion.expect("actual completed MAIN transfer");
        let offset = self.pulses[..first_pulse]
            .iter()
            .map(|pulse| pulse.effects.len())
            .sum::<usize>();
        let mut commits = self.commits[offset..].iter();
        for (index, pulse) in self.pulses[first_pulse..].iter().enumerate() {
            Self::check_main(pulse, completed, access, value);
            for lane in [1, 0] {
                if pulse.write_strobes[lane] {
                    let commit = commits.next().unwrap();
                    let previous = if index == 0 { before } else { value };
                    assert_eq!(commit.before, previous.to_be_bytes()[lane]);
                }
            }
        }
        assert!(commits.next().is_none());
    }

    fn check_sound_prefix(&self, access: BusAccess, entry: u8) -> usize {
        let mut commits = self.commits.iter();
        let mut prefix = 0;
        for pulse in &self.pulses {
            if !pulse.released {
                break;
            }
            assert!(pulse.grant_tristate);
            assert!(!pulse.main_data_selected && !pulse.main_address_selected);
            assert!(pulse.companion.pins.shared() && pulse.companion.pins.write);
            assert_eq!(pulse.companion.main_address().unwrap(), access.bus_address);
            assert_eq!(pulse.companion.pins.data, Some(0x22));
            assert!(pulse.sound_data_drive);
            assert_eq!(pulse.address, access.bus_address);
            Self::check_effects(pulse, access, 0x2222);
            assert_eq!(pulse.effects.len(), 1);
            let commit = commits.next().unwrap();
            assert_eq!(commit.before, if prefix == 0 { entry } else { 0x22 });
            prefix += 1;
        }
        assert!(prefix > 0, "no chronological SOUND prefix");
        assert!(
            prefix < self.pulses.len(),
            "no MAIN pulse after SOUND prefix"
        );
        prefix
    }
}

impl<S: PhysicalCapture> Context<'_, S> {
    fn write_with_work_witness(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (CycleResult<(), BusError>, Witness) {
        assert!(self.work_witness.is_none());
        self.work_witness = Some(Witness::default());
        let cycle = self.write_cycle(access, value, start);
        let witness = self.work_witness.take().unwrap();
        (cycle, witness)
    }
}

mod tests;
