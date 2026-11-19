use super::{Emulator, MdRollbackFrame, MdRollbackSession, MdRollbackSnapshot};
use anyhow::{Result, ensure};
use std::marker::PhantomData;
use std::mem::{size_of, size_of_val};
use std::sync::{Arc, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum MdAllocationKind {
    CoreLayout,
    SessionLayout,
    SnapshotLayout,
    Rom,
    RomMemo,
    Header,
    HeaderText,
    RomOverlay,
    WorkRam,
    SoundRam,
    Vram,
    Framebuffer,
    AudioSamples,
    AudioTraceEvents,
    OwnedBus,
    AudioPending,
    FaultText,
    VdpRuntime,
    Surface,
    SurfacePixels,
    SurfaceSeen,
    SurfaceLastPixels,
    SoundSemantic,
    SoundEffects,
    SoundCheckpoint,
    CallStack,
    CallFrames,
    VdpEffects,
    FrameLayout,
    FrameAudio,
    OwnerToken,
}
const KINDS: usize = MdAllocationKind::OwnerToken as usize + 1;

// Layout/capacity payload units exclude Arc headers and allocator overhead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdRetainedPayload {
    requested_payload_bytes: u64,
    bytes: [u64; KINDS],
    counts: [u64; KINDS],
    arc_allocations: u64,
    collector_capacity_bytes: u64,
}
impl MdRetainedPayload {
    pub fn requested_payload_bytes(&self) -> u64 {
        self.requested_payload_bytes
    }
    pub fn bytes(&self, kind: MdAllocationKind) -> u64 {
        self.bytes[kind as usize]
    }
    pub fn count(&self, kind: MdAllocationKind) -> u64 {
        self.counts[kind as usize]
    }
    pub fn arc_allocations(&self) -> u64 {
        self.arc_allocations
    }
    pub fn collector_capacity_bytes(&self) -> u64 {
        self.collector_capacity_bytes
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    address: usize,
    kind: MdAllocationKind,
    bytes: usize,
}

pub struct MdRetainedAllocations<'a> {
    report: MdRetainedPayload,
    identities: Vec<Identity>,
    failed: bool,
    owners: PhantomData<&'a ()>,
}
impl Default for MdRetainedAllocations<'_> {
    fn default() -> Self {
        Self::new()
    }
}
impl<'a> MdRetainedAllocations<'a> {
    pub fn new() -> Self {
        Self {
            report: MdRetainedPayload {
                requested_payload_bytes: 0,
                bytes: [0; KINDS],
                counts: [0; KINDS],
                arc_allocations: 0,
                collector_capacity_bytes: 0,
            },
            identities: Vec::new(),
            failed: false,
            owners: PhantomData,
        }
    }

    fn checked(&mut self, value: Option<u64>) -> Result<u64> {
        match value {
            Some(value) => Ok(value),
            None => {
                self.failed = true;
                anyhow::bail!("MD retained payload arithmetic overflow")
            }
        }
    }

    pub(crate) fn payload(
        &mut self,
        kind: MdAllocationKind,
        elements: usize,
        width: usize,
    ) -> Result<()> {
        let elements = u64::try_from(elements).ok();
        let width = u64::try_from(width).ok();
        let bytes = self.checked(elements.zip(width).and_then(|(a, b)| a.checked_mul(b)))?;
        let index = kind as usize;
        let total = self.checked(self.report.requested_payload_bytes.checked_add(bytes))?;
        let kind_bytes = self.checked(self.report.bytes[index].checked_add(bytes))?;
        let count = self.checked(self.report.counts[index].checked_add(1))?;
        self.report.requested_payload_bytes = total;
        self.report.bytes[index] = kind_bytes;
        self.report.counts[index] = count;
        Ok(())
    }

    fn unique(&mut self, identity: Identity) -> Result<bool> {
        if self.identities.contains(&identity) {
            return Ok(false);
        }
        if let Err(error) = self.identities.try_reserve(1) {
            self.failed = true;
            return Err(error.into());
        }
        self.identities.push(identity);
        Ok(true)
    }

    pub(crate) fn layout<T>(&mut self, value: &'a T, kind: MdAllocationKind) -> Result<bool> {
        if !self.unique(Identity {
            address: value as *const T as usize,
            kind,
            bytes: size_of::<T>(),
        })? {
            return Ok(false);
        }
        self.payload(kind, 1, size_of::<T>())?;
        Ok(true)
    }

