use super::cartridge::Sega8MapperKind;
use super::constants::{
    MAPPER_FRAME_CONTROL, MAPPER_FRAME_CONTROL_CART_RAM_BANK_SELECT,
    MAPPER_FRAME_CONTROL_CART_RAM_ENABLE, MAPPER_SLOT0_BANK, MAPPER_SLOT1_BANK, MAPPER_SLOT2_BANK,
    ROM_PAGE_8K_SIZE, SLOT0_END, SLOT0_START, SLOT1_END, SLOT1_START, SLOT2_END, SLOT2_START,
    SMS_CARTRIDGE_RAM_BANK_SIZE,
};

pub const CODEMASTERS_RAM_START: u16 = 0xA000;
pub const CODEMASTERS_RAM_END: u16 = 0xBFFF;
const KOREAN_SLOT2_BANK_REGISTER: u16 = 0xA000;
const MSX_SLOT2_REGISTER: u16 = 0x0000;
const MSX_SLOT3_REGISTER: u16 = 0x0001;
const MSX_SLOT0_REGISTER: u16 = 0x0002;
const MSX_SLOT1_REGISTER: u16 = 0x0003;
const JANGGUN_PAGE0_REGISTER: u16 = 0x4000;
const JANGGUN_PAGE1_REGISTER: u16 = 0x6000;
const JANGGUN_PAGE2_REGISTER: u16 = 0x8000;
const JANGGUN_PAGE3_REGISTER: u16 = 0xA000;
const JANGGUN_SLOT01_REGISTER: u16 = 0xFFFE;
const JANGGUN_SLOT23_REGISTER: u16 = 0xFFFF;
const JANGGUN_BANK_MASK: u8 = 0x3F;
const JANGGUN_REVERSE_READ_BIT: u8 = 0x40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegaMapper {
    kind: Sega8MapperKind,
    frame_control: u8,
    slot_banks: [u8; 3],
}

impl Default for SegaMapper {
    fn default() -> Self {
        Self::new(Sega8MapperKind::Sega)
    }
}

impl SegaMapper {
    pub(crate) fn new(kind: Sega8MapperKind) -> Self {
        Self {
            kind,
            frame_control: default_frame_control(kind),
            slot_banks: default_slot_banks(kind),
        }
    }

    pub(crate) fn from_state(
        kind: Sega8MapperKind,
        frame_control: u8,
        slot_banks: [u8; 3],
    ) -> Self {
        Self {
            kind,
            frame_control,
            slot_banks,
        }
    }

    pub fn kind(self) -> Sega8MapperKind {
        self.kind
    }

