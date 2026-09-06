use super::super::cdrom2::{CDROM2_REGISTER_END, CDROM2_REGISTER_START};
use super::super::vce::VcePixelClock;
#[cfg(feature = "profiling")]
use super::super::vdc_horizontal::VdcHorizontalAdvance;
#[cfg(feature = "profiling")]
use super::super::vdc_scanline::VdcActiveDisplayLine;
use super::*;
#[cfg(feature = "profiling")]
use crate::hardware::profiling::{PceBusAccessKind, PceDeviceMaterializationCause, PceProfiling};

pub(super) struct TimedMachineBus<'a> {
    inner: &'a mut BaseBus<PceDevices>,
    front_video: &'a mut PceActiveOnlyVideoFrame,
    back_video: &'a mut PceActiveOnlyVideoFrame,
    vce_line_accumulator: &'a mut u64,
    vdc_pixel_clock_remainder: &'a mut u8,
    vce_line_index: &'a mut u16,
    vce_frame_length: &'a mut VceFrameLength,
    master_ticks_per_cycle: u64,
    observed_cycles: u32,
    pub(super) video_wait_cycles: u32,
    pub(super) vram_contention_wait_cycles: u32,
    pub(super) elapsed_master_ticks: u64,
    unclaimed_on_chip_master_ticks: u64,
    pending_device_master_ticks: u64,
    pub(super) vce_lines: u64,
    pub(super) frames_published: u64,
    pub(super) fault: Option<PceMachineError>,
    pending_debug_write: Option<(u32, u8)>,
    trace: Option<&'a mut TimedInstructionTrace>,
    trace_enabled: bool,
    capture_old_writes: bool,
    pub(super) dma_completed: bool,
    debug: &'a mut AddressDebugController,
    #[cfg(test)]
    coalesce_device_advancement: bool,
    #[cfg(test)]
    fault_after_device_chunks: Option<u64>,
    #[cfg(test)]
    device_advance_calls: u64,
    #[cfg(feature = "profiling")]
    profiling: &'a mut PceProfiling,
}

#[derive(Debug)]
pub(super) struct TimedInstructionTrace {
    pub(super) instruction_bytes: [u8; MAX_TRACE_INSTRUCTION_BYTES],
    pub(super) instruction_byte_len: u8,
    pub(super) trace_writes: [TraceWrite; MAX_TRACE_WRITES],
    pub(super) trace_write_len: u8,
    pub(super) trace_write_overflow: u16,
}

impl Default for TimedInstructionTrace {
    fn default() -> Self {
        Self {
            instruction_bytes: [0; MAX_TRACE_INSTRUCTION_BYTES],
            instruction_byte_len: 0,
            trace_writes: [TraceWrite::default(); MAX_TRACE_WRITES],
            trace_write_len: 0,
            trace_write_overflow: 0,
        }
    }
}

impl TimedInstructionTrace {
    pub(super) fn clear(&mut self) {
        self.instruction_byte_len = 0;
        self.trace_write_len = 0;
        self.trace_write_overflow = 0;
    }
}

