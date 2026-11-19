use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugSavePorts {
    pub data_start: u32,
    pub data_end: u32,
    pub control: u32,
    pub enabled: bool,
    pub write_protected: bool,
}

impl DebugSavePorts {
    pub fn accepts_byte(self, address: u32) -> bool {
        (self.data_start..=self.data_end).contains(&address) || address == self.control
    }
}

impl Emulator {
    pub fn debug_save_ports(&self) -> Option<DebugSavePorts> {
        if self.bus.tmss.is_some() || !self.bus.cartridge.timed_save_owner() {
            return None;
        }
        let save = self.bus.cartridge.save.as_ref()?;
        Some(DebugSavePorts {
            data_start: 0x200000,
            data_end: 0x203fff,
            control: 0xa130f1,
            enabled: save.enabled,
            write_protected: save.write_protected,
        })
    }

    pub(super) fn save_port_transfer(&self, write: DebugPortWrite) -> Option<DebugDataWrite> {
        let DebugPortWrite::Byte { address, value } = write else {
            return None;
        };
        self.debug_save_ports()?
            .accepts_byte(address)
            .then_some(DebugDataWrite::Byte { address, value })
    }
}
