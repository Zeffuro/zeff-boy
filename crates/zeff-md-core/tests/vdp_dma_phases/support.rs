pub(crate) use crate::vdp_local_dma::support as base;
pub use base::{Operation, command, counters, image, word};
use zeff_md_core::{
    ControlCommand, VdpDmaCopy, VdpDmaError, VdpDmaRegisters, VdpDmaWrite, VdpMemory,
};

pub type Read<'a> = dyn FnMut(
        &VdpDmaRegisters,
        &ControlCommand,
        u8,
        &VdpMemory,
    ) -> Result<Option<VdpDmaCopy>, VdpDmaError>
    + 'a;
pub type Write<'a> = dyn FnMut(
        &mut VdpDmaRegisters,
        &mut ControlCommand,
        u8,
        VdpDmaCopy,
        &mut VdpMemory,
    ) -> Result<VdpDmaWrite, VdpDmaError>
    + 'a;
pub type Batch<'a> = dyn FnMut(
        &mut VdpDmaRegisters,
        &mut ControlCommand,
        u8,
        Operation,
        u32,
        &mut VdpMemory,
    ) -> Result<u32, VdpDmaError>
    + 'a;

pub fn batch(
    dma: &mut VdpDmaRegisters,
    command: &mut ControlCommand,
    increment: u8,
    operation: Operation,
    count: u32,
    memory: &mut VdpMemory,
) -> Result<u32, VdpDmaError> {
    match operation {
        Operation::Fill(data) => dma.fill_bytes(command, increment, data, count, memory),
        Operation::Copy => dma.copy_bytes(command, increment, count, memory),
    }
}
