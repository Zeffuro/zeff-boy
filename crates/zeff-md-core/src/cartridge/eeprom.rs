use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const WRITE_CYCLES_PER_SECOND: u64 = 200;
mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChipProfile {
    Serial128,
    Serial256,
    Split256,
    Split2048,
    Split8192,
    Split512,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Address,
    AddressAck,
    Write,
    WriteAck,
    Read,
    ReadAck,
    WaitStop,
    Device,
    DeviceAck,
    WordAddress,
    WordAddressAck,
    WordAddressHigh,
    WordAddressHighAck,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Eeprom {
    profile: ChipProfile,
    scl: bool,
    sda: bool,
    released: bool,
    phase: Phase,
    bits: u8,
    shift: u8,
    address: u16,
    reading: bool,
    ack: bool,
    page: [u8; 64],
    page_base: u16,
    dirty: u64,
    busy_until: u64,
}

impl Default for Eeprom {
    fn default() -> Self {
        Self {
            profile: ChipProfile::Serial128,
            scl: true,
            sda: true,
            released: true,
            phase: Phase::Idle,
            bits: 0,
            shift: 0,
            address: 0,
            reading: false,
            ack: false,
            page: [0; 64],
            page_base: 0,
            dirty: 0,
            busy_until: 0,
        }
    }
}

impl Eeprom {
    pub(super) fn new(separate_address: bool) -> Self {
        Self {
            profile: if separate_address {
                ChipProfile::Serial256
            } else {
                ChipProfile::Serial128
            },
            ..Self::default()
        }
    }

    pub(super) fn new_split_lines() -> Self {
        Self {
            profile: ChipProfile::Split256,
            ..Self::default()
        }
    }

    pub(super) fn split_lines(&self) -> bool {
        matches!(
            self.profile,
            ChipProfile::Split256
                | ChipProfile::Split512
                | ChipProfile::Split2048
                | ChipProfile::Split8192
        )
    }

    pub(super) fn new_split_512() -> Self {
        Self {
            profile: ChipProfile::Split512,
            ..Self::default()
        }
    }

    pub(super) fn new_split_2048() -> Self {
        Self {
            profile: ChipProfile::Split2048,
            ..Self::default()
        }
    }

    pub(super) fn chip_profile(&self) -> ChipProfile {
        self.profile
    }

    pub(super) fn new_split_8192() -> Self {
        Self {
            profile: ChipProfile::Split8192,
            ..Self::default()
        }
    }

    fn cached_address(&self) -> bool {
        self.profile == ChipProfile::Split8192
    }

    fn wide_address(&self) -> bool {
        self.banked_address() || self.cached_address()
    }

    fn write_offset(&self) -> u16 {
        if self.cached_address() {
            self.address.wrapping_sub(self.page_base) & self.address_mask()
        } else {
            self.address & self.page_mask()
        }
    }

    fn write_cursor_valid(&self) -> bool {
        if self.cached_address() {
            self.write_offset() < 64
        } else {
            self.page_base == self.address & !self.page_mask()
        }
    }

    fn banked_address(&self) -> bool {
        matches!(self.profile, ChipProfile::Split512 | ChipProfile::Split2048)
    }

    fn bank_mask(&self) -> u16 {
        self.address_mask() & !0xff
    }

    fn device_bank(&self, value: u8) -> u16 {
        (u16::from(value) << 7) & self.bank_mask()
    }

    fn page_mask(&self) -> u16 {
        match self.profile {
            ChipProfile::Serial128 | ChipProfile::Serial256 => 3,
            ChipProfile::Split256 => 7,
            ChipProfile::Split512 | ChipProfile::Split2048 => 15,
            ChipProfile::Split8192 => 7,
        }
    }

    fn page_len(&self) -> usize {
        if self.cached_address() {
            64
        } else {
            usize::from(self.page_mask()) + 1
        }
    }

    fn accepts_device(&self, value: u8) -> bool {
        value
            & if self.split_lines() && !self.cached_address() {
                0xf0
            } else {
                0xfe
            }
            == 0xa0
    }

    pub(super) fn write_line(
        &mut self,
        value: u8,
        clock: bool,
        now: u64,
        master_hz: u64,
        bytes: &mut [u8],
    ) {
        let scl = if clock { value & 1 != 0 } else { self.scl };
        let sda = if clock { self.sda } else { value & 1 != 0 };
        self.write(u8::from(sda) | (u8::from(scl) << 1), now, master_hz, bytes);
    }

    pub(super) fn separate_address(&self) -> bool {
        self.profile != ChipProfile::Serial128
    }

    pub(super) fn reset_clock(&mut self, now: u64) {
        self.busy_until = self.busy_until.saturating_sub(now);
    }

    fn address_mask(&self) -> u16 {
        match self.profile {
            ChipProfile::Serial128 => 0x7f,
            ChipProfile::Split512 => 0x1ff,
            ChipProfile::Split2048 => 0x7ff,
            ChipProfile::Split8192 => 0x1fff,
            _ => 0xff,
        }
    }

    pub(super) fn profile(rom: &[u8]) -> Option<bool> {
        if rom.len() == 0x10_0000
            && rom.get(0x180..0x18e) == Some(b"MK 00001211-00")
            && rom.get(0x1b0..0x1bc) == Some(b"RA\xe8\x40\x00\x03\x0d\x41\x00\x03\x0d\x41")
        {
            return Some(false);
        }
        if rom.len() == 0x20_0000
            && rom.get(0x1b0..0x1bc) == Some(b"RA\xe8\x40\x00\x20\x00\x01\x00\x20\x00\x01")
            && matches!(
                rom.get(0x180..0x18e),
                Some(b"GM T-81033  00") | Some(b"GM T-081326 01") | Some(b"GM T-081326 00")
            )
        {
            return Some(true);
        }
        let sega = rom.len() <= 0x20_0000
            && rom.get(0x1b0..0x1bc) == Some(b"RA\xe8\x40\x00\x20\x00\x01\x00\x20\x00\x01")
            && matches!(
                rom.get(0x180..0x18e),
                Some(b"GM 00054503-00")
                    | Some(b"GM G-4060  -00")
                    | Some(b"GM MK-1228 -00")
                    | Some(b"GM G-5538  -00")
                    | Some(b"GM MK-1215 -00")
                    | Some(b"GM_00004076-00")
                    | Some(b"GM T-12046 -00")
                    | Some(b"GM T-12053 -00")
                    | Some(b"GM G-4524  -00")
            );
        sega.then_some(false)
    }

    pub(super) fn output(&self) -> u8 {
        u8::from(self.sda && self.released)
            << u8::from(self.separate_address() && !self.split_lines())
    }

    pub(super) fn write(&mut self, value: u8, now: u64, master_hz: u64, bytes: &mut [u8]) {
        let scl = value & 2 != 0;
        let sda = value & 1 != 0;
        let old_wire = self.sda && self.released;
        let wire = sda && self.released;
        if self.scl && scl && old_wire != wire {
            if wire {
                if self.dirty != 0 {
                    for offset in 0..self.page_len() {
                        if self.dirty & (1 << offset) != 0 {
                            let address = (usize::from(self.page_base) + offset)
                                & usize::from(self.address_mask());
                            bytes[address] = self.page[offset];
                        }
                    }
                    let pages = if self.cached_address() {
                        (0..8)
                            .filter(|page| self.dirty & (0xff_u64 << (page * 8)) != 0)
                            .count() as u64
                    } else {
                        1
                    };
                    self.busy_until = now.saturating_add(
                        master_hz
                            .div_ceil(WRITE_CYCLES_PER_SECOND)
                            .saturating_mul(pages),
                    );
                }
                self.phase = Phase::Idle;
                self.bits = 0;
                self.dirty = 0;
                self.released = true;
            } else {
                self.phase = if self.separate_address() {
                    Phase::Device
                } else {
                    Phase::Address
                };
                self.bits = 0;
                self.shift = 0;
                self.dirty = 0;
                self.released = true;
            }
        } else if !self.scl && scl {
            self.rising(sda, now);
        } else if self.scl && !scl {
            self.falling(bytes);
        }
        self.scl = scl;
        self.sda = sda;
    }

    fn rising(&mut self, sda: bool, now: u64) {
        match self.phase {
            Phase::Address
            | Phase::Device
            | Phase::WordAddressHigh
            | Phase::WordAddress
            | Phase::Write => {
                self.shift = (self.shift << 1) | u8::from(sda);
                self.bits += 1;
                if self.bits == 8 {
                    if self.phase == Phase::Address {
                        self.address = u16::from(self.shift >> 1);
                        self.reading = self.shift & 1 != 0;
                        self.page_base = self.address & !self.page_mask();
                        self.ack = now >= self.busy_until;
                        self.phase = Phase::AddressAck;
                    } else if self.phase == Phase::Device {
                        self.reading = self.shift & 1 != 0;
                        self.ack = self.accepts_device(self.shift) && now >= self.busy_until;
                        if self.ack && self.banked_address() {
                            self.address = self.device_bank(self.shift) | (self.address & 0xff);
                        }
                        self.phase = Phase::DeviceAck;
                    } else if self.phase == Phase::WordAddressHigh {
                        self.ack = self.shift & 0xe0 == 0;
                        if self.ack {
                            self.address = (u16::from(self.shift) << 8) | (self.address & 0xff);
                        }
                        self.phase = Phase::WordAddressHighAck;
                    } else if self.phase == Phase::WordAddress {
                        self.address =
                            (self.address & (self.address_mask() & !0xff)) | u16::from(self.shift);
                        self.page_base = self.address & !self.page_mask();
                        self.ack = true;
                        self.phase = Phase::WordAddressAck;
                    } else {
                        let offset = self.write_offset();
                        self.page[usize::from(offset)] = self.shift;
                        self.dirty |= 1 << offset;
                        self.address = if self.cached_address() {
                            (self.page_base + ((offset + 1) & 63)) & self.address_mask()
                        } else {
                            self.page_base | (self.address.wrapping_add(1) & self.page_mask())
                        };
                        self.ack = true;
                        self.phase = Phase::WriteAck;
                    }
                }
            }
            Phase::AddressAck
            | Phase::WriteAck
            | Phase::DeviceAck
            | Phase::WordAddressAck
            | Phase::WordAddressHighAck => self.bits = 9,
            Phase::Read => self.bits += 1,
            Phase::ReadAck => {
                self.ack = !sda;
                self.bits = 1;
            }
            Phase::Idle | Phase::WaitStop => {}
        }
    }

    fn falling(&mut self, bytes: &[u8]) {
        match self.phase {
            Phase::AddressAck
            | Phase::WriteAck
            | Phase::DeviceAck
            | Phase::WordAddressAck
            | Phase::WordAddressHighAck
                if self.bits == 8 =>
            {
                self.released = !self.ack
            }
            Phase::AddressAck
            | Phase::WriteAck
            | Phase::DeviceAck
            | Phase::WordAddressAck
            | Phase::WordAddressHighAck => {
                self.released = true;
                if !self.ack {
                    self.phase = Phase::WaitStop;
                    self.bits = 0;
                } else if matches!(self.phase, Phase::AddressAck | Phase::DeviceAck) && self.reading
                {
                    self.begin_read(bytes);
                } else {
                    self.phase = if self.phase == Phase::DeviceAck {
                        if self.cached_address() {
                            Phase::WordAddressHigh
                        } else {
                            Phase::WordAddress
                        }
                    } else if self.phase == Phase::WordAddressHighAck {
                        Phase::WordAddress
                    } else {
                        Phase::Write
                    };
                    self.bits = 0;
                    self.shift = 0;
                }
            }
            Phase::Read if self.bits == 8 => {
                self.address = self.address.wrapping_add(1) & self.address_mask();
                self.phase = Phase::ReadAck;
                self.bits = 0;
                self.released = true;
            }
            Phase::Read => self.released = self.shift & (0x80 >> self.bits) != 0,
            Phase::ReadAck if self.bits == 1 => {
                if self.ack {
                    self.begin_read(bytes);
                } else {
                    self.phase = Phase::WaitStop;
                    self.bits = 0;
                    self.released = true;
                }
            }
            _ => {}
        }
    }

    fn begin_read(&mut self, bytes: &[u8]) {
        self.phase = Phase::Read;
        self.bits = 0;
        self.shift = bytes[usize::from(self.address)];
        self.released = self.shift & 0x80 != 0;
    }
}
