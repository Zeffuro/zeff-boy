use super::domain::{BoardBoundary, Domain, Effect, Storage};
use super::internal_ram::Target;
use super::render::Renderer;
use crate::Region;
use crate::cartridge::SampledByte;
use crate::hardware::vdp::VdpMemory;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};
use zeff_emu_common::time::MasterTicks;

mod observation;
pub(crate) use observation::{OwnedGeometry, OwnedObservation, OwnedPixel};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Runtime {
    domain: Domain<Renderer>,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::hardware::vdp) enum MemoryWrite {
    Vram { address: u16, value: u8 },
    Cram { index: u8, mask: u16, value: u16 },
    Vsram { index: u8, mask: u16, value: u16 },
}

#[must_use]
pub(crate) struct PreparedHalf<'a, B> {
    commit: PreparedCommit<'a>,
    boundary: B,
}

impl<'a, B> PreparedHalf<'a, B> {
    pub(crate) fn into_parts(self) -> (PreparedCommit<'a>, B) {
        (self.commit, self.boundary)
    }
}

#[must_use]
pub(crate) struct PreparedCommit<'a> {
    runtime: &'a mut Runtime,
    dma_activity_epoch: &'a mut u64,
    dma_activity: bool,
    memory: &'a mut VdpMemory,
    work: &'a mut [u8],
    domain: Domain<Renderer>,
    writes: [Option<MemoryWrite>; 12],
    effects: Vec<Effect>,
    observation: OwnedObservation,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkCommit {
    pub half: u64,
    pub ordinal: usize,
    pub address: u16,
    pub before: u8,
    pub value: u8,
}

pub(crate) trait WorkWriteSink {
    fn before_write(&mut self, ordinal: usize, address: u16, value: u8, work: &[u8]);
}

impl WorkWriteSink for () {
    fn before_write(&mut self, _: usize, _: u16, _: u8, _: &[u8]) {}
}

impl PreparedCommit<'_> {
    pub(crate) fn commit(self) -> OwnedObservation {
        self.commit_capturing(&mut ())
    }

    pub(crate) fn commit_capturing<S: WorkWriteSink>(self, sink: &mut S) -> OwnedObservation {
        self.commit_recording(
            sink,
            #[cfg(test)]
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn commit_with_work_witness<S: WorkWriteSink>(
        self,
        half: u64,
        witness: &mut Vec<WorkCommit>,
        sink: &mut S,
    ) -> OwnedObservation {
        self.commit_recording(sink, Some((half, witness)))
    }

    fn commit_recording<S: WorkWriteSink>(
        self,
        sink: &mut S,
        #[cfg(test)] mut witness: Option<(u64, &mut Vec<WorkCommit>)>,
    ) -> OwnedObservation {
        self.memory.commit_owned_writes(&self.writes);
        let mut ordinal = 0;
        for effect in self.effects {
            if let Effect::WorkRam { address, value } = effect {
                #[cfg(test)]
                {
                    if let Some((half, writes)) = &mut witness {
                        writes.push(WorkCommit {
                            half: *half,
                            ordinal,
                            address,
                            before: self.work[usize::from(address)],
                            value,
                        });
                    }
                }
                sink.before_write(ordinal, address, value, self.work);
                self.work[usize::from(address)] = value;
                ordinal += 1;
            }
        }
        self.runtime.domain = self.domain;
        if self.dma_activity {
            *self.dma_activity_epoch = self.dma_activity_epoch.wrapping_add(1);
        }
        self.observation
    }
}

struct Memory<'a, F> {
    vdp: &'a VdpMemory,
    work: &'a [u8],
    main: F,
}

impl<F: Fn(u32) -> Result<SampledByte>> Storage for Memory<'_, F> {
    fn vram(&self, address: u16) -> Result<u8> {
        self.vdp
            .vram()
            .get(usize::from(address))
            .copied()
            .ok_or_else(|| anyhow::anyhow!("invalid physical VRAM address"))
    }
    fn cram(&self, index: u8) -> Result<u16> {
        self.vdp
            .cram_word(usize::from(index))
            .ok_or_else(|| anyhow::anyhow!("unknown physical CRAM word"))
    }
    fn vsram(&self, index: u8) -> Result<u16> {
        self.vdp
            .vsram()
            .get(usize::from(index))
            .copied()
            .ok_or_else(|| anyhow::anyhow!("invalid physical VSRAM index"))
    }
    fn main_byte(&self, address: u32) -> Result<u8> {
        self.main_byte_sampled(address).map(|sample| sample.value)
    }
    fn main_byte_sampled(&self, address: u32) -> Result<SampledByte> {
        ensure!(address <= 0xffffff, "invalid physical MAIN address");
        if address >= 0xe00000 {
            Ok(SampledByte::unknown(self.work[address as usize & 65535]))
        } else {
            (self.main)(address)
        }
    }
}

