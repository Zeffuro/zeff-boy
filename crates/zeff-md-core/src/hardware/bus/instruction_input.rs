use super::*;

pub(super) fn pair(high: SampledByte, low: SampledByte) -> InstructionInput {
    InstructionInput {
        value: u16::from_be_bytes([high.value, low.value]),
        origin: [high.origin, low.origin],
    }
}

pub(super) fn selected(access: BusAccess, high: SampledByte, low: SampledByte) -> InstructionInput {
    let mut input = pair(high, low);
    if access.lanes == ByteLanes::Upper {
        input.origin[1] = zeff_m68k::timing::InstructionByteOrigin::UNKNOWN;
    }
    if access.lanes == ByteLanes::Lower {
        input.origin[0] = zeff_m68k::timing::InstructionByteOrigin::UNKNOWN;
    }
    input
}

pub(super) fn lanes<E>(
    access: BusAccess,
    byte: impl Fn(u32) -> Result<SampledByte, E>,
) -> Result<InstructionInput, E> {
    match access.lanes {
        ByteLanes::Upper => {
            byte(access.bus_address).map(|high| pair(high, SampledByte::unknown(0)))
        }
        ByteLanes::Lower => {
            byte(access.bus_address + 1).map(|low| pair(SampledByte::unknown(0), low))
        }
        ByteLanes::Both => byte(access.bus_address)
            .and_then(|high| byte(access.bus_address + 1).map(|low| pair(high, low))),
    }
}

pub(super) fn value<E>(cycle: CycleResult<InstructionInput, E>) -> CycleResult<u16, E> {
    CycleResult {
        result: cycle.result.map(|input| input.value),
        wait_clocks: cycle.wait_clocks,
    }
}

pub(super) fn unknown<E>(cycle: CycleResult<u16, E>) -> CycleResult<InstructionInput, E> {
    CycleResult {
        result: cycle.result.map(InstructionInput::unknown),
        wait_clocks: cycle.wait_clocks,
    }
}

impl Bus {
    pub(super) fn read_input_cycle_timed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, BusError> {
        if let Err(error) = self
            .preflight_svp(access, Direction::Read)
            .and_then(|()| self.preflight_bounded_odd(access, Direction::Read))
            .and_then(|()| self.preflight_bounded_switched(access, None))
            .and_then(|()| self.preflight_save_latch(access, Direction::Read))
            .and_then(|()| self.preflight_download(access, Direction::Read))
        {
            return CycleResult {
                result: Err(error),
                wait_clocks: 0,
            };
        }
        if self.rmw.is_some() {
            return instruction_input::unknown(self.read_ram_rmw(access, start));
        }
        let held = self.hold_svp_main(access, Direction::Read);
        let mut waits = 0u32;
        let mut ports = VdpTransfer::default();
        let result = (|| {
            self.check_main_clock(start)?;
            self.preflight_realtec_read(access)?;
            self.preflight_vdp_read(access)?;
            let input_begin = self.now.get();
            self.wait_external_dma(&mut waits)?;
            self.wait_svp_main(&mut waits)?;
            ports = self.begin_vdp_transfer(access, VdpTransferRequest::Read)?;
            self.advance_main(self.cpu_clock, TRANSFER_CLOCKS)?;
            while let Some(clocks) = self.vdp_read_wait(access)? {
                let total = waits.checked_add(clocks).ok_or(BusError::ClockOverflow)?;
                self.advance_main(self.cpu_clock, clocks)?;
                waits = total;
            }
            self.read_timed(access, input_begin)
        })();
        self.release_svp_main(held);
        if result.is_err() && ports.data_started {
            self.vdp.abort_main_data();
        }
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }
}
