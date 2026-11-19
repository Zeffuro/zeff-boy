use super::cpu_operation::OperationStep;
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimingModel {
    #[default]
    Standard,
    Precise,
}

impl Emulator {
    pub fn new_with_timing(
        rom: Vec<u8>,
        config: LoadConfig,
        timing: TimingModel,
    ) -> Result<Self, LoadError> {
        if timing == TimingModel::Standard {
            return Self::new(rom, config);
        }
        if !config.concurrent_sound {
            return Err(LoadError::TimingProfile(
                "precise timing requires concurrent SOUND".into(),
            ));
        }
        let cartridge = Cartridge::new(rom)?;
        Self::new_owned_with_cartridge(cartridge, config)
    }

    fn new_owned_with_cartridge(
        cartridge: Cartridge,
        config: LoadConfig,
    ) -> Result<Self, LoadError> {
        let bus = Bus::new_owned(cartridge, config.region)
            .map_err(|error| LoadError::TimingProfile(format!("{error:#}")))?;
        let mut machine = Self {
            cpu: PrefetchCpu::default(),
            bus,
            sound_cpu: zeff_z80::Cpu::new(),
            main_stopped: false,
            concurrent_sound: true,
            sound_schedule: SoundSchedule::default(),
            call_stack: call_stack::Tracker::default(),
        };
        machine.cpu.registers = machine.bus.cartridge.effective_initial();
        machine
            .cpu
            .prime(&mut machine.bus.owned_context().expect("fresh owned bus"))
            .map_err(|error| LoadError::TimingProfile(format!("{error:?}")))?;
        Ok(machine)
    }

    pub fn timing_model(&self) -> TimingModel {
        if self.bus.owned.is_some() {
            TimingModel::Precise
        } else {
            TimingModel::Standard
        }
    }

    pub fn vdp_registers(&self) -> [Option<u8>; 24] {
        match self
            .bus
            .vdp
            .owned_observation()
            .expect("validated VDP owner")
        {
            Some(view) => view.registers.map(Some),
            None => *self.bus.vdp.registers().registers(),
        }
    }

    pub(super) fn step_owned_boundary<P: crate::sound_trace::physical::PhysicalCapture>(
        &mut self,
        observer: &mut impl CpuOperation<Output = TimedOutcome>,
        physical: &mut P,
    ) -> Result<MachineStep, MachineFault> {
        let step = self.step_owned_operation_capturing(observer, physical)?;
        Ok(MachineStep {
            cpu: step.output,
            elapsed: step.elapsed,
        })
    }

    #[cfg(test)]
    pub(super) fn step_owned_operation<O: CpuOperation>(
        &mut self,
        operation: &mut O,
    ) -> Result<OperationStep<O::Output>, MachineFault> {
        self.step_owned_operation_capturing(operation, &mut ())
    }

    pub(super) fn step_owned_operation_capturing<
        O: CpuOperation,
        P: crate::sound_trace::physical::PhysicalCapture,
    >(
        &mut self,
        operation: &mut O,
        physical: &mut P,
    ) -> Result<OperationStep<O::Output>, MachineFault> {
        if let Some(fault) = self.input_fault() {
            return Err(MachineFault::Input(fault));
        }
        if self.bus.clock_fault {
            return Err(self.owned_clock_fault());
        }
        let trace_before = physical.position();
        self.begin_call_stack_operation();
        let result: Result<OperationStep<O::Output>, MachineFault> = (|| {
            let before = self.bus.now;
            if O::SAMPLE_IRQ_INPUT {
                self.cpu
                    .interrupt_input()
                    .set_level(self.bus.interrupt_level());
            }
            let output = {
                let mut bus = self
                    .bus
                    .owned_context_capturing(&mut *physical)
                    .expect("owned MAIN profile");
                let output = operation.step(&mut self.cpu, &mut bus)?;
                if O::stopped(&output) {
                    bus.wait_stopped()
                        .map_err(|error| MachineFault::PreciseTiming(format!("{error:#}")))?;
                }
                output
            };
            if self.bus.clock_fault {
                return Err(self.owned_clock_fault());
            }
            self.main_stopped = self.cpu.stopped();
            Ok(OperationStep {
                output,
                elapsed: self
                    .bus
                    .now
                    .checked_duration_since(before)
                    .expect("monotonic owned clock"),
            })
        })();
        self.finish_call_stack_operation(result.is_ok());
        if result.is_err() && physical.position() != trace_before {
            physical.fault();
        }
        result
    }

    pub(super) fn power_on_owned(&mut self) {
        let recording = self.call_stack_status() != CallStackStatus::Disabled;
        let mut input = self.bus.input.clone();
        input.power_on();
        let rate = self.bus.audio.sample_rate();
        let generation = self.bus.audio.sample_generation_enabled();
        let (psg_mutes, fm_mutes) = self.bus.audio.channel_mutes();
        let mut fresh = Self::new_owned_with_cartridge(
            self.bus.cartridge.clone(),
            LoadConfig {
                region: self.region(),
                concurrent_sound: true,
            },
        )
        .expect("validated precise cartridge restart");
        fresh.set_sample_rate(rate);
        fresh.set_apu_sample_generation_enabled(generation);
        fresh.set_apu_channel_mutes(psg_mutes);
        fresh.set_fm_channel_mutes(fm_mutes);
        fresh.bus.input = input;
        fresh.set_call_stack_recording(recording);
        *self = fresh;
    }

    fn owned_clock_fault(&self) -> MachineFault {
        self.bus.owned_fault().map_or_else(
            || TimingFault::ClockOverflow.into(),
            |fault| MachineFault::PreciseTiming(fault.to_owned()),
        )
    }
}
