use super::*;
use crate::ea::Size;

#[derive(Clone, Copy)]
pub(super) struct ImmediateMoveProfile {
    destination: stores::StoreProfile,
}

impl ImmediateMoveProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        if opcode & 0x003f != 0x003c {
            return None;
        }
        Some(Self {
            destination: stores::StoreProfile::decode_destination(opcode)?,
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_immediate_move<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: ImmediateMoveProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let long = profile.destination.size == Size::Long;
        let phase = self.prefetch_phase(
            InstructionPhase::ExtensionFirst,
            pipeline.address.wrapping_add(4),
            pipeline.address.wrapping_add(if long { 2 } else { 4 }),
            pipeline.words[0],
        );
        let first = self.physical_read(bus, phase)?;
        let (value, retained, length) = if long {
            let phase = self.prefetch_phase(
                InstructionPhase::ExtensionSecond,
                pipeline.address.wrapping_add(6),
                pipeline.address.wrapping_add(6),
                pipeline.words[0],
            );
            let retained = self.physical_read(bus, phase)?;
            (
                (u32::from(pipeline.words[1]) << 16) | u32::from(first),
                retained,
                6,
            )
        } else {
            (
                profile
                    .destination
                    .size
                    .truncate(u32::from(pipeline.words[1])),
                first,
                4,
            )
        };
        // The final source read retains the destination extension or next opcode.
        let stream = stores::StoreStream {
            anchor: pipeline.address.wrapping_add(length),
            opcode: pipeline.words[0],
            retained,
        };
        self.physical_store_move(bus, stream, profile.destination, value)
    }
}
