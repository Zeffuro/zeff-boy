#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum SaveRamKind {
    None,
    KnownVolatile { size: usize },
    KnownBatteryBacked { size: usize },
    KnownNonVolatile { size: usize },
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

    pub const fn known_nonvolatile(size: usize) -> Self {
        Self::KnownNonVolatile { size }
    }

    pub const fn mapper_ram_unknown(size: usize) -> Self {
        Self::MapperRamUnknown { size }
    }

    pub const fn is_battery_backed(self) -> bool {
        matches!(self, Self::KnownBatteryBacked { .. })
    }

    pub const fn is_persistent(self) -> bool {
        matches!(
            self,
            Self::KnownBatteryBacked { .. } | Self::KnownNonVolatile { .. }
        )
    }

    pub const fn has_ram(self) -> bool {
        !matches!(self, Self::None)
    }

    pub const fn size(self) -> usize {
        match self {
            Self::None => 0,
            Self::KnownVolatile { size }
            | Self::KnownBatteryBacked { size }
            | Self::KnownNonVolatile { size }
            | Self::MapperRamUnknown { size } => size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SaveRamKind;

    #[test]
    fn persistence_and_battery_classification_are_independent() {
        for (kind, ram, persistent, battery, size) in [
            (SaveRamKind::none(), false, false, false, 0),
            (SaveRamKind::known_volatile(512), true, false, false, 512),
            (
                SaveRamKind::known_battery_backed(512),
                true,
                true,
                true,
                512,
            ),
            (SaveRamKind::known_nonvolatile(512), true, true, false, 512),
            (
                SaveRamKind::mapper_ram_unknown(512),
                true,
                false,
                false,
                512,
            ),
        ] {
            assert_eq!(kind.has_ram(), ram);
            assert_eq!(kind.is_persistent(), persistent);
            assert_eq!(kind.is_battery_backed(), battery);
            assert_eq!(kind.size(), size);
        }
    }
}