impl<'a> TimedMachineBus<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        inner: &'a mut BaseBus<PceDevices>,
        front_video: &'a mut PceActiveOnlyVideoFrame,
        back_video: &'a mut PceActiveOnlyVideoFrame,
        vce_line_accumulator: &'a mut u64,
        vdc_pixel_clock_remainder: &'a mut u8,
        vce_line_index: &'a mut u16,
        vce_frame_length: &'a mut VceFrameLength,
        master_ticks_per_cycle: u64,
        trace: Option<&'a mut TimedInstructionTrace>,
        debug: &'a mut AddressDebugController,
        #[cfg(test)] coalesce_device_advancement: bool,
        #[cfg(feature = "profiling")] profiling: &'a mut PceProfiling,
    ) -> Self {
        let trace_enabled = trace.is_some();
        let capture_old_writes = trace_enabled
            || debug.watchpoints.iter().any(|watchpoint| {
                matches!(
                    watchpoint.watch_type,
                    WatchType::Write | WatchType::ReadWrite
                )
            });
        Self {
            inner,
            front_video,
            back_video,
            vce_line_accumulator,
            vdc_pixel_clock_remainder,
            vce_line_index,
            vce_frame_length,
            master_ticks_per_cycle,
            observed_cycles: 0,
            video_wait_cycles: 0,
            vram_contention_wait_cycles: 0,
            elapsed_master_ticks: 0,
            unclaimed_on_chip_master_ticks: 0,
            pending_device_master_ticks: 0,
            vce_lines: 0,
            frames_published: 0,
            fault: None,
            pending_debug_write: None,
            trace,
            trace_enabled,
            capture_old_writes,
            dma_completed: false,
            debug,
            #[cfg(test)]
            coalesce_device_advancement,
            #[cfg(test)]
            fault_after_device_chunks: None,
            #[cfg(test)]
            device_advance_calls: 0,
            #[cfg(feature = "profiling")]
            profiling,
        }
    }

    fn advance_cycle(&mut self) {
        self.observed_cycles += 1;
        self.advance_elapsed_master_ticks(self.master_ticks_per_cycle);
        let horizon = self.next_fallible_device_horizon();
        if self.pending_device_master_ticks >= horizon {
            self.materialize_pending_devices(
                #[cfg(feature = "profiling")]
                if horizon == 0 {
                    PceDeviceMaterializationCause::DmaHorizon
                } else {
                    PceDeviceMaterializationCause::LineHorizon
                },
            );
        }
    }

    fn advance_access(&mut self, physical_addr: u32, write: bool) -> bool {
        self.pending_debug_write = None;
        self.advance_cycle();
        if is_timing_observable_access(self.inner, physical_addr) {
            self.materialize_pending_devices(
                #[cfg(feature = "profiling")]
                PceDeviceMaterializationCause::TimingMmio,
            );
        }
        let video_access = is_vdc_vce_access(physical_addr);
        if video_access {
            self.advance_elapsed_master_ticks(
                u64::from(PCE_VDC_VCE_ACCESS_WAIT_CYCLES) * self.master_ticks_per_cycle,
            );
            self.materialize_pending_devices(
                #[cfg(feature = "profiling")]
                PceDeviceMaterializationCause::DirectVdc,
            );
            self.video_wait_cycles += PCE_VDC_VCE_ACCESS_WAIT_CYCLES;
            if let Some(target) = vdc_vram_cycle_target(self.inner, physical_addr, write) {
                self.wait_for_vdc_dma(target);
            }
        }
        let completed = self.fault.is_none();
        if completed && write && self.capture_old_writes {
            self.pending_debug_write = Some((physical_addr, self.inner.peek(physical_addr)));
        }
        completed
    }

    fn advance_plain_memory_access(&mut self) -> bool {
        self.pending_debug_write = None;
        self.advance_cycle();
        self.fault.is_none()
    }

    #[cfg(feature = "profiling")]
    fn record_direct_plain_memory_access(&mut self, physical_addr: u32, kind: PceBusAccessKind) {
        match kind {
            PceBusAccessKind::Read => self.profiling.snapshot.bus_reads += 1,
            PceBusAccessKind::Write => self.profiling.snapshot.bus_writes += 1,
            PceBusAccessKind::DummyRead => self.profiling.snapshot.bus_dummy_reads += 1,
            PceBusAccessKind::DummyWrite => self.profiling.snapshot.bus_dummy_writes += 1,
        }
        self.profiling.record_bus_access(
            self.inner.devices().topology(),
            self.inner.hucard_board(),
            physical_addr,
            kind,
        );
        self.profiling.snapshot.plain_memory_lane_direct_accesses += 1;
    }

    fn read_plain_memory(
        &mut self,
        physical_addr: u32,
        target: super::super::bus::PlainMemoryTarget,
        _dummy: bool,
    ) -> u8 {
        #[cfg(not(feature = "profiling"))]
        let _ = physical_addr;
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.plain_memory_lane_attempts += 1;
        }
        #[cfg(feature = "profiling")]
        self.record_direct_plain_memory_access(
            physical_addr,
            if _dummy {
                PceBusAccessKind::DummyRead
            } else {
                PceBusAccessKind::Read
            },
        );
        if self.advance_plain_memory_access() {
            self.inner.read_plain_memory_target(target)
        } else {
            OPEN_BUS_VALUE
        }
    }

    fn write_plain_memory(
        &mut self,
        physical_addr: u32,
        target: super::super::bus::PlainMemoryTarget,
        value: u8,
        _dummy: bool,
    ) {
        #[cfg(not(feature = "profiling"))]
        let _ = physical_addr;
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.plain_memory_lane_attempts += 1;
        }
        debug_assert!(matches!(
            target,
            super::super::bus::PlainMemoryTarget::WorkRam(_)
        ));
        #[cfg(feature = "profiling")]
        self.record_direct_plain_memory_access(
            physical_addr,
            if _dummy {
                PceBusAccessKind::DummyWrite
            } else {
                PceBusAccessKind::Write
            },
        );
        if self.advance_plain_memory_access() {
            self.inner.write_plain_memory_target(target, value);
            self.observe_dma_completion();
        }
    }

    fn record_plain_memory_fallback(&mut self) {
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.plain_memory_lane_attempts += 1;
            self.profiling.snapshot.plain_memory_lane_fallback_accesses += 1;
        }
    }

    fn advance_direct_vdc_access(&mut self, port: VdcPort) -> bool {
        self.advance_cycle();
        self.materialize_pending_devices(
            #[cfg(feature = "profiling")]
            PceDeviceMaterializationCause::DirectVdc,
        );
        self.advance_elapsed_master_ticks(
            u64::from(PCE_VDC_VCE_ACCESS_WAIT_CYCLES) * self.master_ticks_per_cycle,
        );
        self.materialize_pending_devices(
            #[cfg(feature = "profiling")]
            PceDeviceMaterializationCause::DirectVdc,
        );
        self.video_wait_cycles += PCE_VDC_VCE_ACCESS_WAIT_CYCLES;
        if let Some(target) = direct_vdc_vram_write_target(self.inner, port) {
            self.wait_for_vdc_dma(target);
        }
        self.fault.is_none()
    }

    pub(super) fn advance_remaining(
        &mut self,
        reported_cycles: u32,
    ) -> Result<(), PceMachineError> {
        let remaining = reported_cycles.checked_sub(self.observed_cycles).ok_or(
            PceMachineError::CpuCycleAccounting {
                reported: reported_cycles,
                observed: self.observed_cycles,
            },
        )?;
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.bus_idle_cycles += u64::from(remaining);
        }
        self.advance_elapsed_master_ticks(u64::from(remaining) * self.master_ticks_per_cycle);
        let horizon = self.next_fallible_device_horizon();
        if self.pending_device_master_ticks >= horizon {
            self.materialize_pending_devices(
                #[cfg(feature = "profiling")]
                if horizon == 0 {
                    PceDeviceMaterializationCause::DmaHorizon
                } else {
                    PceDeviceMaterializationCause::LineHorizon
                },
            );
        }
        self.fault.map_or(Ok(()), Err)
    }

    fn record_trace_write(&mut self, write: TraceWrite) {
        let Some(trace) = &mut self.trace else {
            return;
        };
        let len = usize::from(trace.trace_write_len);
        if len == MAX_TRACE_WRITES {
            trace.trace_write_overflow = trace.trace_write_overflow.saturating_add(1);
        } else {
            trace.trace_writes[len] = write;
            trace.trace_write_len += 1;
        }
    }

    fn wait_for_vdc_dma(&mut self, target: VpcVdc) {
        while self.fault.is_none()
            && self
                .inner
                .devices()
                .vdc_for(target)
                .is_some_and(HuC6270::dma_owns_vram_slots)
        {
            self.advance_devices(self.master_ticks_per_cycle);
            self.video_wait_cycles += 1;
            self.vram_contention_wait_cycles += 1;
        }
    }

    fn advance_elapsed_master_ticks(&mut self, master_ticks: u64) {
        if self.fault.is_some() || master_ticks == 0 {
            return;
        }
        #[cfg(test)]
        if !self.coalesce_device_advancement {
            self.advance_devices_now(master_ticks);
            return;
        }
        self.pending_device_master_ticks += master_ticks;
    }

    fn next_fallible_device_horizon(&self) -> u64 {
        #[cfg(test)]
        if self.fault_after_device_chunks.is_some() {
            return 0;
        }
        let vdc = self.inner.devices().vdc();
        if vdc.pending_vram_dma().is_some()
            || vdc.active_vram_dma().is_some()
            || vdc.pending_satb_dma().is_some()
            || vdc.active_satb_dma().is_some()
        {
            return 0;
        }
        PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE - *self.vce_line_accumulator
    }

    fn materialize_pending_devices(
        &mut self,
        #[cfg(feature = "profiling")] cause: PceDeviceMaterializationCause,
    ) {
        let master_ticks = std::mem::take(&mut self.pending_device_master_ticks);
        #[cfg(feature = "profiling")]
        self.profiling
            .record_device_materialization(cause, master_ticks);
        self.advance_devices_now(master_ticks);
    }

    pub(super) fn finish_action(&mut self) {
        self.materialize_pending_devices(
            #[cfg(feature = "profiling")]
            PceDeviceMaterializationCause::ActionFinish,
        );
    }

    pub(super) fn advance_devices(&mut self, master_ticks: u64) {
        self.advance_elapsed_master_ticks(master_ticks);
        self.materialize_pending_devices(
            #[cfg(feature = "profiling")]
            PceDeviceMaterializationCause::DirectVdc,
        );
    }

    fn advance_devices_now(&mut self, master_ticks: u64) {
        if self.fault.is_some() || master_ticks == 0 {
            return;
        }
        #[cfg(test)]
        {
            self.device_advance_calls += 1;
        }
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.device_advance_calls += 1;
            self.profiling.snapshot.device_advance_master_ticks += master_ticks;
        }
        let mut remaining = master_ticks;
        while remaining != 0 {
            let until_line = PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE - *self.vce_line_accumulator;
            let elapsed = remaining.min(until_line);
            #[cfg(feature = "profiling")]
            {
                self.profiling.snapshot.device_advance_chunks += 1;
            }
            let result = self.advance_video_chunk(elapsed);
            self.elapsed_master_ticks += elapsed;
            self.unclaimed_on_chip_master_ticks += elapsed;
            #[cfg(feature = "profiling")]
            self.inner
                .devices_mut()
                .advance_master_ticks_profiled(elapsed, self.profiling);
            #[cfg(not(feature = "profiling"))]
            self.inner.devices_mut().advance_master_ticks(elapsed);
            self.observe_dma_completion();
            #[cfg(test)]
            if let Some(chunks) = &mut self.fault_after_device_chunks {
                *chunks -= 1;
                if *chunks == 0 {
                    self.fault = Some(PceMachineError::ClockOverflow {
                        counter: PceClockCounter::MasterTicks,
                        current: 0,
                        delta: 0,
                    });
                    return;
                }
            }
            remaining -= elapsed;
            if let Err(error) = result {
                self.fault = Some(error);
                return;
            }
        }
    }

    fn observe_dma_completion(&mut self) {
        self.dma_completed |= self.inner.devices_mut().take_debug_dma_completed();
    }

    fn advance_video_chunk(&mut self, master_ticks: u64) -> Result<(), PceMachineError> {
        debug_assert!(
            master_ticks <= PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE - *self.vce_line_accumulator
        );
        self.advance_vdc_master_ticks(master_ticks)?;
        *self.vce_line_accumulator += master_ticks;
        if *self.vce_line_accumulator == PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE {
            *self.vce_line_accumulator = 0;
            self.process_vce_line()?;
            self.inner.devices_mut().begin_external_horizontal_line();
            self.vce_lines += 1;
            #[cfg(feature = "profiling")]
            {
                self.profiling.snapshot.vce_line_transitions += 1;
            }
            *self.vce_line_index += 1;
            if *self.vce_line_index == self.vce_frame_length.scanlines() {
                *self.vce_line_index = 0;
                std::mem::swap(self.front_video, self.back_video);
                self.back_video.begin_frame();
                self.frames_published += 1;
            }
        }
        Ok(())
    }

    fn advance_vdc_master_ticks(&mut self, master_ticks: u64) -> Result<(), PceMachineError> {
        let total = master_ticks + u64::from(*self.vdc_pixel_clock_remainder);
        let (pixel_clocks, remainder) =
            split_vdc_master_ticks(total, self.inner.devices().vce().pixel_clock());
        *self.vdc_pixel_clock_remainder = remainder;
        if pixel_clocks != 0 {
            #[cfg(feature = "profiling")]
            {
                let advance = self
                    .inner
                    .devices_mut()
                    .advance_horizontal_pixels(pixel_clocks)
                    .map_err(PceMachineError::Dma)?;
                let second = advance.1;
                let first = advance.0;
                let dma_slots =
                    first.dma_slots() + second.map_or(0, VdcHorizontalAdvance::dma_slots);
                let active_dma_slots = first.satb_words()
                    + first.vram_words()
                    + second.map_or(0, VdcHorizontalAdvance::satb_words)
                    + second.map_or(0, VdcHorizontalAdvance::vram_words);
                self.profiling.record_vdc_advance(
                    pixel_clocks,
                    first.phase_transitions()
                        + second.map_or(0, VdcHorizontalAdvance::phase_transitions),
                    dma_slots,
                    active_dma_slots,
                );
            }
            #[cfg(not(feature = "profiling"))]
            self.inner
                .devices_mut()
                .advance_horizontal_pixels(pixel_clocks)
                .map_err(PceMachineError::Dma)?;
        }
        Ok(())
    }

    fn process_vce_line(&mut self) -> Result<(), PceMachineError> {
        let vsync_started = *self.vce_line_index == 0;
        if vsync_started {
            *self.vce_frame_length = self.inner.devices().vce().frame_length();
        }
        let input = VdcExternalVceScanline::new(1, vsync_started, *self.vce_frame_length);
        let (boundary, second_boundary) = self
            .inner
            .devices_mut()
            .advance_machine_vce_scanline(input)
            .map_err(PceMachineError::UnsupportedVdcSync)?;
        let pixel_clock = self.inner.devices().vce().pixel_clock();
        match second_boundary {
            None => {
                if let Some(display) = boundary.active_display() {
                    #[cfg(feature = "profiling")]
                    {
                        self.profiling.snapshot.raster_base_lines += 1;
                        self.profiling.snapshot.raster_active_lines += 1;
                        self.profiling.snapshot.raster_pixels += u64::from(display.source_width());
                    }
                    let (vdc, vce) = self.inner.devices_mut().video_devices_mut();
                    self.back_video
                        .render_active_line(vdc, vce, display, *self.vce_line_index, pixel_clock)
                        .map_err(PceMachineError::VideoRender)?;
                }
            }
            Some(second) => {
                let display_one = boundary.active_display();
                let display_two = second.active_display();
                if display_one.is_some() || display_two.is_some() {
                    #[cfg(feature = "profiling")]
                    {
                        self.profiling.snapshot.raster_supergrafx_lines += 1;
                        self.profiling.snapshot.raster_active_lines += 1;
                        self.profiling.snapshot.raster_pixels += u64::from(
                            display_one
                                .into_iter()
                                .chain(display_two)
                                .map(VdcActiveDisplayLine::source_width)
                                .max()
                                .unwrap_or(0),
                        );
                    }
                    let (vdc_one, vdc_two, vpc, vce) = self
                        .inner
                        .devices_mut()
                        .supergrafx_video_devices_mut()
                        .expect("SuperGrafx boundary requires SuperGrafx devices");
                    self.back_video
                        .render_supergrafx_active_line(
                            vdc_one,
                            vdc_two,
                            vpc,
                            vce,
                            display_one,
                            display_two,
                            *self.vce_line_index,
                            pixel_clock,
                        )
                        .map_err(PceMachineError::VideoRender)?;
                }
            }
        }
        Ok(())
    }
}

