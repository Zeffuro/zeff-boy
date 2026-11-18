use crate::hardware::vdp::owner::board::PsgWrite;
use std::collections::VecDeque;
use zeff_z80::RetirementEvidence;

pub(crate) mod producer;
pub(crate) mod retirement;
pub(crate) mod vdp;
pub(crate) const HALF_EVENTS: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Stamp {
    pub half: u64,
    pub pass: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Fetch,
    Read,
    Write,
    Internal,
    InterruptAck,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub address: u16,
    pub bank: u16,
    pub shape: Shape,
    pub external: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Occurrence {
    pub generation: u64,
    pub serial: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkTag {
    pub stamp: Stamp,
    pub ordinal: usize,
    pub lane: usize,
    pub logical: u16,
    pub bank: u16,
    pub address: u16,
    pub value: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClassifiedRetirement {
    pub complete: bool,
    pub consumed: Option<(u16, u8)>,
    pub cpu_before: u64,
    pub cpu_after: u64,
    pub input_lost: bool,
    pub kind: zeff_z80::RetirementKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CpuSnapshot {
    pub before: [u32; 14],
    pub after: [u32; 14],
    pub starts: bool,
}
impl CpuSnapshot {
    pub(crate) fn registers(cpu: &zeff_z80::Cpu) -> [u32; 14] {
        super::attempt::registers(cpu)
    }
    pub(crate) fn new(before: &zeff_z80::Cpu, after: &zeff_z80::Cpu, starts: bool) -> Self {
        Self {
            before: Self::registers(before),
            after: Self::registers(after),
            starts,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fact {
    Activated(Binding),
    Sampled(u8),
    Retired(RetirementEvidence),
    CpuSnapshot(CpuSnapshot),
    LocalWrite { address: u16, value: u8, tick: u64 },
    ExternalWriteStrobe { address: u16, bank: u16, value: u8 },
    ClassifiedRetired(ClassifiedRetirement),
    PsgAccepted(PsgWrite),
    VdpAccepted(vdp::Acceptance),
    WorkPulse(WorkTag),
    WorkCommitted { tag: WorkTag, before: u8 },
    Reset,
    Fault,
    Gap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Event {
    pub stamp: Stamp,
    pub occurrence: Option<Occurrence>,
    pub frame: Option<u64>,
    pub fact: Fact,
}
#[derive(Clone, Copy)]
struct Interval {
    retired: bool,
    id: Option<Occurrence>,
    binding: Binding,
    stamp: Stamp,
}
#[derive(Clone)]
struct State {
    vdp: vdp::Shadow,
    generation: u64,
    next: Option<u64>,
    partial: bool,
    recent: [Option<Interval>; 8],
    gaps: u64,
    last: Stamp,
}
impl Default for State {
    fn default() -> Self {
        Self {
            vdp: vdp::Shadow::default(),
            generation: 1,
            next: Some(1),
            partial: false,
            recent: [None; 8],
            gaps: 0,
            last: Stamp::default(),
        }
    }
}

pub(crate) trait HalfCapture: Clone + vdp::Capture {
    const ENABLED: bool;
    fn at(&mut self, stamp: Stamp);
    fn activated(&mut self, binding: Binding);
    fn sampled(&mut self, value: u8);
    fn retired(&mut self, evidence: RetirementEvidence);
    fn cpu_snapshot(&mut self, snapshot: CpuSnapshot);
    fn classified_retired(&mut self, evidence: ClassifiedRetirement);
    fn local_write(&mut self, address: u16, value: u8, tick: u64);
    fn external_write(&mut self, address: u16, bank: u16, value: u8);
    fn reset(&mut self);
    fn psg(&mut self, receipt: PsgWrite);
    fn work_tag(&mut self, tag: WorkTag);
    fn work_commit(&mut self, ordinal: usize, address: u16, value: u8, work: &[u8]);
}
impl HalfCapture for () {
    const ENABLED: bool = false;
    fn at(&mut self, _: Stamp) {}
    fn activated(&mut self, _: Binding) {}
    fn sampled(&mut self, _: u8) {}
    fn retired(&mut self, _: RetirementEvidence) {}
    fn cpu_snapshot(&mut self, _: CpuSnapshot) {}
    fn classified_retired(&mut self, _: ClassifiedRetirement) {}
    fn local_write(&mut self, _: u16, _: u8, _: u64) {}
    fn external_write(&mut self, _: u16, _: u16, _: u8) {}
    fn reset(&mut self) {}
    fn psg(&mut self, _: PsgWrite) {}
    fn work_tag(&mut self, _: WorkTag) {}
    fn work_commit(&mut self, _: usize, _: u16, _: u8, _: &[u8]) {}
}

pub(crate) trait PhysicalCapture {
    type Half: HalfCapture;
    fn begin_half(&self, half: u64) -> Self::Half;
    fn accept_half(&mut self, half: Self::Half);
    fn fault(&mut self);
    fn after_commit(&mut self, _: &zeff_z80::Cpu) {}
    fn position(&self) -> Option<Stamp> {
        None
    }
}
impl PhysicalCapture for () {
    type Half = ();
    fn begin_half(&self, _: u64) {}
    fn accept_half(&mut self, _: ()) {}
    fn fault(&mut self) {}
}
impl<C: PhysicalCapture> PhysicalCapture for &mut C {
    type Half = C::Half;
    fn begin_half(&self, half: u64) -> C::Half {
        (**self).begin_half(half)
    }
    fn accept_half(&mut self, half: C::Half) {
        (**self).accept_half(half);
    }
    fn fault(&mut self) {
        (**self).fault();
    }
    fn after_commit(&mut self, cpu: &zeff_z80::Cpu) {
        (**self).after_commit(cpu);
    }
    fn position(&self) -> Option<Stamp> {
        (**self).position()
    }
}

#[derive(Default)]
pub(crate) struct Journal {
    state: State,
    enabled: bool,
    events: VecDeque<Event>,
    #[cfg(test)]
    raw: Option<Vec<vdp::raw::Record>>,
}
pub(crate) struct Checkpoint {
    state: State,
    enabled: bool,
    events: VecDeque<Event>,
    #[cfg(test)]
    raw: Option<Vec<vdp::raw::Record>>,
}
impl Journal {
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.discontinuity();
            self.enabled = enabled;
        }
    }
    pub(crate) fn discontinuity(&mut self) {
        self.state.vdp = vdp::Shadow::default();
        self.state.recent = [None; 8];
        self.state.partial = false;
        if self.state.generation != 0 {
            self.state.generation = self.state.generation.checked_add(1).unwrap_or(0);
        }
        self.state.next = (self.state.generation != 0).then_some(1);
        self.state.gaps = self.state.gaps.saturating_add(1);
    }
    pub(crate) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            state: self.state.clone(),
            enabled: self.enabled,
            events: self.events.clone(),
            #[cfg(test)]
            raw: self.raw.clone(),
        }
    }
    pub(crate) fn restore(&mut self, checkpoint: Checkpoint) {
        self.state = checkpoint.state;
        self.enabled = checkpoint.enabled;
        self.events = checkpoint.events;
        #[cfg(test)]
        {
            self.raw = checkpoint.raw;
        }
    }
    #[cfg(test)]
    pub(crate) fn start_vdp_witness(&mut self) {
        self.raw = Some(Vec::new());
    }
    #[cfg(test)]
    pub(crate) fn vdp_witness(&self) -> &[vdp::raw::Record] {
        self.raw.as_deref().unwrap_or(&[])
    }
    #[cfg(test)]
    pub(crate) fn last_serial_next(&mut self) {
        self.state.next = Some(u64::MAX);
    }
    #[cfg(test)]
    pub(crate) fn events(&self) -> &VecDeque<Event> {
        &self.events
    }
    #[cfg(test)]
    pub(crate) fn gaps(&self) -> u64 {
        self.state.gaps
    }
    fn push(&mut self, event: Event) {
        if self.events.len() == 256 {
            self.events.pop_front();
            self.state.gaps = self.state.gaps.saturating_add(1);
        }
        self.events.push_back(event);
    }
}
#[derive(Clone)]
pub(crate) struct Half {
    state: State,
    enabled: bool,
    events: [Option<Event>; HALF_EVENTS],
    #[cfg(test)]
    raw: Option<Vec<vdp::raw::Record>>,
    count: usize,
    overflow: bool,
    tags: [Option<(WorkTag, Option<Occurrence>)>; 2],
}
impl Half {
    fn current(&self) -> Option<Occurrence> {
        self.state.recent[0].and_then(|a| a.id)
    }
    fn gap(&mut self) {
        self.state.gaps = self.state.gaps.saturating_add(1);
    }
    fn push_for(&mut self, fact: Fact, occurrence: Option<Occurrence>) {
        if !self.enabled {
            return;
        }
        let event = Event {
            stamp: self.state.last,
            occurrence,
            frame: None,
            fact,
        };
        if let Some(slot) = self.events.get_mut(self.count) {
            *slot = Some(event);
            self.count += 1;
        } else {
            self.overflow = true;
            self.state.vdp = vdp::Shadow::default();
            self.state.recent = [None; 8];
        }
    }
    fn push(&mut self, fact: Fact) {
        self.push_for(fact, self.current());
    }
    fn source(&mut self, stamp: Stamp, address: u16, bank: u16) -> Option<Occurrence> {
        let active = self
            .state
            .recent
            .iter()
            .flatten()
            .find(|a| a.stamp <= stamp);
        let id = active
            .filter(|a| {
                a.binding.shape == Shape::Write
                    && a.binding.address == address
                    && a.binding.bank == bank
            })
            .and_then(|a| a.id);
        if id.is_none() {
            self.gap();
        }
        id
    }
}
impl PhysicalCapture for Journal {
    type Half = Half;
    fn position(&self) -> Option<Stamp> {
        self.enabled.then_some(self.state.last)
    }
    fn begin_half(&self, half: u64) -> Half {
        let mut state = self.state.clone();
        state.last = Stamp { half, pass: 0 };
        Half {
            state,
            enabled: self.enabled,
            events: [None; HALF_EVENTS],
            #[cfg(test)]
            raw: self.raw.as_ref().map(|_| Vec::new()),
            count: 0,
            overflow: false,
            tags: [None; 2],
        }
    }
    fn accept_half(&mut self, half: Half) {
        self.state = half.state;
        #[cfg(test)]
        if let Some(raw) = &mut self.raw
            && let Some(staged) = half.raw
        {
            raw.extend(staged);
        }
        for event in half.events.into_iter().flatten() {
            self.push(event);
        }
        if half.overflow {
            self.state.gaps = self.state.gaps.saturating_add(1);
            self.push(Event {
                stamp: self.state.last,
                occurrence: None,
                frame: None,
                fact: Fact::Gap,
            });
        }
    }
    fn fault(&mut self) {
        if self.enabled
            && (self.state.partial || self.state.recent[0].is_some_and(|active| !active.retired))
        {
            self.push(Event {
                stamp: self.state.last,
                occurrence: self.state.recent[0].and_then(|a| a.id),
                frame: None,
                fact: Fact::Fault,
            });
            self.discontinuity();
        }
    }
}
impl HalfCapture for Half {
    const ENABLED: bool = true;
    fn at(&mut self, stamp: Stamp) {
        self.state.last = stamp;
    }
    fn activated(&mut self, binding: Binding) {
        if !self.enabled {
            return;
        }
        let id = self.state.next.map(|serial| Occurrence {
            generation: self.state.generation,
            serial,
        });
        self.state.next = self.state.next.and_then(|serial| serial.checked_add(1));
        if id.is_none() {
            self.gap();
        }
        self.state.recent.rotate_right(1);
        self.state.recent[0] = Some(Interval {
            retired: false,
            id,
            binding,
            stamp: self.state.last,
        });
        self.push(Fact::Activated(binding));
    }
    fn sampled(&mut self, value: u8) {
        if self.enabled {
            self.state.partial = true;
        }
        self.push(Fact::Sampled(value));
    }
    fn cpu_snapshot(&mut self, snapshot: CpuSnapshot) {
        self.push(Fact::CpuSnapshot(snapshot));
    }
    fn retired(&mut self, evidence: RetirementEvidence) {
        self.state.partial = !evidence.complete;
        if let Some(active) = &mut self.state.recent[0] {
            active.retired = true;
        }
        if evidence.input_overflowed {
            self.gap();
        }
        self.push(Fact::Retired(evidence));
    }
    fn classified_retired(&mut self, evidence: ClassifiedRetirement) {
        self.state.partial = !evidence.complete;
        if let Some(active) = &mut self.state.recent[0] {
            active.retired = true;
        }
        if evidence.input_lost {
            self.gap();
        }
        self.push(Fact::ClassifiedRetired(evidence));
    }
    fn local_write(&mut self, address: u16, value: u8, tick: u64) {
        self.push(Fact::LocalWrite {
            address,
            value,
            tick,
        });
    }
    fn external_write(&mut self, address: u16, bank: u16, value: u8) {
        self.push(Fact::ExternalWriteStrobe {
            address,
            bank,
            value,
        });
    }
    fn reset(&mut self) {
        self.push(Fact::Reset);
        self.state.vdp = vdp::Shadow::default();
        self.state.recent = [None; 8];
        self.state.partial = false;
    }
    fn psg(&mut self, receipt: PsgWrite) {
        if !self.enabled {
            return;
        }
        let b = receipt.binding;
        let occurrence = self.source(
            Stamp {
                half: b.half,
                pass: b.pass,
            },
            b.address,
            b.bank,
        );
        self.push_for(Fact::PsgAccepted(receipt), occurrence);
    }
    fn work_tag(&mut self, tag: WorkTag) {
        if !self.enabled {
            return;
        }
        let occurrence = self.source(tag.stamp, tag.logical, tag.bank);
        if let Some(slot) = self.tags.get_mut(tag.ordinal) {
            *slot = Some((tag, occurrence));
        } else {
            self.gap();
        }
        self.push_for(Fact::WorkPulse(tag), occurrence);
    }
    fn work_commit(&mut self, ordinal: usize, address: u16, value: u8, work: &[u8]) {
        let Some((tag, occurrence)) = self.tags.get(ordinal).copied().flatten() else {
            return;
        };
        if tag.stamp.half != self.state.last.half
            || tag.ordinal != ordinal
            || tag.address != address
            || tag.value != value
        {
            self.gap();
            return;
        }
        self.push_for(
            Fact::WorkCommitted {
                tag,
                before: work[usize::from(address)],
            },
            occurrence,
        );
    }
}

#[cfg(test)]
mod counter_tests;