    pub fn kind_label(self) -> &'static str {
        self.kind.label()
    }

    pub fn frame_control(self) -> u8 {
        self.frame_control
    }

    pub fn slot_banks(self) -> [u8; 3] {
        self.slot_banks
    }

    pub fn slot0_bank(self) -> u8 {
        self.slot_banks[0]
    }

    pub fn slot1_bank(self) -> u8 {
        match self.kind {
            Sega8MapperKind::Codemasters => self.slot_banks[1] & 0x7F,
            Sega8MapperKind::Sega
            | Sega8MapperKind::Korean
            | Sega8MapperKind::Msx
            | Sega8MapperKind::Nemesis
            | Sega8MapperKind::Janggun => self.slot_banks[1],
        }
    }

    pub fn slot2_bank(self) -> u8 {
        self.slot_banks[2]
    }

    pub fn slot2_cartridge_ram_enabled(self) -> bool {
        self.kind == Sega8MapperKind::Sega
            && self.frame_control & MAPPER_FRAME_CONTROL_CART_RAM_ENABLE != 0
    }

    pub fn codemasters_cartridge_ram_enabled(self) -> bool {
        self.kind == Sega8MapperKind::Codemasters && self.slot_banks[1] & 0x80 != 0
    }

    pub fn cartridge_ram_bank(self) -> usize {
        usize::from(self.frame_control & MAPPER_FRAME_CONTROL_CART_RAM_BANK_SELECT != 0)
    }

    pub(crate) fn slot2_cartridge_ram_offset(self, addr: u16) -> usize {
        self.cartridge_ram_bank() * SMS_CARTRIDGE_RAM_BANK_SIZE
            + usize::from(addr.wrapping_sub(SLOT2_START))
    }

    pub(crate) fn codemasters_cartridge_ram_offset(self, addr: u16) -> Option<usize> {
        (self.codemasters_cartridge_ram_enabled()
            && (CODEMASTERS_RAM_START..=CODEMASTERS_RAM_END).contains(&addr))
        .then(|| usize::from(addr.wrapping_sub(CODEMASTERS_RAM_START)))
    }

    pub(crate) fn rom_page_8k_mapping(
        self,
        addr: u16,
        page_count: usize,
    ) -> Option<(u8, u16, bool)> {
        match self.kind {
            Sega8MapperKind::Msx => msx_rom_page_mapping(addr, page_count, self.msx_page_banks()),
            Sega8MapperKind::Nemesis => {
                nemesis_rom_page_mapping(addr, page_count, self.msx_page_banks())
            }
            Sega8MapperKind::Janggun => janggun_rom_page_mapping(addr, self.msx_page_banks()),
            Sega8MapperKind::Sega | Sega8MapperKind::Codemasters | Sega8MapperKind::Korean => None,
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new(self.kind);
    }

    pub(crate) fn write_sega_register(&mut self, addr: u16, val: u8) {
        if self.kind != Sega8MapperKind::Sega {
            return;
        }

        match addr {
            MAPPER_FRAME_CONTROL => self.frame_control = val,
            MAPPER_SLOT0_BANK => self.slot_banks[0] = val,
            MAPPER_SLOT1_BANK => self.slot_banks[1] = val,
            MAPPER_SLOT2_BANK => self.slot_banks[2] = val,
            _ => {}
        }
    }

    pub(crate) fn write_codemasters_register(&mut self, addr: u16, val: u8) {
        if self.kind != Sega8MapperKind::Codemasters {
            return;
        }

        match addr {
            SLOT0_START..=SLOT0_END => self.slot_banks[0] = val,
            SLOT1_START..=SLOT1_END => self.slot_banks[1] = val,
            SLOT2_START..=SLOT2_END => self.slot_banks[2] = val,
            _ => {}
        }
    }

    pub(crate) fn write_korean_register(&mut self, addr: u16, val: u8) {
        if self.kind == Sega8MapperKind::Korean && addr == KOREAN_SLOT2_BANK_REGISTER {
            self.slot_banks[2] = val;
        }
    }

    pub(crate) fn write_msx_register(&mut self, addr: u16, val: u8) {
        if !matches!(self.kind, Sega8MapperKind::Msx | Sega8MapperKind::Nemesis) {
            return;
        }

        match addr {
            MSX_SLOT0_REGISTER => self.slot_banks[0] = val,
            MSX_SLOT1_REGISTER => self.slot_banks[1] = val,
            MSX_SLOT2_REGISTER => self.slot_banks[2] = val,
            MSX_SLOT3_REGISTER => self.frame_control = val,
            _ => {}
        }
    }

    pub(crate) fn write_janggun_register(&mut self, addr: u16, val: u8) {
        if self.kind != Sega8MapperKind::Janggun {
            return;
        }

        match addr {
            JANGGUN_PAGE0_REGISTER => self.slot_banks[0] = val,
            JANGGUN_PAGE1_REGISTER => self.slot_banks[1] = val,
            JANGGUN_PAGE2_REGISTER => self.slot_banks[2] = val,
            JANGGUN_PAGE3_REGISTER => self.frame_control = val,
            JANGGUN_SLOT01_REGISTER => {
                self.slot_banks[0] = val;
                self.slot_banks[1] = next_janggun_page_value(val);
            }
            JANGGUN_SLOT23_REGISTER => {
                self.slot_banks[2] = val;
                self.frame_control = next_janggun_page_value(val);
            }
            _ => {}
        }
    }

    fn msx_page_banks(self) -> [u8; 4] {
        [
            self.slot_banks[0],
            self.slot_banks[1],
            self.slot_banks[2],
            self.frame_control,
        ]
    }
}

fn default_slot_banks(kind: Sega8MapperKind) -> [u8; 3] {
    match kind {
        Sega8MapperKind::Sega | Sega8MapperKind::Korean => [0, 1, 2],
        Sega8MapperKind::Codemasters => [0, 1, 0],
        Sega8MapperKind::Msx | Sega8MapperKind::Nemesis | Sega8MapperKind::Janggun => [2, 3, 4],
    }
}

fn default_frame_control(kind: Sega8MapperKind) -> u8 {
    match kind {
        Sega8MapperKind::Msx | Sega8MapperKind::Nemesis | Sega8MapperKind::Janggun => 5,
        Sega8MapperKind::Sega | Sega8MapperKind::Codemasters | Sega8MapperKind::Korean => 0,
    }
}

