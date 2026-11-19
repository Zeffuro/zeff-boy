use super::*;

impl Emulator {
    /// Boundary restart preserves machine time, RAM and external interrupt inputs.
    pub fn reset_cpu(&mut self) -> Result<(), TimingFault<BusError>> {
        self.reset_main_cpu_with_capture(&mut NoSoundCapture)
    }

    pub(super) fn reset_main_cpu_with_capture<C: SoundCapture>(
        &mut self,
        capture: &mut C,
    ) -> Result<(), TimingFault<BusError>> {
        if self.bus.owned.is_some() {
            return Err(TimingFault::ResetOutputUnsupported);
        }
        if self
            .bus
            .cartridge
            .registers
            .as_ref()
            .is_some_and(|board| board.is_banked())
            || self.bus.tmss.is_some()
            || self.bus.input.uart_reset_blocked()
            || self.bus.cartridge.parallel_adapter.is_some()
            || self.bus.cartridge.sf004.is_some()
            || self.bus.cartridge.extended_ssf.is_some()
            || self.bus.cartridge.realtec.is_some()
            || self.bus.cartridge.download.is_some()
            || self.bus.cartridge.svp.is_some()
        {
            return Err(TimingFault::ResetOutputUnsupported);
        }
        if self
            .bus
            .cartridge
            .sf002
            .as_ref()
            .is_some_and(|board| board.validate_warm_reset().is_err())
        {
            return Err(TimingFault::ResetOutputUnsupported);
        }
        self.restart_cpu_captured(capture)
    }

    pub(super) fn restart_cpu(&mut self) -> Result<(), TimingFault<BusError>> {
        self.restart_cpu_captured(&mut NoSoundCapture)
    }

    pub(super) fn restart_cpu_captured<C: SoundCapture>(
        &mut self,
        capture: &mut C,
    ) -> Result<(), TimingFault<BusError>> {
        let checkpoint = capture.checkpoint();
        let result = self.restart_cpu_inner(capture);
        if result.is_err() {
            capture.rollback(checkpoint);
        }
        result
    }

    fn restart_cpu_inner<C: SoundCapture>(
        &mut self,
        capture: &mut C,
    ) -> Result<(), TimingFault<BusError>> {
        if self
            .bus
            .now
            .get()
            .checked_add(PRIME_CLOCKS * MAIN_CPU_DIVISOR)
            .is_none()
        {
            return Err(TimingFault::ClockOverflow);
        }
        // Priming may publish sound before a failed restart restores its checkpoint.
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        let mut cpu = PrefetchCpu::default();
        cpu.registers = self.bus.initial_registers();
        let clocks = (
            self.bus.now,
            self.bus.cpu_clock,
            self.bus.clock_fault,
            self.bus.prefetch,
            self.bus.rmw.clone(),
            self.bus.vdp.checkpoint(),
            self.bus.video.checkpoint(),
            self.bus.audio.checkpoint(),
        );
        let checkpoint = self.concurrent_sound.then(|| {
            let mut ram = [0; WORK_RAM_SIZE];
            ram.copy_from_slice(&self.bus.ram);
            (
                self.bus.sound.clone(),
                self.sound_cpu.clone(),
                self.sound_schedule.clone(),
                ram,
                self.bus.cartridge.registers.clone(),
                self.bus.cartridge.sf001.clone(),
                self.bus.cartridge.sf002,
                self.bus.cartridge.sf004.clone(),
                self.bus.cartridge.extended_ssf,
                self.bus.cartridge.save.clone(),
                self.bus.cartridge.parallel_adapter.clone(),
                self.bus.cartridge.download.clone(),
            )
        });
        let svp_checkpoint = self
            .bus
            .cartridge
            .svp
            .as_ref()
            .filter(|model| model.functional_timing().is_some())
            .cloned();
        let rmw_checkpoint = self.sound_schedule.rmw_checkpoint.clone();
        if let Some(scope) = self.bus.rmw.take()
            && let Some(reservation) = scope.reservation
        {
            self.sound_schedule
                .finish_rmw(&reservation, self.bus.now.get(), capture);
        }
        self.sound_schedule.rmw_checkpoint = None;
        capture.finish_rmw(false);
        self.bus.cpu_clock = CpuClocks(0);
        self.bus.clock_fault = false;
        let result = if self.concurrent_sound {
            cpu.prime(&mut ScheduledBus {
                capture: &mut *capture,
                bus: &mut self.bus,
                cpu: &mut self.sound_cpu,
                schedule: &mut self.sound_schedule,
            })
        } else {
            cpu.prime(&mut self.bus)
        };
        if let Err(fault) = result {
            let (now, clock, clock_fault, prefetch, rmw, vdp, video, audio) = clocks;
            if let Some(model) = svp_checkpoint {
                self.bus.cartridge.svp = Some(model);
            }
            self.bus.now = now;
            self.bus.cpu_clock = clock;
            self.bus.clock_fault = clock_fault;
            self.bus.prefetch = prefetch;
            self.bus.rmw = rmw;
            self.sound_schedule.rmw_checkpoint = rmw_checkpoint;
            self.bus.vdp.restore(vdp);
            self.bus.video.restore(video);
            self.bus.audio.restore(audio);
            if let Some((
                sound,
                sound_cpu,
                schedule,
                ram,
                registers,
                sf001,
                sf002,
                sf004,
                extended_ssf,
                save,
                parallel_adapter,
                download,
            )) = checkpoint
            {
                self.bus.sound = sound;
                self.sound_cpu = sound_cpu;
                self.sound_schedule = schedule;
                self.bus.ram.copy_from_slice(&ram);
                self.bus.cartridge.registers = registers;
                self.bus.cartridge.sf001 = sf001;
                self.bus.cartridge.sf002 = sf002;
                self.bus.cartridge.sf004 = sf004;
                self.bus.cartridge.extended_ssf = extended_ssf;
                self.bus.cartridge.save = save;
                self.bus.cartridge.parallel_adapter = parallel_adapter;
                self.bus.cartridge.download = download;
            }
            return Err(fault);
        }
        self.cpu = cpu;
        self.main_stopped = false;
        self.reset_call_stack();
        Ok(())
    }
}
