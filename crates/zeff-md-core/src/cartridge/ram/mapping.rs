use super::*;

impl SaveMemory {
    pub(in crate::cartridge) fn special_rom_overlay_layout(&self) -> Option<OddSaveLayout> {
        matches!(
            self.layout,
            OddSaveLayout::ExpansionBytesKiB64
                | OddSaveLayout::BoundedSwitchedBytes2048
                | OddSaveLayout::BoundedOdd2400
                | OddSaveLayout::BoundedSwitchedOddKiB8
                | OddSaveLayout::ReservedSwitchedRom
                | OddSaveLayout::BoundedSplitKiB32
        )
        .then_some(self.layout)
    }

    pub(crate) fn unclaimed_parallel_8k(&self) -> bool {
        matches!(
            self.layout,
            OddSaveLayout::KiB8 | OddSaveLayout::HeaderlessOddKiB8
        ) && self.start == SAVE_START
            && !self.switched
            && !self.time_latch
            && self.serial.is_none()
            && self.bytes.len() == 8192
    }

    pub(crate) fn ordinary_timed_header(&self, rom: &[u8]) -> bool {
        rom.len() <= SAVE_START as usize
            && rom.get(0x1b0..0x1bc) == Some(b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff")
            && self.layout == OddSaveLayout::KiB8
            && self.start == SAVE_START
            && !self.switched
            && self.initially_enabled
            && !self.time_latch
            && self.serial.is_none()
            && self.bytes.len() == 8192
            && self.validate_state().is_ok()
    }

    pub(crate) fn partial_switched(&self) -> bool {
        self.writable_partial_switched() || self.reserved_switched()
    }

    pub(crate) fn writable_partial_switched(&self) -> bool {
        self.bounded_switched() || self.bounded_bytes()
    }

    pub(crate) fn partial_switched_byte(&self, address: u32) -> bool {
        self.bounded_switched_byte(address) || self.bounded_bytes_byte(address)
    }

    pub fn rom_visible(&self) -> bool {
        self.switched && !self.enabled
    }

    pub fn control(&mut self, value: u8, word: bool) -> bool {
        if self.expansion_bytes() {
            return false;
        }
        if self.reserved_switched() {
            return false;
        }
        if self.writable_partial_switched() {
            return self.control_partial_switched(value, word);
        }
        if matches!(
            self.layout,
            OddSaveLayout::FixedOddKiB8 | OddSaveLayout::HeaderlessOddKiB8
        ) || self.bounded_odd()
        {
            return false;
        }
        if self.bounded_split() {
            return true;
        }
        if let Some(serial) = &self.serial {
            !serial.separate_address()
        } else if self.switched && (self.time_latch || value <= 2) {
            self.enabled = value & 1 != 0;
            true
        } else if !self.switched && !word && (value == 1 || value == 3 && self.protectable()) {
            self.enabled = true;
            self.write_protected = value == 3;
            true
        } else {
            false
        }
    }

    fn control_partial_switched(&mut self, value: u8, word: bool) -> bool {
        if word || value > 3 {
            return false;
        }
        self.enabled = value & 1 != 0;
        self.write_protected = self.enabled && value & 2 != 0;
        true
    }

    pub(super) fn protectable(&self) -> bool {
        self.writable_partial_switched()
            || !self.switched
                && self.serial.is_none()
                && matches!(self.layout, OddSaveLayout::KiB8 | OddSaveLayout::KiB32)
    }

    pub fn unchanged_upper_control(&self, value: u8) -> bool {
        !self.partial_switched()
            && value == 1
            && self.start == SAVE_START
            && self.protectable()
            && self.enabled
            && !self.write_protected
    }

    pub fn accepts_control_address(&self, address: u32) -> bool {
        !self.expansion_bytes()
            && !self.reserved_switched()
            && !self.bounded_odd()
            && !self.layout.split_serial()
            && (address == SAVE_CONTROL
                || self.time_latch && (SAVE_CONTROL..=SAVE_CONTROL_END).contains(&address))
    }

    pub fn reset_mapping(&mut self, now: u64) {
        self.enabled = self.initially_enabled;
        self.write_protected = false;
        if let Some(serial) = &mut self.serial {
            if serial.separate_address() {
                serial.reset_clock(now);
            } else {
                *serial = Eeprom::default();
            }
        }
    }

    pub fn read(&self, address: u32) -> Option<u8> {
        if self.expansion_bytes() {
            return (self.enabled && self.contains(address))
                .then(|| self.bytes[(address - self.start) as usize]);
        }
        if self.bounded_bytes() {
            return (self.enabled && bounded_bytes::byte(address))
                .then(|| self.bytes[self.cell(address)]);
        }
        if self.reserved_switched() {
            return None;
        }
        if self.bounded_switched() && !bounded_switched::byte(address) {
            return None;
        }
        if self.bounded_odd() && !bounded_odd::byte(address) {
            return None;
        }
        if self.rom_visible() {
            return None;
        }
        if let Some(serial) = &self.serial {
            return (address & 1 != 0).then(|| serial.output());
        }
        self.enabled.then(|| {
            if address & 1 == 0 {
                0xff
            } else {
                self.bytes[self.cell(address)]
            }
        })
    }

    pub fn write(&mut self, address: u32, value: u8, now: u64, master_hz: u64) {
        if self.expansion_bytes() {
            if self.enabled && !self.write_protected && self.contains(address) {
                self.bytes[(address - self.start) as usize] = value;
            }
            return;
        }
        if self.bounded_bytes() {
            if self.enabled && !self.write_protected && bounded_bytes::byte(address) {
                let index = self.cell(address);
                self.bytes[index] = value;
            }
            return;
        }
        if self.reserved_switched() {
            return;
        }
        if self.bounded_switched() && (!self.enabled || !bounded_switched::byte(address)) {
            return;
        }
        if self.bounded_odd() && !bounded_odd::byte(address) {
            return;
        }
        if address & 1 != 0 && !self.write_protected {
            if let Some(serial) = &mut self.serial {
                serial.write(value, now, master_hz, &mut self.bytes);
            } else {
                let index = self.cell(address);
                self.bytes[index] = value;
            }
        }
    }

    fn cell(&self, address: u32) -> usize {
        if self.bounded_bytes() {
            return (address - self.start) as usize;
        }
        let index = (address - self.start) as usize / 2;
        if self.bounded_odd() || self.partial_switched() {
            index
        } else {
            index & (self.layout.bytes() - 1)
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg(test)]
fn rom_overlay_admits_nineteen_layouts_with_constructor_bound_special_owners() {
    let admitted = [
        OddSaveLayout::Bytes512,
        OddSaveLayout::KiB8,
        OddSaveLayout::FixedOddKiB8,
        OddSaveLayout::HeaderlessOddKiB8,
        OddSaveLayout::KiB32,
        OddSaveLayout::Parallel2048,
        OddSaveLayout::BoundedParallel2048,
        OddSaveLayout::Serial128,
        OddSaveLayout::Serial256,
        OddSaveLayout::SplitSerial256,
        OddSaveLayout::SplitSerial512,
        OddSaveLayout::SplitSerial2048,
        OddSaveLayout::SplitSerial8192,
        OddSaveLayout::ExpansionBytesKiB64,
        OddSaveLayout::BoundedSwitchedBytes2048,
        OddSaveLayout::BoundedOdd2400,
        OddSaveLayout::BoundedSwitchedOddKiB8,
        OddSaveLayout::ReservedSwitchedRom,
        OddSaveLayout::BoundedSplitKiB32,
    ];
    for layout in admitted {
        let mut save = headerless_odd8::fixture();
        save.layout = layout;
        let mut bytes = vec![0; 0x10000];
        bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
        let mut cart = crate::cartridge::Cartridge::new(bytes).unwrap();
        let cart_expected = save.special_rom_overlay_layout().is_none();
        cart.save = Some(save);
        assert_eq!(cart.supports_rom_overlay(), cart_expected, "{layout:?}");
        let before = cart.clone();
        let prepared =
            cart.prepare_rom_overlay(&[zeff_emu_common::cheats::CheatPatch::WideRomWrite {
                address: 0x1000,
                value: zeff_emu_common::cheats::CheatValue::Constant(0x5a),
            }]);
        assert_eq!(prepared.is_ok(), cart_expected, "{layout:?}");
        if let Ok(overlay) = prepared {
            cart.rom_overlay = overlay;
            assert_eq!(cart.read(0x1000), Some(0x5a), "{layout:?}");
        } else {
            assert_eq!(cart, before);
        }
    }
}
