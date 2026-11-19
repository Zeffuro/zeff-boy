use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SoundPins {
    pub address: Option<u16>,
    pub data: Option<u8>,
    pub mreq: bool,
    pub read: bool,
    pub write: bool,
    pub m1: bool,
    pub refresh: bool,
    pub iorq: bool,
    pub bus_ack: bool,
}

impl SoundPins {
    fn validate(self) -> Result<()> {
        ensure!(
            !self.iorq
                || self.m1
                    && !self.mreq
                    && !self.read
                    && !self.write
                    && !self.refresh
                    && !self.bus_ack
                    && self.data.is_none()
                    && self.address.is_some(),
            "SOUND IO outside interrupt acknowledge"
        );
        ensure!(!(self.read && self.write), "conflicting SOUND direction");
        ensure!(
            !(self.mreq || self.read || self.write || self.refresh) || self.address.is_some(),
            "active SOUND bus lacks its address"
        );
        ensure!(!self.write || self.data.is_some(), "SOUND write lacks data");
        ensure!(
            !self.bus_ack
                || self.address.is_none()
                    && self.data.is_none()
                    && !(self.mreq
                        || self.read
                        || self.write
                        || self.m1
                        || self.refresh
                        || self.iorq),
            "SOUND bus acknowledge retains active pins"
        );
        Ok(())
    }

    pub(crate) fn shared(self) -> bool {
        self.mreq
            && !self.refresh
            && (self.read || self.write)
            && self
                .address
                .is_some_and(|a| a >= 0x8000 || a & 0xff00 == 0x7f00)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SoundResponse {
    pub wait: bool,
    pub request: bool,
    pub reset: bool,
    pub data: u8,
    pub known: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SoundState {
    pub address: u16,
    pub main_mask: u16,
    pub response: SoundResponse,
}

impl SoundState {
    pub(crate) const fn origin() -> Self {
        Self {
            address: 0,
            main_mask: 0,
            response: SoundResponse {
                wait: true,
                request: false,
                reset: true,
                data: 0,
                known: 0,
            },
        }
    }
}

impl Companion {
    pub(crate) fn validate(self) -> Result<()> {
        ensure!(
            self.pass < 2 && self.bank < 512,
            "invalid SOUND board frontier"
        );
        self.pins.validate()?;
        if self.pins.shared() {
            let address = self.main_address()?;
            let memory = address <= 0x3fffff || address >= 0xe00000;
            let vdp = address & 0xe700e0 == 0xc00000;
            ensure!(
                memory || vdp,
                "SOUND MAIN peripheral owner is absent: {address:06x}"
            );
            if vdp {
                let port = (address >> 1) & 12;
                ensure!(
                    port == 0 || port == 4 && self.pins.read || port == 8 && self.pins.write,
                    "SOUND VDP test or unsupported direction"
                );
            }
        }
        Ok(())
    }

    pub(crate) fn main_address(self) -> Result<u32> {
        let address = self
            .pins
            .address
            .ok_or_else(|| anyhow::anyhow!("SOUND address is undriven"))?;
        Ok(Self::mapped_address(address, self.bank))
    }

    pub(crate) fn mapped_address(address: u16, bank: u16) -> u32 {
        if address & 0xff00 == 0x7f00 {
            0xc00000 | u32::from(address & 255)
        } else {
            u32::from(bank) << 15 | u32::from(address & 0x7fff)
        }
    }
}

impl Board {
    pub fn sound_response(&self) -> SoundResponse {
        self.sound.response
    }

    pub(crate) fn validate_sound_continuation(&self, companion: Companion) -> Result<()> {
        companion.validate()?;
        if let Some(previous) = self.last_companion
            && previous.pins.shared()
            && companion.pins.shared()
        {
            ensure!(
                previous.bank == companion.bank
                    && previous.pins.address == companion.pins.address
                    && previous.pins.read == companion.pins.read
                    && previous.pins.write == companion.pins.write
                    && (!companion.pins.write || previous.pins.data == companion.pins.data),
                "SOUND request changed before its strobes released"
            );
        }
        Ok(())
    }

    pub(crate) fn sound_resolve(&mut self, companion: Companion) -> Result<()> {
        self.validate_sound_continuation(companion)?;
        if let Some(address) = companion.pins.address {
            self.sound.address = address;
        }
        Ok(())
    }

    pub(super) fn sound_bridge(&mut self, stage: Stage) -> (bool, bool) {
        let zv = self.arbiter.sound_to_main_level();
        let vz = self.arbiter.main_to_sound_level();
        let cas = stage.arbiter.cas;
        let to_main = !((zv || !cas) && (vz || cas));
        let to_sound = !((zv || cas) && (vz || !cas));
        let shift = if self.sound.address & 1 == 0 { 8 } else { 0 };
        let known = if to_sound {
            (stage.data_mask >> shift) as u8
        } else {
            0
        };
        self.sound.main_mask = stage.data_mask;
        self.sound.response = SoundResponse {
            wait: !self.arbiter.wait_level,
            request: self.arbiter.sound_request(),
            reset: self.arbiter.sound_reset(),
            data: ((stage.arbiter.data >> shift) as u8) & known,
            known,
        };
        (to_main, !zv)
    }

    pub(crate) fn write_sound(&self, writer: &mut StateWriter) {
        writer.write_u16(self.sound.address);
        writer.write_u16(self.sound.main_mask);
        writer.write_bool(self.sound.response.wait);
        writer.write_bool(self.sound.response.request);
        writer.write_bool(self.sound.response.reset);
        writer.write_u8(self.sound.response.data);
        writer.write_u8(self.sound.response.known);
    }

    pub(crate) fn read_sound(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        self.sound = SoundState {
            address: reader.read_u16()?,
            main_mask: reader.read_u16()?,
            response: SoundResponse {
                wait: reader.read_bool()?,
                request: reader.read_bool()?,
                reset: reader.read_bool()?,
                data: reader.read_u8()?,
                known: reader.read_u8()?,
            },
        };
        ensure!(
            self.sound.response.data & !self.sound.response.known == 0,
            "noncanonical SOUND response"
        );
        Ok(())
    }
}

pub(crate) fn write_pins(writer: &mut StateWriter, pins: SoundPins) {
    writer.write_bool(pins.address.is_some());
    if let Some(address) = pins.address {
        writer.write_u16(address);
    }
    writer.write_bool(pins.data.is_some());
    if let Some(data) = pins.data {
        writer.write_u8(data);
    }
    logic::write_bits(
        writer,
        [
            pins.mreq,
            pins.read,
            pins.write,
            pins.m1,
            pins.refresh,
            pins.iorq,
            pins.bus_ack,
        ],
    );
}

pub(crate) fn read_pins(reader: &mut StateReader<'_>) -> Result<SoundPins> {
    let address = if reader.read_bool()? {
        Some(reader.read_u16()?)
    } else {
        None
    };
    let data = if reader.read_bool()? {
        Some(reader.read_u8()?)
    } else {
        None
    };
    let [mreq, read, write, m1, refresh, iorq, bus_ack] = logic::read_bits(reader)?;
    let pins = SoundPins {
        address,
        data,
        mreq,
        read,
        write,
        m1,
        refresh,
        iorq,
        bus_ack,
    };
    pins.validate()?;
    Ok(pins)
}
