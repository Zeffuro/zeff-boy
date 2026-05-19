pub type Address = u32;

#[inline]
pub fn narrow_u16(addr: Address) -> u16 {
    addr as u16
}
