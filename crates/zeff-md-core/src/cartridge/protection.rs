use super::CartridgeHeader;
use zeff_m68k::bus::ByteLanes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadLayout {
    Sequential,
    SparseUpperBytePair,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FixedProtection {
    values: [u8; 4],
    count: u8,
    layout: ReadLayout,
}

impl FixedProtection {
    pub fn detect(size: usize, header: Option<&CartridgeHeader>) -> Option<Self> {
        let header = header?;
        let (values, count, layout) = match (size, header.checksum, header.calculated_checksum) {
            (0x100000, 0x0080, 0x3dba) => ([0x55, 0x0f, 0xc9, 0x18], 4, ReadLayout::Sequential),
            (0x100000, 0x0000, 0x5843) => ([0x55, 0x0f, 0xaa, 0xf0], 4, ReadLayout::Sequential),
            (0x100000, 0x0000, 0x16cd) => ([0x55, 0x0f, 0, 0], 2, ReadLayout::Sequential),
            (0x200000, 0xffff, 0xd472) => ([0x63, 0x98, 0xc9, 0x18], 4, ReadLayout::Sequential),
            (0x100000, 0x0000, 0x7037)
                if header.copyright == "(C)1995 CREATON." && header.product == "MDGM-0001-0001" =>
            {
                ([0x90, 0xd3, 0, 0], 2, ReadLayout::SparseUpperBytePair)
            }
            _ => return None,
        };
        Some(Self {
            values,
            count,
            layout,
        })
    }

    pub fn contains(&self, address: u32) -> bool {
        (0x400000..=0x40ffff).contains(&address)
    }

    pub fn read(&self, address: u32) -> Option<u8> {
        if self.layout == ReadLayout::SparseUpperBytePair {
            return match address {
                0x400000 => Some(self.values[0]),
                0x401000 => Some(self.values[1]),
                _ => None,
            };
        }
        let offset = address.checked_sub(0x400000)?;
        (offset & 1 == 0 && offset / 2 < u32::from(self.count))
            .then(|| self.values[(offset / 2) as usize])
    }

    pub fn read_cycle(&self, address: u32, lanes: ByteLanes) -> Option<u16> {
        if self.layout == ReadLayout::SparseUpperBytePair && lanes != ByteLanes::Upper {
            return None;
        }
        self.read(address)
            .map(|value| u16::from(value) << if lanes == ByteLanes::Upper { 8 } else { 0 })
    }

    pub fn rejects_missing_byte(&self, address: u32) -> bool {
        self.layout == ReadLayout::SparseUpperBytePair
            && self.contains(address)
            && self.read(address).is_none()
    }

    pub fn rejects_write(&self, address: u32) -> bool {
        self.layout == ReadLayout::SparseUpperBytePair && self.contains(address)
    }
}

#[cfg(test)]
pub(crate) mod tests;
