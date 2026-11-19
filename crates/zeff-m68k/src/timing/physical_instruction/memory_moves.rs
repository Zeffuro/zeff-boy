use super::*;
use memory_reads::MemoryReadProfile;
use stores::{StoreProfile, StoreStream};

#[derive(Clone, Copy)]
pub(super) struct MemoryMoveProfile {
    destination: StoreProfile,
    source: MemoryReadProfile,
}

impl MemoryMoveProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        let destination = StoreProfile::decode_destination(opcode)?;
        Some(Self {
            destination,
            source: MemoryReadProfile::decode(opcode, destination.size)?,
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_memory_move<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: MemoryMoveProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let operand = self.physical_memory_read(bus, pipeline, profile.source)?;
        let stream = StoreStream {
            anchor: operand.anchor,
            opcode: pipeline.words[0],
            retained: operand.retained,
        };
        self.physical_memory_store_move(bus, stream, profile.destination, operand.value)
    }
}
