use std::collections::BTreeMap;

use crate::bus::{
    AccessKind, BusAccess, BusResponse, ByteLanes, Direction, FunctionCode, M68kBus, RmwError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub access: BusAccess,
    pub direction: Direction,
    pub value: u16,
}

#[derive(Default)]
pub struct TestBus {
    pub(crate) memory: BTreeMap<u32, u8>,
    pub transfers: Vec<Transfer>,
    pub fail_at: Option<usize>,
    pub support_reset: bool,
    pub reset_failure: bool,
    pub reset_pulses: Vec<u32>,
    pub reset_active: bool,
    pub reset_edges: Vec<bool>,
    pub support_tas: bool,
    pub tas_write: bool,
    pub tas_error_after_write: bool,
    pub locked: bool,
    pub lock_events: Vec<(usize, bool)>,
}

impl TestBus {
    pub fn seed(&mut self, address: u32, bytes: &[u8]) {
        for (offset, &byte) in bytes.iter().enumerate() {
            self.memory.insert(
                address.wrapping_add(u32::try_from(offset).unwrap()) & 0x00ff_ffff,
                byte,
            );
        }
    }

    pub fn word(&mut self, address: u32, value: u16) {
        self.seed(address, &value.to_be_bytes());
    }

    pub fn peek(&self, address: u32) -> u8 {
        self.memory
            .get(&(address & 0x00ff_ffff))
            .copied()
            .unwrap_or(0)
    }

    fn transfer(&mut self, transfer: Transfer) -> Result<(), usize> {
        let index = self.transfers.len();
        self.transfers.push(transfer);
        if self.fail_at == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

impl M68kBus for TestBus {
    type Error = usize;

    fn read(&mut self, access: BusAccess) -> Result<u16, Self::Error> {
        let value = u16::from_be_bytes([
            self.peek(access.bus_address),
            self.peek(access.bus_address + 1),
        ]);
        self.transfer(Transfer {
            access,
            direction: Direction::Read,
            value,
        })?;
        Ok(value)
    }

    fn write(&mut self, access: BusAccess, value: u16) -> Result<(), Self::Error> {
        self.transfer(Transfer {
            access,
            direction: Direction::Write,
            value,
        })?;
        let [upper, lower] = value.to_be_bytes();
        if matches!(access.lanes, ByteLanes::Upper | ByteLanes::Both) {
            self.memory.insert(access.bus_address, upper);
        }
        if matches!(access.lanes, ByteLanes::Lower | ByteLanes::Both) {
            self.memory.insert(access.bus_address + 1, lower);
        }
        Ok(())
    }

    fn reset_output(&mut self, asserted_clocks: u32) -> Result<BusResponse<()>, Self::Error> {
        if !self.support_reset {
            return Ok(BusResponse::Unsupported);
        }
        self.reset_active = true;
        self.reset_edges.push(true);
        self.reset_pulses.push(asserted_clocks);
        self.reset_active = false;
        self.reset_edges.push(false);
        if self.reset_failure {
            return Err(0);
        }
        Ok(BusResponse::Complete(()))
    }

    fn test_and_set(
        &mut self,
        access: BusAccess,
    ) -> Result<BusResponse<u16>, RmwError<Self::Error>> {
        if !self.support_tas {
            return Ok(BusResponse::Unsupported);
        }
        self.locked = true;
        self.lock_events.push((self.transfers.len(), true));
        let result = (|| {
            let value = self.read(access).map_err(RmwError::Read)?;
            let bit = if access.lanes == ByteLanes::Upper {
                0x8000
            } else {
                0x80
            };
            if self.tas_write {
                self.write(access, value | bit).map_err(RmwError::Write)?;
            } else {
                self.transfer(Transfer {
                    access,
                    direction: Direction::Write,
                    value: value | bit,
                })
                .map_err(RmwError::Write)?;
            }
            if self.tas_error_after_write {
                return Err(RmwError::Write(self.transfers.len() - 1));
            }
            Ok(BusResponse::Complete(value))
        })();
        self.locked = false;
        self.lock_events.push((self.transfers.len(), false));
        result
    }
}

pub fn access(
    logical_address: u32,
    bus_address: u32,
    function_code: FunctionCode,
    kind: AccessKind,
    lanes: ByteLanes,
) -> BusAccess {
    BusAccess {
        logical_address,
        bus_address,
        function_code,
        kind,
        lanes,
    }
}
