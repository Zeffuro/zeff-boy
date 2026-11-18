use super::*;

pub(super) enum VdpTransferRequest {
    Read,
    Write(u16),
}

impl VdpTransferRequest {
    pub(super) fn direction(&self) -> Direction {
        match self {
            Self::Read => Direction::Read,
            Self::Write(_) => Direction::Write,
        }
    }
}

#[derive(Default)]
pub(super) struct VdpTransfer {
    pub(super) data_started: bool,
    control: Option<MainControlWrite>,
}

struct MainControlWrite {
    access: BusAccess,
    value: u16,
    begin: MasterTicks,
    end: MasterTicks,
}

impl MainControlWrite {
    fn finish(self, at: MasterTicks, access: BusAccess, value: u16) -> (BusAccess, u16) {
        assert_eq!(self.end, at);
        assert_eq!(self.access, access);
        assert_eq!(self.value, value);
        assert_eq!(
            self.end.get() - self.begin.get(),
            u64::from(TRANSFER_CLOCKS) * MAIN_CPU_DIVISOR
        );
        (self.access, self.value)
    }
}

impl Bus {
    pub(super) fn vdp_data_wait(
        &self,
        access: BusAccess,
        receipt: Option<&ZeroFillWrite>,
    ) -> Result<Option<u32>, BusError> {
        if let Some(receipt) = receipt {
            if !Self::zero_fill_data_access(access) {
                return Err(BusError::VdpPort(VdpPortError::Dma(
                    crate::hardware::vdp::VdpDmaError::Busy,
                )));
            }
            self.vdp
                .validate_zero_fill_write(receipt, self.region)
                .map_err(BusError::VdpPort)?;
            return Ok(None);
        }
        self.vdp_wait(access, false)
    }

    pub(super) fn prepare_zero_fill_transfer(
        &self,
        access: BusAccess,
    ) -> Result<Option<ZeroFillWrite>, BusError> {
        if !Self::zero_fill_data_access(access) {
            return Ok(None);
        }
        let end = self
            .now
            .get()
            .checked_add(u64::from(TRANSFER_CLOCKS) * MAIN_CPU_DIVISOR)
            .map(MasterTicks::new)
            .ok_or(BusError::ClockOverflow)?;
        Ok(self.vdp.prepare_zero_fill_write(end, self.region))
    }

    pub(super) fn write_zero_fill_timed_capturing<S: previous::Capture>(
        &mut self,
        write: (BusAccess, u16),
        begin: u64,
        receipt: Option<ZeroFillWrite>,
        ports: &mut VdpTransfer,
        previous: &mut S,
    ) -> Result<(), BusError> {
        let (access, value) = write;
        if let Some(control) = ports.control.take() {
            assert!(receipt.is_none());
            let (held_access, held_value) = control.finish(self.now, access, value);
            return self.write_timed_capturing(held_access, held_value, begin, previous);
        }
        let Some(receipt) = receipt else {
            return self.write_timed_capturing(access, value, begin, previous);
        };
        if !Self::zero_fill_data_access(access) || value != 0 {
            return Err(BusError::VdpPort(VdpPortError::Dma(
                crate::hardware::vdp::VdpDmaError::Busy,
            )));
        }
        #[cfg(feature = "first-failure-diagnostic")]
        crate::diagnostic::service_entry(self, access, crate::diagnostic::Callback::Write);
        self.prefetch = Prefetch::Empty;
        self.vdp
            .commit_zero_fill_write(value, receipt, self.region)
            .map_err(BusError::VdpPort)
    }

    pub(super) fn vdp_read_wait(&mut self, access: BusAccess) -> Result<Option<u32>, BusError> {
        if access.bus_address & VDP_DECODE_MASK == VDP_DECODE_VALUE
            && access.bus_address & VDP_PORT_MASK <= VDP_DATA_END
        {
            self.prefetch = Prefetch::Empty;
        }
        self.vdp_wait(access, true)
    }

    fn vdp_wait(&self, access: BusAccess, read: bool) -> Result<Option<u32>, BusError> {
        if access.bus_address & VDP_DECODE_MASK != VDP_DECODE_VALUE
            || access.bus_address & VDP_PORT_MASK > VDP_DATA_END
        {
            return Ok(None);
        }
        let wait = if read {
            self.vdp.data_read_wait(self.region)
        } else {
            self.vdp.data_write_wait(self.region)
        };
        let Some(deadline) = wait.map_err(BusError::VdpPort)? else {
            return Ok(None);
        };
        let clocks = u32::try_from((deadline.get() - self.now.get()).div_ceil(MAIN_CPU_DIVISOR))
            .map_err(|_| BusError::ClockOverflow)?;
        Ok(Some(clocks))
    }

