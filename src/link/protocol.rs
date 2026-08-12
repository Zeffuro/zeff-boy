pub(crate) const LINK_PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LinkEndpointId(pub(crate) u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LinkTransferId(pub(crate) u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum LinkSystemType {
    GameBoy = 1,
    GameGear = 2,
    WonderSwan = 3,
}

impl TryFrom<u8> for LinkSystemType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::GameBoy),
            2 => Ok(Self::GameGear),
            3 => Ok(Self::WonderSwan),
            _ => Err(()),
        }
    }
}
