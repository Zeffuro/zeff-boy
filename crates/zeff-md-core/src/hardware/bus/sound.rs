use super::*;
use zeff_m68k::bus::FunctionCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Control {
    Request,
    Reset,
}

impl Control {
    pub(super) fn decode(address: u32) -> Option<Self> {
        match address & SOUND_CONTROL_ADDRESS_MASK {
            SOUND_BUS_REQUEST => Some(Self::Request),
            SOUND_RESET => Some(Self::Reset),
            _ => None,
        }
    }
}

impl Bus {
    pub(super) fn discarded_read(&self, access: BusAccess) -> bool {
        Self::discarded_reset_read(access) || self.discarded_parallel_adapter_read(access)
    }

    pub(super) fn discarded_reset_read(access: BusAccess) -> bool {
        access.bus_address == SOUND_RESET
            && access.logical_address & BUS_ADDRESS_MASK == SOUND_RESET
            && access.kind == AccessKind::Data
            && access.lanes == ByteLanes::Upper
            && matches!(
                access.function_code,
                FunctionCode::UserData | FunctionCode::SupervisorData
            )
    }

    pub(super) fn complete_discarded_read(&mut self, access: BusAccess) {
        #[cfg(feature = "first-failure-diagnostic")]
        crate::diagnostic::service_entry(self, access, crate::diagnostic::Callback::Read);
        #[cfg(not(feature = "first-failure-diagnostic"))]
        let _ = access;
        self.prefetch = Prefetch::Empty;
    }

    pub(super) fn reset_observation(access: BusAccess) -> bool {
        access.bus_address == SOUND_RESET
            && access.kind == AccessKind::Data
            && (access.lanes == ByteLanes::Both
                || access.lanes == ByteLanes::Upper
                    && access.logical_address == SOUND_RESET
                    && matches!(
                        access.function_code,
                        FunctionCode::UserData | FunctionCode::SupervisorData
                    ))
    }

    pub(super) fn request_observation(access: BusAccess) -> bool {
        access.kind == AccessKind::Data
            && matches!(access.lanes, ByteLanes::Both | ByteLanes::Lower)
            && Control::decode(access.bus_address) == Some(Control::Request)
    }

    pub(super) fn closed_sound_read(&self, access: BusAccess) -> bool {
        access.kind == AccessKind::Data
            && (SOUND_APERTURE_START..=SOUND_APERTURE_END).contains(&access.bus_address)
            && (!self.sound.granted || self.sound.reset_asserted)
    }

    pub(super) fn sound_interrupt(&self) -> crate::hardware::vdp::SoundInterrupt {
        self.vdp
            .interrupts()
            .sound_interrupt(self.vdp.registers(), self.region)
    }

    pub(super) fn read_sound(&mut self, access: BusAccess) -> Option<u16> {
        if self.closed_sound_read(access) {
            return self.prefetch.sample(access.lanes);
        }
        if Self::reset_observation(access) {
            return self.prefetch.sample(access.lanes);
        }
        if access.kind == AccessKind::Data
            && access.lanes == ByteLanes::Lower
            && Control::decode(access.bus_address) == Some(Control::Request)
        {
            return self.prefetch.sample(access.lanes);
        }
        if Control::decode(access.bus_address) == Some(Control::Request)
            && access.lanes != ByteLanes::Lower
        {
            let source = match access.lanes {
                ByteLanes::Upper => u16::from(self.prefetch.upper().unwrap_or_default()) << 8,
                ByteLanes::Both => self.prefetch.word()?,
                ByteLanes::Lower => unreachable!(),
            };
            return Some((source & !SOUND_CONTROL_MASK) | (u16::from(!self.sound.granted) << 8));
        }
        match access.bus_address {
            SOUND_APERTURE_START..=SOUND_APERTURE_END
                if self.sound.granted && !self.sound.reset_asserted =>
            {
                let address = (access.bus_address + u32::from(access.lanes == ByteLanes::Lower))
                    & SOUND_MAIN_MASK;
                let status = if (u32::from(SOUND_OPN2_START)..=u32::from(SOUND_OPN2_END))
                    .contains(&address)
                {
                    Some(if address & 3 == 0 {
                        self.audio.flush_pending();
                        self.audio.opn2_status_at(self.now.get())
                    } else {
                        0
                    })
                } else {
                    self.sound.read(address as u16)
                };
                status.map(|value| u16::from_be_bytes([value; 2]))
            }
            _ => None,
        }
    }

    pub(crate) fn set_sound_reset(&mut self, asserted: bool) {
        if asserted && !self.sound.reset_asserted {
            self.audio.reset_opn2(self.now.get());
        }
        self.sound.set_reset(asserted);
    }

    pub(super) fn write_sound(&mut self, access: BusAccess, value: u16) -> bool {
        if access.lanes != ByteLanes::Lower
            && let Some(control) = Control::decode(access.bus_address)
        {
            match control {
                Control::Request => self.sound.request(value & SOUND_CONTROL_MASK != 0),
                Control::Reset => {
                    let asserted = value & SOUND_CONTROL_MASK == 0;
                    self.set_sound_reset(asserted);
                }
            }
            return true;
        }
        match access.bus_address {
            SOUND_APERTURE_START..=SOUND_APERTURE_END
                if !self.sound.granted || self.sound.reset_asserted =>
            {
                true
            }
            SOUND_APERTURE_START..=SOUND_APERTURE_END
                if self.sound.granted && !self.sound.reset_asserted =>
            {
                let lower = access.lanes == ByteLanes::Lower;
                let address = (access.bus_address + u32::from(lower)) & SOUND_MAIN_MASK;
                let byte = if lower {
                    value as u8
                } else {
                    (value >> 8) as u8
                };
                if (u32::from(SOUND_OPN2_START)..=u32::from(SOUND_OPN2_END)).contains(&address) {
                    self.audio
                        .write_opn2(self.now.get(), (address & 3) as u8, byte);
                    true
                } else if (u32::from(SOUND_PSG_START)..=u32::from(SOUND_PSG_END)).contains(&address)
                {
                    if address & 1 != 0 {
                        self.audio.write_psg(self.now.get(), byte);
                    }
                    true
                } else {
                    self.sound.write(address as u16, byte)
                }
            }
            _ => false,
        }
    }
}