    pub(super) fn read_vdp(
        &mut self,
        access: BusAccess,
        main_data: bool,
    ) -> Option<Result<u16, BusError>> {
        if access.bus_address & VDP_DECODE_MASK != VDP_DECODE_VALUE
            || access.bus_address & VDP_PORT_MASK > VDP_HV_END
        {
            return None;
        }
        if self.vdp.requires_timed_ports() && !self.vdp.has_timed_status() {
            return Some(Err(BusError::VdpPort(VdpPortError::Status(
                crate::hardware::vdp::VdpStatusError::UnknownBusHistory,
            ))));
        }
        if access.bus_address & VDP_PORT_MASK >= VDP_HV_START {
            self.vdp.retire_timed_port_history();
            return Some(self.vdp.read_hv(self.region).map_err(BusError::Vdp));
        }
        if access.bus_address & VDP_PORT_MASK >= VDP_CONTROL_START {
            let upper = if access.lanes == ByteLanes::Lower {
                0
            } else {
                let upper = self.prefetch.upper().unwrap_or(0);
                (u16::from(upper) << 8) & VDP_STATUS_FLOAT_MASK
            };
            let word = if self.vdp.has_timed_status() {
                self.vdp.finish_timed_status(self.now, self.region)
            } else {
                self.vdp.read_cpu_status(self.region)
            };
            return Some(word.map(|word| word | upper).map_err(BusError::VdpPort));
        }
        let result = if main_data && Self::ordinary_vdp_data(access) {
            self.vdp.read_main_data(self.region)
        } else {
            self.vdp.read_data(self.region)
        };
        Some(result.map_err(BusError::VdpPort))
    }

    pub(super) fn write_vdp(
        &mut self,
        access: BusAccess,
        value: u16,
        main_data: bool,
    ) -> Option<Result<(), BusError>> {
        let address = access.bus_address;
        if address & VDP_DECODE_MASK != VDP_DECODE_VALUE
            || address & VDP_PORT_MASK > VDP_CONTROL_END
        {
            return None;
        }
        if self.vdp.requires_timed_ports() && !self.vdp.has_timed_control() {
            return Some(Err(BusError::VdpPort(VdpPortError::Status(
                crate::hardware::vdp::VdpStatusError::UnknownBusHistory,
            ))));
        }
        let [high, low] = value.to_be_bytes();
        let word = match access.lanes {
            ByteLanes::Both => value,
            ByteLanes::Upper => u16::from_be_bytes([high; 2]),
            ByteLanes::Lower => u16::from_be_bytes([low; 2]),
        };
        let result = if address & VDP_PORT_MASK < VDP_CONTROL_START {
            if main_data && Self::ordinary_vdp_data(access) {
                self.vdp.write_main_data(word, self.region)
            } else {
                self.vdp.retire_timed_port_history();
                self.vdp.write_data(word, self.region)
            }
        } else {
            let timed = self.vdp.has_timed_control();
            let result = self
                .vdp
                .write_control_from(
                    word,
                    self.region,
                    VdpDmaSource::from_cartridge(&self.cartridge, &self.ram)
                        .with_tmss(self.tmss.as_ref()),
                )
                .map(|_| ());
            match result {
                Ok(()) if timed => self.vdp.finish_timed_control(self.now, self.region),
                Err(error) => {
                    self.vdp.retire_timed_port_history();
                    Err(error)
                }
                result => result,
            }
        };
        Some(result.map_err(BusError::VdpPort))
    }
}

impl Bus {
    fn zero_fill_data_access(access: BusAccess) -> bool {
        access.bus_address & VDP_DECODE_MASK == VDP_DECODE_VALUE
            && access.lanes == ByteLanes::Both
            && Self::ordinary_vdp_data(access)
    }

    fn ordinary_vdp_access(access: BusAccess) -> bool {
        let logical = access.logical_address & BUS_ADDRESS_MASK;
        let canonical = logical & !1 == access.bus_address
            && match access.lanes {
                ByteLanes::Both | ByteLanes::Upper => logical & 1 == 0,
                ByteLanes::Lower => logical & 1 != 0,
            };
        access.kind == AccessKind::Data
            && canonical
            && matches!(
                access.function_code,
                zeff_m68k::bus::FunctionCode::UserData
                    | zeff_m68k::bus::FunctionCode::SupervisorData
            )
    }

