#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub(crate) enum Metric {
    OwnersCreated,
    OwnersClosed,
    GenerationsCreated,
    Snapshots,
    DeepCopies,
    MutBorrows,
    SharedDetaches,
    WeakDissociations,
    CopyBytes,
    ImagesCreated,
    ImagesClosed,
    ImageClones,
    Attempts,
    Successes,
    Failures,
    Publications,
    RamReads,
    RamWrites,
    OtherReads,
    OtherWrites,
    ZeroWrites,
    OneWrite,
    TwoWrites,
    ManyWrites,
}

pub(super) const METRICS: usize = 24;
pub(super) const CAPACITY: usize = 1024;
pub(super) const DEPTH: usize = 16;
pub(super) const INVALID_SOURCE: u32 = 1;
pub(super) const EXHAUSTED: u32 = 2;
pub(super) const OVERFLOW: u32 = 4;
pub(super) const REENTRANT: u32 = 8;
#[cfg(any(test, feature = "ram-copy-census"))]
pub(super) const INCOMPLETE: u32 = 16;
pub(super) const BAD_ORDER: u32 = 32;

#[derive(Clone, Copy)]
pub(super) struct Owner {
    pub generation: u64,
}

#[derive(Clone, Copy)]
pub(super) struct Generation {
    pub address: usize,
    pub owners: u32,
    pub last_strong: usize,
    pub last_weak: usize,
}

#[derive(Clone, Copy)]
pub(super) struct Image {
    pub owner: u64,
}

#[derive(Clone, Copy)]
pub(super) struct Preparation {
    pub writes: u32,
}

#[cfg(any(test, feature = "ram-copy-census"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Report {
    pub scope: &'static str,
    pub full_coverage: bool,
    pub flags: u32,
    pub live_owners: usize,
    pub live_images: usize,
    pub counters: [u64; METRICS],
}

#[cfg(any(test, feature = "ram-copy-census"))]
impl Report {
    pub(crate) fn count(&self, metric: Metric) -> u64 {
        self.counters[metric as usize]
    }

    pub(crate) fn closed(&self) -> bool {
        self.flags == 0 && self.live_owners == 0 && self.live_images == 0
    }
}