#[inline]
fn split_vdc_master_ticks(total: u64, pixel_clock: VcePixelClock) -> (u64, u8) {
    match pixel_clock {
        VcePixelClock::DivideByFour => (total >> 2, (total & 3) as u8),
        VcePixelClock::DivideByThree => (total / 3, (total % 3) as u8),
        VcePixelClock::DivideByTwo => (total >> 1, (total & 1) as u8),
    }
}

impl CpuBus for TimedMachineBus<'_> {
    fn read(&mut self, physical_addr: u32) -> u8 {
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.bus_reads += 1;
            self.profiling.record_bus_access(
                self.inner.devices().topology(),
                self.inner.hucard_board(),
                physical_addr,
                PceBusAccessKind::Read,
            );
        }
        if self.advance_access(physical_addr, false) {
            self.inner.read(physical_addr)
        } else {
            OPEN_BUS_VALUE
        }
    }

    fn write(&mut self, physical_addr: u32, value: u8) {
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.bus_writes += 1;
            self.profiling.record_bus_access(
                self.inner.devices().topology(),
                self.inner.hucard_board(),
                physical_addr,
                PceBusAccessKind::Write,
            );
        }
        if self.advance_access(physical_addr, true) {
            self.inner.write(physical_addr, value);
            self.observe_dma_completion();
        }
    }

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.bus_dummy_reads += 1;
            self.profiling.record_bus_access(
                self.inner.devices().topology(),
                self.inner.hucard_board(),
                physical_addr,
                PceBusAccessKind::DummyRead,
            );
        }
        if self.advance_access(physical_addr, false) {
            self.inner.dummy_read(physical_addr)
        } else {
            OPEN_BUS_VALUE
        }
    }

    fn dummy_write(&mut self, physical_addr: u32, value: u8) {
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.bus_dummy_writes += 1;
            self.profiling.record_bus_access(
                self.inner.devices().topology(),
                self.inner.hucard_board(),
                physical_addr,
                PceBusAccessKind::DummyWrite,
            );
        }
        if self.advance_access(physical_addr, true) {
            self.inner.dummy_write(physical_addr, value);
            self.observe_dma_completion();
        }
    }

    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        #[cfg(feature = "profiling")]
        {
            self.profiling.snapshot.bus_writes += 1;
            self.profiling.snapshot.bus_vdc_accesses += 1;
        }
        let old_value = self
            .trace_enabled
            .then(|| self.inner.peek(0x1F_E000 | u32::from(port.offset())));
        if self.advance_direct_vdc_access(port) {
            self.inner.write_vdc(port, value);
            if let Some(old_value) = old_value {
                self.record_trace_write(TraceWrite {
                    address: u32::from(port.offset()),
                    old_value: u32::from(old_value),
                    new_value: u32::from(value),
                    width: TraceWriteWidth::Byte,
                    kind: TraceWriteKind::Io,
                });
            }
        }
    }

    fn advance_internal_access(&mut self, physical_addr: u32, write: bool) -> bool {
        self.advance_access(physical_addr, write)
    }

    fn take_elapsed_master_ticks(&mut self) -> u64 {
        std::mem::take(&mut self.unclaimed_on_chip_master_ticks)
    }

    fn observe_internal_read(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.inner
            .observe_internal_read(physical_addr, value, dummy);
    }

    fn observe_internal_write(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.inner
            .observe_internal_write(physical_addr, value, dummy);
    }

    fn observe_logical_read(
        &mut self,
        logical_addr: u16,
        _physical_addr: u32,
        value: u8,
        _dummy: bool,
    ) {
        if self.debug.hit_watchpoint.is_none() {
            self.debug
                .check_watch_read(Address::from(logical_addr), value);
        }
    }

    fn observe_logical_write(
        &mut self,
        logical_addr: u16,
        physical_addr: u32,
        value: u8,
        _dummy: bool,
    ) {
        let Some((pending_addr, old_value)) = self.pending_debug_write.take() else {
            return;
        };
        debug_assert_eq!(pending_addr, physical_addr);
        self.record_trace_write(TraceWrite {
            address: u32::from(logical_addr),
            old_value: u32::from(old_value),
            new_value: u32::from(value),
            width: TraceWriteWidth::Byte,
            kind: TraceWriteKind::Memory,
        });
        if self.debug.hit_watchpoint.is_none() {
            self.debug
                .check_watch_write(Address::from(logical_addr), old_value, value);
        }
    }

    fn observe_instruction_byte(&mut self, _logical_addr: u16, _physical_addr: u32, value: u8) {
        if let Some(trace) = &mut self.trace {
            let len = usize::from(trace.instruction_byte_len);
            if len < MAX_TRACE_INSTRUCTION_BYTES {
                trace.instruction_bytes[len] = value;
                trace.instruction_byte_len += 1;
            }
        }
    }

    fn idle(&mut self) {
        self.advance_cycle();
        self.inner.idle();
    }
}