    pub(crate) fn boxed<T: ?Sized>(&mut self, value: &'a T, kind: MdAllocationKind) -> Result<()> {
        let bytes = size_of_val(value);
        if bytes == 0 {
            return Ok(());
        }
        self.payload(kind, 1, bytes)
    }

    pub(crate) fn vector<T>(&mut self, value: &'a Vec<T>, kind: MdAllocationKind) -> Result<()> {
        if value.capacity() == 0 || size_of::<T>() == 0 {
            return Ok(());
        }
        self.payload(kind, value.capacity(), size_of::<T>())
    }

    pub(crate) fn text(&mut self, value: &'a String, kind: MdAllocationKind) -> Result<()> {
        if value.capacity() == 0 {
            return Ok(());
        }
        self.payload(kind, value.capacity(), 1)
    }

    pub(crate) fn arc<T: ?Sized>(
        &mut self,
        value: &'a Arc<T>,
        kind: MdAllocationKind,
    ) -> Result<bool> {
        let bytes = size_of_val(value.as_ref());
        if !self.unique(Identity {
            address: Arc::as_ptr(value) as *const () as usize,
            kind,
            bytes,
        })? {
            return Ok(false);
        }
        let count = self.checked(self.report.arc_allocations.checked_add(1))?;
        self.payload(kind, 1, bytes)?;
        self.report.arc_allocations = count;
        Ok(true)
    }

    fn weak_owner(&mut self, value: &'a Weak<()>) -> Result<()> {
        if !self.unique(Identity {
            address: value.as_ptr() as usize,
            kind: MdAllocationKind::OwnerToken,
            bytes: 0,
        })? {
            return Ok(());
        }
        let count = self.checked(self.report.arc_allocations.checked_add(1))?;
        self.payload(MdAllocationKind::OwnerToken, 0, 1)?;
        self.report.arc_allocations = count;
        Ok(())
    }

    // Borrows keep every deduplicated allocation alive for the whole observation.
    pub fn visit_emulator(&mut self, core: &'a Emulator) -> Result<()> {
        let result = (|| {
            ensure!(
                !cfg!(any(
                    feature = "sound-shadow-observer",
                    feature = "first-failure-diagnostic"
                )),
                "MD payload accounting excludes instrumented builds"
            );
            ensure!(
                core.bus.cartridge.rollback_plain_factory() && core.bus.tmss.is_none(),
                "MD payload accounting requires a plain factory cartridge"
            );
            if !self.layout(core, MdAllocationKind::CoreLayout)? {
                return Ok(());
            }
            let Emulator {
                cpu: _,
                bus,
                sound_cpu: _,
                main_stopped: _,
                concurrent_sound: _,
                sound_schedule,
                call_stack,
            } = core;
            bus.visit_retained_allocations(self)?;
            sound_schedule.visit_retained_allocations(self)?;
            call_stack.visit_retained_allocations(self)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub fn finish(&self) -> Result<MdRetainedPayload> {
        ensure!(!self.failed, "MD retained payload observation failed");
        let mut report = self.report.clone();
        report.collector_capacity_bytes = u64::try_from(self.identities.capacity())?
            .checked_mul(u64::try_from(size_of::<Identity>())?)
            .ok_or_else(|| anyhow::anyhow!("MD collector capacity overflow"))?;
        Ok(report)
    }
}

impl MdRollbackSession {
    pub fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        let result = (|| {
            if !into.layout(self, MdAllocationKind::SessionLayout)? {
                return Ok(());
            }
            into.arc(&self.owner, MdAllocationKind::OwnerToken)?;
            into.visit_emulator(&self.baseline)?;
            into.visit_emulator(&self.current)?;
            if let Some(pending) = &self.pending {
                into.visit_emulator(&pending.candidate)?;
            }
            Ok(())
        })();
        if result.is_err() {
            into.failed = true;
        }
        result
    }
}
impl MdRollbackSnapshot {
    pub fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        let result = (|| {
            if !into.layout(self, MdAllocationKind::SnapshotLayout)? {
                return Ok(());
            }
            into.weak_owner(&self.owner)?;
            into.visit_emulator(&self.runtime)
        })();
        if result.is_err() {
            into.failed = true;
        }
        result
    }
}

impl MdRollbackFrame {
    pub fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        let result = (|| {
            if !into.layout(self, MdAllocationKind::FrameLayout)? {
                return Ok(());
            }
            let Self {
                field: _,
                master_tick: _,
                nominal_duration_ns: _,
                audio,
            } = self;
            into.vector(audio, MdAllocationKind::FrameAudio)
        })();
        if result.is_err() {
            into.failed = true;
        }
        result
    }
}
