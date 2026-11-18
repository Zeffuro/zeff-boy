mod checkpoint;

use super::*;
use crate::cartridge::SampledByte;
use zeff_m68k::timing::{InstructionByteOrigin, InstructionInput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ReadKey {
    request: main_bus::Request,
    start_half: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sample {
    key: ReadKey,
    input: InstructionInput,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MainReadOrigin {
    latch: Option<Sample>,
    sampled: Option<Sample>,
}

#[derive(Clone, Debug, Default)]
// Debug receipts do not participate in hardware equality.
pub(super) struct Metadata(pub(super) MainReadOrigin);

impl PartialEq for Metadata {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for Metadata {}

fn instruction_read(request: main_bus::Request) -> bool {
    request.operation == main_bus::Operation::Read
        && request.access.kind == zeff_m68k::bus::AccessKind::Prefetch
        && request.access.lanes == ByteLanes::Both
        && matches!(
            request.access.function_code,
            FunctionCode::SupervisorProgram | FunctionCode::UserProgram
        )
}

impl MainReadOrigin {
    pub(super) fn clear_latch(&mut self) {
        self.latch = None;
    }

    pub(super) fn clear_sample(&mut self) {
        self.sampled = None;
    }

    pub(super) fn drive(&mut self, key: Option<ReadKey>, bytes: [SampledByte; 2]) {
        self.latch = key.and_then(|key| {
            let high = bytes[0].origin.storage_offset()?;
            let low = bytes[1].origin.storage_offset()?;
            (high.checked_add(1) == Some(low)).then_some(Sample {
                key,
                input: InstructionInput {
                    value: u16::from_be_bytes(bytes.map(|byte| byte.value)),
                    origin: bytes.map(|byte| byte.origin),
                },
            })
        });
    }

    pub(super) fn freeze(
        &mut self,
        pending: Option<main_bus::Pending>,
        word: u16,
        mask: u16,
        plain: bool,
    ) {
        self.sampled = pending.and_then(|pending| {
            if !plain
                || !instruction_read(pending.request)
                || pending.vpa.is_some()
                || mask != 0xffff
            {
                return None;
            }
            let key = ReadKey {
                request: pending.request,
                start_half: pending.start_half,
            };
            self.latch
                .filter(|latch| latch.key == key && latch.input.value == word)
        });
    }

    pub(super) fn take(&mut self, receipt: main_bus::Receipt) -> [InstructionByteOrigin; 2] {
        self.sampled
            .take()
            .filter(|sample| {
                sample.key.request == receipt.request
                    && sample.key.start_half == receipt.start_half
                    && receipt.read_word == Some(sample.input.value)
                    && receipt.vpa.is_none()
            })
            .map_or([InstructionByteOrigin::UNKNOWN; 2], |sample| {
                sample.input.origin
            })
    }

    #[cfg(test)]
    pub(crate) fn latched_input(self) -> Option<InstructionInput> {
        self.latch.map(|sample| sample.input)
    }

    #[cfg(test)]
    pub(crate) fn sampled_input(self) -> Option<InstructionInput> {
        self.sampled.map(|sample| sample.input)
    }
}

impl Board {
    pub(super) fn cart_read_key(&self, stage: Stage, released: bool) -> Option<ReadKey> {
        let pending = self.main.as_ref()?.snapshot().pending?;
        let request = pending.request;
        (!released
            && self.protocol.is_none()
            && instruction_read(request)
            && pending.vpa.is_none()
            && request.access.bus_address <= 0x3ffffe
            && stage.pins.read
            && stage.pins.address_strobe
            && stage.pins.upper_strobe
            && stage.pins.lower_strobe
            && !stage.pins.interrupt_acknowledge
            && stage.pins.address == Some(request.access.bus_address)
            && self.address == request.access.bus_address)
            .then_some(ReadKey {
                request,
                start_half: pending.start_half,
            })
    }

    #[cfg(test)]
    pub(crate) fn sampled_read_debug(&self) -> (u16, MainReadOrigin) {
        (self.data, self.instruction_origin.0)
    }
}