fn msx_rom_page_mapping(
    addr: u16,
    _page_count: usize,
    page_banks: [u8; 4],
) -> Option<(u8, u16, bool)> {
    let offset = addr & (ROM_PAGE_8K_SIZE as u16 - 1);
    match addr {
        0x0000..=0x1FFF => Some((0, offset, false)),
        0x2000..=0x3FFF => Some((1, offset, false)),
        0x4000..=0x5FFF => Some((page_banks[0], offset, false)),
        0x6000..=0x7FFF => Some((page_banks[1], offset, false)),
        0x8000..=0x9FFF => Some((page_banks[2], offset, false)),
        0xA000..=0xBFFF => Some((page_banks[3], offset, false)),
        _ => None,
    }
}

fn nemesis_rom_page_mapping(
    addr: u16,
    page_count: usize,
    page_banks: [u8; 4],
) -> Option<(u8, u16, bool)> {
    let offset = addr & (ROM_PAGE_8K_SIZE as u16 - 1);
    match addr {
        0x0000..=0x1FFF => Some((page_count.saturating_sub(1) as u8, offset, false)),
        0x2000..=0x3FFF => Some((1, offset, false)),
        0x4000..=0x5FFF => Some((page_banks[0], offset, false)),
        0x6000..=0x7FFF => Some((page_banks[1], offset, false)),
        0x8000..=0x9FFF => Some((page_banks[2], offset, false)),
        0xA000..=0xBFFF => Some((page_banks[3], offset, false)),
        _ => None,
    }
}

fn janggun_rom_page_mapping(addr: u16, page_banks: [u8; 4]) -> Option<(u8, u16, bool)> {
    let offset = addr & (ROM_PAGE_8K_SIZE as u16 - 1);
    match addr {
        0x0000..=0x1FFF => Some((0, offset, false)),
        0x2000..=0x3FFF => Some((1, offset, false)),
        0x4000..=0x5FFF => Some(janggun_page_mapping(page_banks[0], offset)),
        0x6000..=0x7FFF => Some(janggun_page_mapping(page_banks[1], offset)),
        0x8000..=0x9FFF => Some(janggun_page_mapping(page_banks[2], offset)),
        0xA000..=0xBFFF => Some(janggun_page_mapping(page_banks[3], offset)),
        _ => None,
    }
}

fn janggun_page_mapping(raw: u8, offset: u16) -> (u8, u16, bool) {
    (
        raw & JANGGUN_BANK_MASK,
        offset,
        raw & JANGGUN_REVERSE_READ_BIT != 0,
    )
}

