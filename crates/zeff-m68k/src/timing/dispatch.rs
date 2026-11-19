use super::control::Completion;
use super::*;

impl PrefetchCpu {
    #[inline(always)]
    pub(super) fn dispatch_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<Completion, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let handled = match opcode >> 12 {
            0 => {
                if let Some(completion) = self.control(bus, pipeline)? {
                    return Ok(completion);
                }
                self.transfer_instruction(bus, pipeline)?
                    || self.immediate_instruction(bus, pipeline)?
                    || self.bit_instruction(bus, pipeline)?
                    || self.register_instruction(bus, pipeline)?
            }
            1..=3 => self.move_instruction(bus, pipeline)?,
            4 => {
                if let Some(completion) = self.control(bus, pipeline)? {
                    return Ok(completion);
                }
                if let Some(completion) = self.frame_instruction(bus, pipeline)? {
                    return Ok(completion);
                }
                if let Some(completion) = self.status_instruction(bus, pipeline)? {
                    return Ok(completion);
                }
                if let Some(completion) = self.math_instruction(bus, pipeline)? {
                    return Ok(completion);
                }
                self.control_address_instruction(bus, pipeline)?
                    || self.transfer_instruction(bus, pipeline)?
                    || self.tas_instruction(bus, pipeline)?
                    || self.unary_instruction(bus, pipeline)?
                    || self.paired_instruction(bus, pipeline)?
                    || self.register_instruction(bus, pipeline)?
            }
            5 => {
                self.branch_condition_instruction(bus, pipeline)?
                    || self.binary_instruction(bus, pipeline)?
                    || self.register_instruction(bus, pipeline)?
            }
            6 => self.branch_condition_instruction(bus, pipeline)?,
            8 | 12 => {
                if let Some(completion) = self.math_instruction(bus, pipeline)? {
                    return Ok(completion);
                }
                self.binary_instruction(bus, pipeline)?
                    || self.paired_instruction(bus, pipeline)?
                    || self.register_instruction(bus, pipeline)?
            }
            9 | 11 | 13 => {
                self.binary_instruction(bus, pipeline)?
                    || self.paired_instruction(bus, pipeline)?
                    || self.register_instruction(bus, pipeline)?
            }
            10 | 15 => {
                if let Some(completion) = self.control(bus, pipeline)? {
                    return Ok(completion);
                }
                false
            }
            14 => {
                self.memory_shift_instruction(bus, pipeline)?
                    || self.register_instruction(bus, pipeline)?
            }
            _ => false,
        };
        if handled {
            Ok(Completion {
                exception: None,
                trace: true,
            })
        } else {
            Err(TimingFault::UnsupportedInstruction {
                pc: pipeline.address,
                opcode,
            })
        }
    }
}
