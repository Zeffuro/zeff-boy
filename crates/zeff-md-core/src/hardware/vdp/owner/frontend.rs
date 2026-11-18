mod dma;
mod fifo;
mod input;
mod port;
mod read;
mod state;

use crate::hardware::vdp::control_core::{
    ControlCore, Evaluation as ControlEvaluation, FrontendFeedback, HandshakeFeedback, Intake,
    MainRequests, MemoryFeedback, PhaseLevels, PrimitiveInputs, RegisterStage,
};
use anyhow::{Result, ensure};
use dma::Dma;
use fifo::Fifo;
pub(in crate::hardware::vdp) use input::{
    DmaView, ExternalFeedback, ExternalPins, Inputs, MemoryDrives, MemoryInputs, MemoryTarget,
    MemoryWrite, Output, WriteReceipt,
};
use port::Port;
use read::Read;

#[derive(Clone, Copy)]
struct Context {
    input: Inputs,
    control: ControlEvaluation,
    code: u8,
    dma: DmaView,
    old_read: bool,
    old_write: bool,
    old_selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Frontend {
    port: Port,
    dma: Dma,
    fifo: Fifo,
    read: Read,
    sequence: u64,
    launched: Option<WriteReceipt>,
}

impl Frontend {
    pub fn software_origin() -> Self {
        Self {
            port: Port::software_origin(),
            dma: Dma::software_origin(),
            fifo: Fifo::software_origin(),
            read: Read::software_origin(),
            sequence: 0,
            launched: None,
        }
    }

    pub fn external_feedback(&self) -> ExternalFeedback {
        ExternalFeedback {
            source_enabled: self.dma.source_enabled,
            bus_grant_acknowledge: self.dma.grant_acknowledge,
        }
    }

    pub fn fifo_full(&self, reset: bool) -> bool {
        !reset && self.fifo.equal() && (self.dma.produce[1] || self.dma.full[1])
    }

    pub fn read_word(&self) -> u16 {
        self.read.word()
    }

    pub fn bus_request(&self) -> bool {
        self.dma.request_delay.slave
    }

    pub fn dma_active(&self) -> bool {
        self.dma.active.slave
    }

    pub fn fifo_empty(&self) -> bool {
        self.dma.empty[1]
    }

    pub fn copy_read_address_selected(&self, dma: DmaView) -> bool {
        dma.copy && self.port.write_selected[1]
    }

    pub fn evaluate(&mut self, control: &mut ControlCore, input: Inputs) -> Result<Output> {
        let mut candidate = *self;
        let mut next_control = *control;
        let output = candidate.evaluate_inner(&mut next_control, input)?;
        *self = candidate;
        *control = next_control;
        Ok(output)
    }

    fn evaluate_inner(&mut self, control: &mut ControlCore, input: Inputs) -> Result<Output> {
        ensure!(
            input.memory.address <= 0x1ffff,
            "invalid VDP memory address input"
        );
        let before = control.registers().logical();
        let feedback = control.handshake();
        let equal = self.fifo.equal();
        let full = !input.reset_comb && equal && (self.dma.produce[1] || self.dma.full[1]);
        let terminal = self.dma.terminal[1];
        let old_read = self.port.read_selected[1];
        let old_write = self.port.write_selected[1];
        let old_selected = self.dma.selected[1];
        let old_ready = self.port.dma_ready;
        let old_read_ready = self.port.read_cycle_ready;
        let second_ready = self.port.byte_second_n;
        let dma = self.dma.early(
            input.phases,
            input.external,
            input.reset_comb,
            control.registers().dma().mode(),
        );
        let second_dma = input.main.write
            && second_ready
            && feedback.write_control_latched
            && feedback.command_second
            && before[1] & 4 != 0
            && input.main.data_bus & 128 != 0;
        self.dma.request(input, before, second_dma);
        if input.reset_comb || self.read.ready[1] {
            self.port.read_cycle_ready = true;
        } else if !feedback.read_control_latched && input.main.read {
            self.port.read_cycle_ready = false;
        }
        self.port.recover(feedback.recovery, input.reset_comb);
        let capture = self.port.pending && feedback.recovery;
        let evaluation = control.evaluate(PrimitiveInputs {
            phases: input.phases,
            main: input.main,
            frontend: FrontendFeedback {
                second_word_ready: second_ready,
                first_word_ready: self.port.byte_second_n,
                dma_bus_ready: old_ready,
                read_cycle_ready: old_read_ready,
                external_address_cycle: input.external_address_cycle,
                cpu_data_capture: capture,
            },
            memory: MemoryFeedback {
                read_selected: old_read,
                write_selected: old_write,
                fill_selected: self.dma.fill_selected[1],
                external_counter_cycle: self.dma.external_cycle,
                dma_active: dma.active,
            },
            reset_comb: input.reset_comb,
            double_counter_test: false,
        });
        Ok(self.finish(
            Context {
                input,
                control: evaluation,
                code: control.code(),
                dma,
                old_read,
                old_write,
                old_selected,
            },
            feedback,
            full,
            equal,
            terminal,
        ))
    }

    fn finish(
        &mut self,
        context: Context,
        feedback: HandshakeFeedback,
        full: bool,
        equal: bool,
        terminal: bool,
    ) -> Output {
        let Context {
            input,
            control,
            code,
            dma,
            old_read,
            old_write,
            ..
        } = context;
        let (capture, produce, read_ack) =
            self.port
                .intake(input, feedback, control.intake, self.read.ready[1]);
        let (read_cram, read_vsram) = self.port.select(context, self.dma.empty[1], terminal);
        self.port.kind(input.phases, read_cram, read_vsram);
        let registers = control.registers.at(RegisterStage::Control);
        if input.external.bus_grant || input.reset_comb {
            self.port.dma_ready = true;
        } else if control.register_load.includes(23)
            && control.register_load.value & 128 == 0
            && registers[1] & 4 != 0
        {
            self.port.dma_ready = false;
        }
        let external_capture = self.dma.service(
            input,
            dma,
            equal,
            produce,
            control.counter_terminal,
            self.fifo.consumer[1] & 1 != 0,
        );
        let fifo = self.fifo.evaluate(
            context,
            &self.dma,
            fifo::Transfer {
                capture,
                produced: produce,
                external_capture,
                new_write: self.port.write_selected[1],
                upper: self.port.upper,
                lower: self.port.lower,
                read_cram,
                read_vsram,
                read_kind_delay: self.port.read_kind_delay[1],
            },
        );
        let mut read_input = input;
        read_input.memory.address = fifo.read_address;
        self.read.evaluate(
            read_input,
            (!control.mode5_before || control.registers.before().logical()[1] & 128 == 0)
                && code & 12 == 0,
            old_write || !dma.copy && old_read,
        );
        let data_ack = !(produce || full) && control.intake.data_write;
        Output {
            control,
            dma,
            memory: fifo.drives,
            acknowledge: control.intake.control_ack || read_ack || data_ack,
            data_ack,
            read_ack,
            bus_load: fifo.bus_load,
            read_word: self.read.word(),
            fifo_full: full,
            fifo_empty: self.dma.empty[1],
            data_capture: capture,
            fifo_capture: fifo.capture,
            dma_terminal: self.dma.terminal[1],
            dma_activity: control.local_dma_service || fifo.dma_activity,
        }
    }

    pub fn launched_write(&self) -> Option<WriteReceipt> {
        self.launched
    }
}