fn next_janggun_page_value(raw: u8) -> u8 {
    (raw & JANGGUN_REVERSE_READ_BIT) | (raw.wrapping_add(1) & JANGGUN_BANK_MASK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::constants::CODEMASTERS_CARTRIDGE_RAM_SIZE;

    #[test]
    fn default_banks_follow_mapper_kind() {
        assert_eq!(
            SegaMapper::new(Sega8MapperKind::Sega).slot_banks(),
            [0, 1, 2]
        );
        assert_eq!(
            SegaMapper::new(Sega8MapperKind::Codemasters).slot_banks(),
            [0, 1, 0]
        );
        assert_eq!(
            SegaMapper::new(Sega8MapperKind::Korean).slot_banks(),
            [0, 1, 2]
        );
        assert_eq!(
            SegaMapper::new(Sega8MapperKind::Msx).slot_banks(),
            [2, 3, 4]
        );
        assert_eq!(SegaMapper::new(Sega8MapperKind::Msx).frame_control(), 5);
        assert_eq!(
            SegaMapper::new(Sega8MapperKind::Nemesis).slot_banks(),
            [2, 3, 4]
        );
        assert_eq!(SegaMapper::new(Sega8MapperKind::Nemesis).frame_control(), 5);
        assert_eq!(
            SegaMapper::new(Sega8MapperKind::Janggun).slot_banks(),
            [2, 3, 4]
        );
        assert_eq!(SegaMapper::new(Sega8MapperKind::Janggun).frame_control(), 5);
    }

    #[test]
    fn sega_frame_control_selects_cartridge_ram_bank() {
        let mut mapper = SegaMapper::new(Sega8MapperKind::Sega);

        mapper.write_sega_register(
            MAPPER_FRAME_CONTROL,
            MAPPER_FRAME_CONTROL_CART_RAM_ENABLE | MAPPER_FRAME_CONTROL_CART_RAM_BANK_SELECT,
        );

        assert!(mapper.slot2_cartridge_ram_enabled());
        assert_eq!(mapper.cartridge_ram_bank(), 1);
        assert_eq!(
            mapper.slot2_cartridge_ram_offset(SLOT2_START),
            SMS_CARTRIDGE_RAM_BANK_SIZE
        );
    }

    #[test]
    fn codemasters_slot1_high_bit_enables_eight_kilobyte_ram_overlay() {
        let mut mapper = SegaMapper::new(Sega8MapperKind::Codemasters);

        mapper.write_codemasters_register(SLOT1_START, 0x83);

        assert_eq!(mapper.slot1_bank(), 3);
        assert!(mapper.codemasters_cartridge_ram_enabled());
        assert_eq!(mapper.codemasters_cartridge_ram_offset(0x9FFF), None);
        assert_eq!(mapper.codemasters_cartridge_ram_offset(0xA000), Some(0));
        assert_eq!(
            mapper.codemasters_cartridge_ram_offset(0xBFFF),
            Some(CODEMASTERS_CARTRIDGE_RAM_SIZE - 1)
        );
    }

    #[test]
    fn korean_mapper_switches_slot2_from_a000_only() {
        let mut mapper = SegaMapper::new(Sega8MapperKind::Korean);

        mapper.write_korean_register(0x9FFF, 3);
        assert_eq!(mapper.slot_banks(), [0, 1, 2]);

        mapper.write_korean_register(KOREAN_SLOT2_BANK_REGISTER, 3);
        assert_eq!(mapper.slot_banks(), [0, 1, 3]);

        mapper.write_sega_register(MAPPER_SLOT1_BANK, 2);
        mapper.write_codemasters_register(SLOT0_START, 2);
        assert_eq!(mapper.slot_banks(), [0, 1, 3]);
    }

    #[test]
    fn msx_like_mapper_registers_control_four_8k_pages() {
        let mut mapper = SegaMapper::new(Sega8MapperKind::Msx);

        mapper.write_msx_register(0x0000, 7);
        mapper.write_msx_register(0x0001, 6);
        mapper.write_msx_register(0x0002, 5);
        mapper.write_msx_register(0x0003, 4);

        assert_eq!(mapper.rom_page_8k_mapping(0x0000, 8), Some((0, 0, false)));
        assert_eq!(mapper.rom_page_8k_mapping(0x2000, 8), Some((1, 0, false)));
        assert_eq!(mapper.rom_page_8k_mapping(0x4000, 8), Some((5, 0, false)));
        assert_eq!(mapper.rom_page_8k_mapping(0x6000, 8), Some((4, 0, false)));
        assert_eq!(mapper.rom_page_8k_mapping(0x8000, 8), Some((7, 0, false)));
        assert_eq!(
            mapper.rom_page_8k_mapping(0xA123, 8),
            Some((6, 0x0123, false))
        );
    }

    #[test]
    fn nemesis_mapper_fixes_zero_page_to_last_8k_rom_page() {
        let mapper = SegaMapper::new(Sega8MapperKind::Nemesis);

        assert_eq!(mapper.rom_page_8k_mapping(0x0000, 8), Some((7, 0, false)));
        assert_eq!(mapper.rom_page_8k_mapping(0x2000, 8), Some((1, 0, false)));
        assert_eq!(mapper.rom_page_8k_mapping(0x4000, 8), Some((2, 0, false)));
        assert_eq!(mapper.rom_page_8k_mapping(0xA000, 8), Some((5, 0, false)));
    }

    #[test]
    fn janggun_mapper_registers_control_pages_and_reverse_flag() {
        let mut mapper = SegaMapper::new(Sega8MapperKind::Janggun);

        mapper.write_janggun_register(0x4000, 0x46);
        mapper.write_janggun_register(0x6000, 0x07);
        mapper.write_janggun_register(0x8000, 0x48);
        mapper.write_janggun_register(0xA000, 0x09);

        assert_eq!(mapper.rom_page_8k_mapping(0x4000, 16), Some((6, 0, true)));
        assert_eq!(mapper.rom_page_8k_mapping(0x6000, 16), Some((7, 0, false)));
        assert_eq!(mapper.rom_page_8k_mapping(0x8000, 16), Some((8, 0, true)));
        assert_eq!(mapper.rom_page_8k_mapping(0xA000, 16), Some((9, 0, false)));

        mapper.write_janggun_register(0xFFFE, 0x42);
        mapper.write_janggun_register(0xFFFF, 0x04);

        assert_eq!(mapper.rom_page_8k_mapping(0x4000, 16), Some((2, 0, true)));
        assert_eq!(mapper.rom_page_8k_mapping(0x6000, 16), Some((3, 0, true)));
        assert_eq!(mapper.rom_page_8k_mapping(0x8000, 16), Some((4, 0, false)));
        assert_eq!(mapper.rom_page_8k_mapping(0xA000, 16), Some((5, 0, false)));
    }
}
