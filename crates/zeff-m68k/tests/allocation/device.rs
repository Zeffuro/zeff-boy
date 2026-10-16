use super::{
    BusAccess, BusResponse, ByteLanes, ClockedBus, CpuClocks, CycleResult, M68kBus, RmwError,
};

pub(super) struct LoopBus;

pub(super) struct MemoryBus(pub(super) [u8; 0x8000]);

impl MemoryBus {
    pub(super) fn word(&mut self, address: usize, value: u16) {
        self.0[address..address + 2].copy_from_slice(&value.to_be_bytes());
    }
}

impl M68kBus for MemoryBus {
    type Error = ();

    fn read(&mut self, access: BusAccess) -> Result<u16, Self::Error> {
        let address = access.bus_address as usize;
        self.0
            .get(address..address + 2)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
            .ok_or(())
    }

    fn write(&mut self, access: BusAccess, value: u16) -> Result<(), Self::Error> {
        let address = access.bus_address as usize;
        let bytes = self.0.get_mut(address..address + 2).ok_or(())?;
        if access.lanes != ByteLanes::Lower {
            bytes[0] = (value >> 8) as u8;
        }
        if access.lanes != ByteLanes::Upper {
            bytes[1] = value as u8;
        }
        Ok(())
    }

    fn reset_output(&mut self, asserted_clocks: u32) -> Result<BusResponse<()>, Self::Error> {
        if asserted_clocks != 124 {
            return Err(());
        }
        Ok(BusResponse::Complete(()))
    }

    fn test_and_set(
        &mut self,
        access: BusAccess,
    ) -> Result<BusResponse<u16>, RmwError<Self::Error>> {
        let value = self.read(access).map_err(RmwError::Read)?;
        let mask = if access.lanes == ByteLanes::Upper {
            0x8000
        } else {
            0x80
        };
        self.write(access, value | mask).map_err(RmwError::Write)?;
        Ok(BusResponse::Complete(value))
    }
}

impl M68kBus for LoopBus {
    type Error = ();

    fn read(&mut self, access: BusAccess) -> Result<u16, Self::Error> {
        match access.bus_address {
            0 => Ok(0x7081),
            2 => Ok(0x4e71),
            4 => Ok(0x60fa),
            _ => Err(()),
        }
    }

    fn write(&mut self, _access: BusAccess, _value: u16) -> Result<(), Self::Error> {
        Err(())
    }
}

impl ClockedBus for LoopBus {
    type Error = ();
    fn read_cycle(&mut self, access: BusAccess, _: CpuClocks) -> CycleResult<u16, ()> {
        CycleResult {
            result: Ok(self.read(access).unwrap_or(0x4e71)),
            wait_clocks: 0,
        }
    }
    fn write_cycle(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> CycleResult<(), ()> {
        CycleResult {
            result: Err(()),
            wait_clocks: 0,
        }
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {}
}

impl ClockedBus for MemoryBus {
    type Error = ();
    fn read_cycle(&mut self, access: BusAccess, _: CpuClocks) -> CycleResult<u16, ()> {
        CycleResult {
            result: self.read(access),
            wait_clocks: 0,
        }
    }
    fn write_cycle(&mut self, access: BusAccess, value: u16, _: CpuClocks) -> CycleResult<(), ()> {
        CycleResult {
            result: self.write(access, value),
            wait_clocks: 0,
        }
    }
    fn idle(&mut self, _: CpuClocks, _: u32) {}
}
