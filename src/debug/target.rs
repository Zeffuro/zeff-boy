#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DebugTarget {
    #[default]
    Primary,
    Sound,
}

impl DebugTarget {
    pub(crate) fn cpu_space(self) -> crate::symbols::AddressSpaceId {
        crate::symbols::AddressSpaceId(u16::from(self == Self::Sound))
    }
}
