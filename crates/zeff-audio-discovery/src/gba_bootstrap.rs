// Reserved bootstrap words pause the driver until the host PCM session is ready.
pub const READY_IWRAM_OFFSET: usize = 0x7fe0;
pub const READY_ADDRESS: u32 = 0x0300_0000 + READY_IWRAM_OFFSET as u32;
pub const READY_VALUE: u32 = u32::from_be_bytes(*b"AUDI");
pub const ACK_ADDRESS: u32 = READY_ADDRESS + size_of::<u32>() as u32;
pub const ACK_VALUE: u32 = 1;
