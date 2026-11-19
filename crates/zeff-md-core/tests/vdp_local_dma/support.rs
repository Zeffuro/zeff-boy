use zeff_emu_common::time::MasterTicks;
use zeff_md_core::{
    ControlCommand, VdpDmaError, VdpDmaRegisters, VdpDmaWrite, VdpMemory, VdpRegisterPipeline,
};

#[derive(Clone, Copy)]
pub enum Operation {
    Fill(u16),
    Copy,
}

pub type Step<'a> = dyn FnMut(
        &mut VdpDmaRegisters,
        &mut ControlCommand,
        u8,
        &mut VdpMemory,
        Operation,
    ) -> Result<Option<VdpDmaWrite>, VdpDmaError>
    + 'a;

pub fn transfer(
    dma: &mut VdpDmaRegisters,
    command: &mut ControlCommand,
    increment: u8,
    memory: &mut VdpMemory,
    operation: Operation,
) -> Result<Option<VdpDmaWrite>, VdpDmaError> {
    match operation {
        Operation::Fill(data) => dma.fill_byte(command, increment, data, memory),
        Operation::Copy => dma.copy_byte(command, increment, memory),
    }
}

pub fn registers(length: u16, source: u16, mode: u8) -> VdpRegisterPipeline {
    let mut registers = VdpRegisterPipeline::at_h32_edge(tick(0));
    let [length_low, length_high] = length.to_le_bytes();
    let [source_low, source_high] = source.to_le_bytes();
    for (index, value) in [length_low, length_high, source_low, source_high, mode]
        .into_iter()
        .enumerate()
    {
        publish(&mut registers, 19 + index as u8, value);
    }
    registers
}

pub fn image(length: u16, source: u16, mode: u8) -> VdpDmaRegisters {
    VdpDmaRegisters::from_registers(&registers(length, source, mode)).unwrap()
}

pub fn publish(registers: &mut VdpRegisterPipeline, index: u8, value: u8) {
    registers
        .advance_to(tick(registers.now().get() + 1))
        .unwrap();
    let write = registers.queue_register(index, value).unwrap();
    registers.advance_to(write.publish_at).unwrap();
}

pub fn command(address: u32, code: u8) -> ControlCommand {
    ControlCommand { address, code }
}

pub fn word(memory: &mut VdpMemory, address: u32, data: u16) {
    memory.write_word(command(address, 1), data).unwrap();
}

pub fn tick(value: u64) -> MasterTicks {
    MasterTicks::new(value)
}

pub fn counters(dma: &VdpDmaRegisters, length: u16, source: u16, mode: u8) {
    assert_eq!(dma.length(), length);
    assert_eq!(dma.source(), source);
    assert_eq!(dma.source_high(), mode);
}