    fn ordinary_vdp_control(access: BusAccess) -> bool {
        Self::ordinary_vdp_access(access)
            && (VDP_CONTROL_START..=VDP_CONTROL_END).contains(&(access.bus_address & VDP_PORT_MASK))
    }

    fn ordinary_vdp_data(access: BusAccess) -> bool {
        Self::ordinary_vdp_access(access) && access.bus_address & VDP_PORT_MASK <= VDP_DATA_END
    }

    pub(super) fn preflight_vdp_transfer(
        &self,
        access: BusAccess,
        direction: Direction,
    ) -> Result<(), BusError> {
        self.preflight_tmss_vdp_lock(access, direction)?;
        if access.bus_address & VDP_DECODE_MASK == VDP_DECODE_VALUE
            && Self::ordinary_vdp_control(access)
        {
            self.vdp.preflight_main_port().map_err(BusError::VdpPort)?;
        }
        if access.bus_address & VDP_DECODE_MASK == VDP_DECODE_VALUE
            && Self::ordinary_vdp_data(access)
        {
            self.vdp.preflight_main_data().map_err(BusError::VdpPort)?;
        }
        if access.bus_address & VDP_DECODE_MASK == VDP_DECODE_VALUE
            && access.bus_address & VDP_PORT_MASK <= VDP_HV_END
            && self.vdp.requires_timed_ports()
            && !Self::ordinary_vdp_control(access)
        {
            return Err(BusError::VdpPort(VdpPortError::Status(
                crate::hardware::vdp::VdpStatusError::UnknownBusHistory,
            )));
        }
        Ok(())
    }

    pub(super) fn preflight_vdp_read(&self, access: BusAccess) -> Result<(), BusError> {
        self.preflight_vdp_transfer(access, Direction::Read)?;
        if access.bus_address & VDP_DECODE_MASK == VDP_DECODE_VALUE
            && Self::ordinary_vdp_control(access)
        {
            self.vdp
                .preflight_main_status()
                .map_err(BusError::VdpPort)?;
        }
        Ok(())
    }

    pub(super) fn begin_vdp_transfer(
        &mut self,
        access: BusAccess,
        request: VdpTransferRequest,
    ) -> Result<VdpTransfer, BusError> {
        let direction = request.direction();
        if access.bus_address & VDP_DECODE_MASK != VDP_DECODE_VALUE
            || access.bus_address & VDP_PORT_MASK > VDP_HV_END
        {
            return Ok(VdpTransfer::default());
        }
        self.cpu_clock
            .0
            .checked_add(u64::from(TRANSFER_CLOCKS))
            .ok_or(BusError::ClockOverflow)?;
        let end = self
            .now
            .get()
            .checked_add(u64::from(TRANSFER_CLOCKS) * MAIN_CPU_DIVISOR)
            .map(MasterTicks::new)
            .ok_or(BusError::ClockOverflow)?;
        self.preflight_vdp_transfer(access, direction)?;
        if Self::ordinary_vdp_data(access) {
            self.vdp
                .begin_main_data(self.now, direction == Direction::Write)
                .map_err(BusError::VdpPort)?;
            return Ok(VdpTransfer {
                data_started: self.vdp.has_main_data(),
                control: None,
            });
        }
        if !Self::ordinary_vdp_control(access) {
            self.vdp.retire_timed_port_history();
            return Ok(VdpTransfer::default());
        }
        let started = VdpTransfer {
            data_started: false,
            control: match request {
                VdpTransferRequest::Read => None,
                VdpTransferRequest::Write(value) => Some(MainControlWrite {
                    access,
                    value,
                    begin: self.now,
                    end,
                }),
            },
        };
        let result = match direction {
            Direction::Read => self.vdp.begin_timed_status(self.now, end, self.region),
            Direction::Write => self.vdp.begin_timed_control(self.now, end, self.region),
        };
        if matches!(
            result,
            Err(VdpPortError::Status(
                crate::hardware::vdp::VdpStatusError::UnknownBusHistory
            ))
        ) && self.vdp.retire_unready_main_port()
        {
            return Ok(started);
        }
        result.map(|()| started).map_err(BusError::VdpPort)
    }
}
