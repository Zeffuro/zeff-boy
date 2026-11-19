use super::*;

impl Bus {
    pub(super) fn write_psg(&mut self, access: BusAccess, value: u16) -> bool {
        let address = access.bus_address;
        if address & VDP_DECODE_MASK != VDP_DECODE_VALUE
            || !(PSG_PORT_START..=PSG_PORT_END).contains(&(address & VDP_PORT_MASK))
        {
            return false;
        }
        if access.lanes != ByteLanes::Upper {
            self.audio.write_psg(self.now.get(), value as u8);
        }
        true
    }
}
