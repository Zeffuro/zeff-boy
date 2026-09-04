#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum SaveRamKind {
    None,
    KnownVolatile { size: usize },
    KnownBatteryBacked { size: usize },
    MapperRamUnknown { size: usize },
}

impl SaveRamKind {
    pub const fn none() -> Self {
        Self::None
    }

    pub const fn known_battery_backed(size: usize) -> Self {
        Self::KnownBatteryBacked { size }
    }

    pub const fn known_volatile(size: usize) -> Self {
        Self::KnownVolatile { size }
    }

    pub const fn mapper_ram_unknown(size: usize) -> Self {
        Self::MapperRamUnknown { size }
    }

    pub const fn is_battery_backed(self) -> bool {
        matches!(self, Self::KnownBatteryBacked { .. })
    }

    pub const fn has_ram(self) -> bool {
        !matches!(self, Self::None)
    }

    pub const fn size(self) -> usize {
        match self {
            Self::None => 0,
            Self::KnownVolatile { size }
            | Self::KnownBatteryBacked { size }
            | Self::MapperRamUnknown { size } => size,
        }
    }
}
