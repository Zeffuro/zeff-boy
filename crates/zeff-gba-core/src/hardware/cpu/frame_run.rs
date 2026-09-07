use super::{Bus, Cpu, CpuState, FetchedInstruction};

impl Cpu {
    pub(crate) fn run_frame_service(
        &mut self,
        bus: &mut Bus,
        guard_cycles: u64,
        allow_direct: bool,
        allow_multiply: bool,
    ) -> Option<FetchedInstruction> {
        bus.begin_frame_cpu_run();
        #[cfg(feature = "profiling")]
        {
            self.profiling.frame_runs = self.profiling.frame_runs.wrapping_add(1);
        }
        loop {
            let (instruction, _count) = if let Some((instruction, count)) = allow_direct
                .then(|| {
                    self.run_frame_direct(
                        bus,
                        guard_cycles,
                        allow_multiply,
                        #[cfg(test)]
                        true,
                    )
                })
                .flatten()
            {
                (Some(instruction), count)
            } else {
                let instruction = self.step(bus);
                #[cfg(feature = "profiling")]
                if let Some(fetched) = instruction {
                    self.profile_frame_scalar_completion(fetched);
                }
                (instruction, 1)
            };
            #[cfg(feature = "profiling")]
            if instruction.is_some() {
                self.profiling.frame_run_instructions = self
                    .profiling
                    .frame_run_instructions
                    .wrapping_add(u64::from(_count));
            }
            if !bus.frame_cpu_run_can_continue()
                || self.state != CpuState::Running
                || self.cycles >= guard_cycles
            {
                return instruction;
            }
        }
    }
}
