use super::*;

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
    pub(super) fn sound_interrupt(&self) -> crate::hardware::vdp::SoundInterrupt {
        self.vdp
            .interrupts()
            .sound_interrupt(*self.vdp.registers(), self.region)
    }

    pub(super) fn read_sound(&self, access: BusAccess) -> Option<u16> {
        if Control::decode(access.bus_address) == Some(Control::Request)
            && access.lanes != ByteLanes::Lower
        {
            let upper = if access.lanes == ByteLanes::Upper {
                (u16::from(self.prefetch_upper.unwrap_or_default()) << 8) & !SOUND_CONTROL_MASK
            } else {
                0
            };
            return Some(upper | (u16::from(!self.sound.granted) << 8));
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

    pub(super) fn write_sound(&mut self, access: BusAccess, value: u16) -> bool {
        if access.lanes != ByteLanes::Lower
            && let Some(control) = Control::decode(access.bus_address)
        {
            match control {
                Control::Request => self.sound.request(value & SOUND_CONTROL_MASK != 0),
                Control::Reset => {
                    let asserted = value & SOUND_CONTROL_MASK == 0;
                    if asserted && !self.sound.reset_asserted {
                        self.audio.reset_opn2(self.now.get());
                    }
                    self.sound.set_reset(asserted);
                }
            }
            return true;
        }
        match access.bus_address {
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