impl PlainMemoryCpuBus for TimedMachineBus<'_> {
    #[inline]
    fn plain_memory_target_for_region(
        &self,
        region: super::super::bus::PhysicalRegion,
    ) -> Option<super::super::bus::PlainMemoryTarget> {
        self.inner.plain_memory_target_for_region(region)
    }

    #[inline]
    fn read_plain_memory(
        &mut self,
        physical_addr: u32,
        target: super::super::bus::PlainMemoryTarget,
        dummy: bool,
    ) -> u8 {
        Self::read_plain_memory(self, physical_addr, target, dummy)
    }

    #[inline]
    fn write_plain_memory(
        &mut self,
        physical_addr: u32,
        target: super::super::bus::PlainMemoryTarget,
        value: u8,
        dummy: bool,
    ) {
        Self::write_plain_memory(self, physical_addr, target, value, dummy);
    }

    #[inline]
    fn record_plain_memory_fallback(&mut self) {
        Self::record_plain_memory_fallback(self);
    }
}

const fn is_vdc_vce_access(physical_addr: u32) -> bool {
    matches!(
        physical_addr & super::super::cpu::PHYSICAL_ADDRESS_MASK,
        0x1F_E000..=0x1F_E7FF
    )
}

#[inline]
fn is_timing_observable_access(bus: &BaseBus<PceDevices>, physical_addr: u32) -> bool {
    matches!(
        bus.decode_physical_region(physical_addr),
        PhysicalRegion::Vdc(_)
            | PhysicalRegion::Vpc(_)
            | PhysicalRegion::Vdc2(_)
            | PhysicalRegion::Vce(_)
            | PhysicalRegion::Psg(_)
            | PhysicalRegion::Timer(_)
            | PhysicalRegion::Controller
            | PhysicalRegion::Irq(_)
    ) || (CDROM2_REGISTER_START..=CDROM2_REGISTER_END)
        .contains(&(physical_addr & super::super::cpu::PHYSICAL_ADDRESS_MASK))
}

