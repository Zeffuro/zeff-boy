use super::super::service::ServiceResource;
use super::{Controller, Fault};

impl Controller {
    pub(in super::super) fn service_target(
        &self,
        index: u8,
        status: u16,
        blind: bool,
        writing: bool,
    ) -> Result<(ServiceResource, u32, u16), Fault> {
        self.preflight(index, blind)?;
        let local = blind
            || index == 6
            || (!writing && index == 5)
            || !Self::memory_port(index, status, writing);
        if local {
            return Ok((ServiceResource::Controller, u32::from(index), 0));
        }
        let channel = self.channel(index, writing)?;
        let resource = match channel.address {
            0..=0x0fffff => ServiceResource::SharedRom,
            0x180000..=0x18ffff => ServiceResource::SharedDram,
            0x1c8000..=0x1c83ff => ServiceResource::ExternalIram,
            _ => return Err(Self::failure(index, "unmapped service resource")),
        };
        Ok((resource, channel.address, channel.mode))
    }
}
