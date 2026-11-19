mod address;
#[cfg(test)]
mod dma_activity_tests;
mod handshake;
mod input;
mod registers;
mod state;

pub(in crate::hardware::vdp) use address::AddressEvaluation;
use address::{Address, LoadPipeline};
use handshake::Handshake;
pub(in crate::hardware::vdp) use input::{
    FrontendFeedback, HandshakeFeedback, Intake, MainRequests, MemoryFeedback, PhaseLevels,
    PrimitiveInputs,
};
pub(in crate::hardware::vdp) use registers::{
    DmaBanks, RegisterBank, RegisterLoad, RegisterStage, RegisterViews,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Evaluation {
    pub intake: Intake,
    pub address: AddressEvaluation,
    pub register_load: RegisterLoad,
    pub changed_registers: u32,
    pub mode5_before: bool,
    pub dma_mode_before: u8,
    pub counter_cycle: bool,
    pub local_dma_service: bool,
    pub counter_terminal: bool,
    pub registers: RegisterViews,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct ControlCore {
    command: Handshake,
    address: Address,
    load: LoadPipeline,
    registers: RegisterBank,
    code: u8,
}

impl ControlCore {
    pub fn software_origin() -> Self {
        Self {
            command: Handshake::software_origin(),
            address: Address::software_origin(),
            load: LoadPipeline::software_origin(),
            registers: RegisterBank::software_origin(),
            code: 0,
        }
    }

    pub fn handshake(&self) -> HandshakeFeedback {
        self.command.feedback()
    }
    pub fn registers(&self) -> &RegisterBank {
        &self.registers
    }
    pub fn code(&self) -> u8 {
        self.code
    }
    pub fn evaluate(&mut self, input: PrimitiveInputs) -> Evaluation {
        let before_bank = self.registers;
        let before = before_bank.logical();
        let mode5_before = self.registers.mode5();
        let dma_mode_before = self.registers.dma().mode();
        let copy = input.memory.dma_active && dma_mode_before == 3;
        let copy_read = input.memory.read_selected && copy;
        let memory_cycle =
            input.memory.write_selected && !copy || copy_read || input.memory.fill_selected;
        let counter_cycle =
            copy_read || input.memory.fill_selected || input.memory.external_counter_cycle;
        let intake = self.command.evaluate(input, mode5_before, &mut self.code);
        let address = self.address.evaluate(
            input,
            intake,
            self.command,
            mode5_before,
            self.registers.increment(),
            memory_cycle,
        );
        let level = self.load.evaluate(input.phases, intake, self.command);
        let register_load =
            RegisterLoad::decode(level, self.address.slave, mode5_before, input.reset_comb);
        self.registers.evaluate(
            input.phases,
            register_load,
            counter_cycle,
            input.double_counter_test,
            input.reset_comb,
        );
        let after = self.registers.logical();
        let changed_registers = before
            .into_iter()
            .zip(after)
            .enumerate()
            .fold(0, |mask, (index, (old, new))| {
                mask | if old != new { 1 << index } else { 0 }
            });
        Evaluation {
            intake,
            address,
            register_load,
            changed_registers,
            mode5_before,
            dma_mode_before,
            counter_cycle,
            local_dma_service: !input.reset_comb
                && input.phases.horizontal_master
                && (copy_read || input.memory.fill_selected)
                && (before_bank.dma.length[0] != self.registers.dma.length[0]
                    || before_bank.dma.source[0] != self.registers.dma.source[0]),
            counter_terminal: counter_cycle && self.registers.dma().length_banks()[1] == 0xfffe,
            registers: RegisterViews::new(before_bank, self.registers, input.reset_comb),
        }
    }
}