#[inline]
fn vdc_vram_cycle_target(
    bus: &BaseBus<PceDevices>,
    physical_addr: u32,
    write: bool,
) -> Option<VpcVdc> {
    let (target, port) = match bus.decode_physical_region(physical_addr) {
        PhysicalRegion::Vdc(port) => (VpcVdc::One, port),
        PhysicalRegion::Vdc2(port) => (VpcVdc::Two, port),
        _ => return None,
    };
    is_vdc_vram_port_cycle(bus, target, port, write).then_some(target)
}

#[inline]
fn direct_vdc_vram_write_target(bus: &BaseBus<PceDevices>, port: VdcPort) -> Option<VpcVdc> {
    let target = bus.devices().direct_vdc_target();
    is_vdc_vram_port_cycle(bus, target, port, true).then_some(target)
}

#[inline]
fn is_vdc_vram_port_cycle(
    bus: &BaseBus<PceDevices>,
    target: VpcVdc,
    port: VdcPort,
    write: bool,
) -> bool {
    if port != VdcPort::DataHigh {
        return false;
    }
    let Some(vdc) = bus.devices().vdc_for(target) else {
        return false;
    };
    match vdc.selected_register() {
        Some(super::super::vdc::VdcRegister::MemoryAddressRead) => write,
        Some(super::super::vdc::VdcRegister::VramData) => true,
        _ => false,
    }
}

#[cfg(test)]
#[path = "timed_bus_tests.rs"]
mod tests;
