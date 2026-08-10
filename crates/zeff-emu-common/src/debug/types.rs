use crate::address::Address;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchType {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Debug)]
pub struct AddressWatchpoint {
    pub address: Address,
    pub watch_type: WatchType,
    pub last_value: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct AddressWatchHit {
    pub address: Address,
    pub old_value: u8,
    pub new_value: u8,
    pub watch_type: WatchType,
}

#[derive(Clone, Debug)]
pub struct Watchpoint {
    pub address: u16,
    pub watch_type: WatchType,
    pub last_value: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct WatchHit {
    pub address: u16,
    pub old_value: u8,
    pub new_value: u8,
    pub watch_type: WatchType,
}