impl Runtime {
    pub(in crate::hardware::vdp) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut crate::emulator::rollback::MdRetainedAllocations<'a>,
    ) -> Result<()> {
        let Self { domain } = self;
        domain.visit_retained_allocations(into)
    }

    pub fn software_origin(region: Region) -> Self {
        Self {
            domain: Domain::software_origin(region == Region::Pal),
        }
    }
    pub fn half(&self) -> u64 {
        self.domain.half()
    }
    pub fn now(&self) -> MasterTicks {
        MasterTicks::new(self.half() / 2)
    }
    pub fn pal(&self) -> bool {
        self.domain.pal()
    }
    pub fn interrupt_level(&self) -> u8 {
        self.domain.interrupt_level()
    }
    pub fn hv_word(&self) -> u16 {
        self.domain.hv_word()
    }
    pub fn geometry(&self) -> OwnedGeometry {
        self.domain.geometry().into()
    }
    pub fn raster_field(&self) -> u64 {
        self.domain.raster_field()
    }
    pub fn observe(&self) -> Result<OwnedObservation> {
        self.domain.observe()
    }

    pub fn validate_state(&self, region: Region) -> Result<()> {
        ensure!(
            self.pal() == (region == Region::Pal),
            "physical VDP region differs"
        );
        self.domain.validate_state()?;
        ensure!(
            self.domain.pending().is_none(),
            "unretired physical VDP effects"
        );
        self.observe()?;
        Ok(())
    }
    pub fn write_state(&self, writer: &mut StateWriter) -> Result<()> {
        self.domain.write_state(writer)?;
        self.domain.write_render_tail(writer)
    }
    pub fn read_state(reader: &mut StateReader<'_>, region: Region) -> Result<Self> {
        let mut value = Self {
            domain: Domain::read_state(reader, region == Region::Pal)?,
        };
        value.domain.read_render_tail(reader)?;
        value.validate_state(region)?;
        Ok(value)
    }

    pub(in crate::hardware::vdp) fn prepare<'a, B: BoardBoundary>(
        &'a mut self,
        boundary: B,
        memory: &'a mut VdpMemory,
        work: &'a mut [u8],
        main: impl Fn(u32) -> Result<SampledByte>,
        dma_activity_epoch: &'a mut u64,
    ) -> Result<PreparedHalf<'a, B>> {
        ensure!(work.len() == 65536, "invalid physical work RAM size");
        let (mut domain, board, receipt) = {
            let storage = Memory {
                vdp: memory,
                work,
                main,
            };
            self.domain.clone().advance_half_owned(boundary, &storage)?
        };
        ensure!(
            receipt.effects.len() <= 12,
            "physical effect capacity exceeded"
        );
        let mut sequence = self.domain.internal_sequence();
        let mut writes = [None; 12];
        for (index, effect) in receipt.effects.iter().enumerate() {
            writes[index] = match effect {
                Effect::Vram(write) => Some(MemoryWrite::Vram {
                    address: write.canonical_address,
                    value: write.value,
                }),
                Effect::Internal(write) => {
                    sequence = sequence
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("physical RAM sequence overflow"))?;
                    ensure!(write.sequence == sequence, "stale physical RAM write");
                    Some(match write.target {
                        Target::Cram => MemoryWrite::Cram {
                            index: write.index,
                            mask: write.mask,
                            value: write.value,
                        },
                        Target::Vsram => MemoryWrite::Vsram {
                            index: write.index,
                            mask: write.mask,
                            value: write.value,
                        },
                    })
                }
                Effect::WorkRam { .. } => None,
            };
        }
        ensure!(
            sequence == domain.internal_sequence(),
            "physical RAM frontier differs"
        );
        memory.validate_owned_writes(&writes)?;
        let observation = domain.observe()?;
        domain.retire(&receipt)?;
        Ok(PreparedHalf {
            commit: PreparedCommit {
                runtime: self,
                dma_activity_epoch,
                dma_activity: receipt.dma_activity,
                memory,
                work,
                domain,
                writes,
                effects: receipt.effects,
                observation,
            },
            boundary: board,
        })
    }
}
