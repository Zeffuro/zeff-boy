use super::*;

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
    pub(super) vce_lines: u64,
    pub(super) frames_published: u64,
    pub(super) fault: Option<PceMachineError>,
    pending_debug_write: Option<(u32, u8)>,
    trace: Option<&'a mut TimedInstructionTrace>,
    trace_enabled: bool,
    capture_old_writes: bool,
    pub(super) dma_completed: bool,
    debug: &'a mut AddressDebugController,
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
            vce_lines: 0,
            frames_published: 0,
            fault: None,
            pending_debug_write: None,
            trace,
            trace_enabled,
            capture_old_writes,
            dma_completed: false,
            debug,
        }
    }

    fn advance_cycle(&mut self) {
        self.observed_cycles += 1;
        self.advance_devices(self.master_ticks_per_cycle);
    }

    fn advance_access(&mut self, physical_addr: u32, write: bool) -> bool {
        self.pending_debug_write = None;
        self.advance_cycle();
        if is_vdc_vce_access(physical_addr) {
            self.advance_devices(
                u64::from(PCE_VDC_VCE_ACCESS_WAIT_CYCLES) * self.master_ticks_per_cycle,
            );
            self.video_wait_cycles += PCE_VDC_VCE_ACCESS_WAIT_CYCLES;
        }
        if let Some(target) = vdc_vram_cycle_target(self.inner, physical_addr, write) {
            self.wait_for_vdc_dma(target);
        }
        let completed = self.fault.is_none();
        if completed && write && self.capture_old_writes {
            self.pending_debug_write = Some((physical_addr, self.inner.peek(physical_addr)));
        }
        completed
    }

    fn advance_direct_vdc_access(&mut self, port: VdcPort) -> bool {
        self.advance_cycle();
        self.advance_devices(
            u64::from(PCE_VDC_VCE_ACCESS_WAIT_CYCLES) * self.master_ticks_per_cycle,
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
        self.advance_devices(u64::from(remaining) * self.master_ticks_per_cycle);
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

    pub(super) fn advance_devices(&mut self, master_ticks: u64) {
        if self.fault.is_some() || master_ticks == 0 {
            return;
        }
        let mut remaining = master_ticks;
        while remaining != 0 {
            let until_line = PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE - *self.vce_line_accumulator;
            let elapsed = remaining.min(until_line);
            let result = self.advance_video_chunk(elapsed);
            self.elapsed_master_ticks += elapsed;
            self.unclaimed_on_chip_master_ticks += elapsed;
            self.inner.devices_mut().advance_master_ticks(elapsed);
            self.observe_dma_completion();
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
        let divisor = u64::from(self.inner.devices().vce().pixel_clock().divisor());
        let total = master_ticks + u64::from(*self.vdc_pixel_clock_remainder);
        let pixel_clocks = total / divisor;
        *self.vdc_pixel_clock_remainder = (total % divisor) as u8;
        self.inner
            .devices_mut()
            .advance_horizontal_pixels(pixel_clocks)
            .map_err(PceMachineError::Dma)?;
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

impl CpuBus for TimedMachineBus<'_> {
    fn read(&mut self, physical_addr: u32) -> u8 {
        if self.advance_access(physical_addr, false) {
            self.inner.read(physical_addr)
        } else {
            OPEN_BUS_VALUE
        }
    }

    fn write(&mut self, physical_addr: u32, value: u8) {
        if self.advance_access(physical_addr, true) {
            self.inner.write(physical_addr, value);
            self.observe_dma_completion();
        }
    }

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        if self.advance_access(physical_addr, false) {
            self.inner.dummy_read(physical_addr)
        } else {
            OPEN_BUS_VALUE
        }
    }

    fn dummy_write(&mut self, physical_addr: u32, value: u8) {
        if self.advance_access(physical_addr, true) {
            self.inner.dummy_write(physical_addr, value);
            self.observe_dma_completion();
        }
    }

    fn write_vdc(&mut self, port: VdcPort, value: u8) {
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

const fn is_vdc_vce_access(physical_addr: u32) -> bool {
    matches!(
        physical_addr & super::super::cpu::PHYSICAL_ADDRESS_MASK,
        0x1F_E000..=0x1F_E7FF
    )
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
