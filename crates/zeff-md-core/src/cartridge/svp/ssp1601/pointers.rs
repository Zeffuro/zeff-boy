use super::decode::Pointer;
use super::{Core, Fault};

impl Core {
    pub(super) fn pointer_address(
        &mut self,
        pointer: Pointer,
        writing: bool,
    ) -> Result<(usize, usize), Fault> {
        let index = usize::from(pointer.index);
        let bank = index / 4;
        let modifier = pointer.modifier;
        if index & 3 == 3 {
            return Ok((bank, usize::from(modifier)));
        }
        let address = self.pointers[index];
        let rpl = (self.st & 7) as u8;
        if writing && rpl != 0 && modifier >= 2 {
            return Err(Fault::PointerMode {
                index: pointer.index,
                modifier,
            });
        }
        let next = match modifier {
            0 => address,
            1 => address.wrapping_add(1),
            2 | 3 => {
                let mask = if rpl == 0 {
                    255
                } else {
                    ((1u16 << rpl) - 1) as u8
                };
                let low = if modifier == 2 {
                    address.wrapping_sub(1)
                } else {
                    address.wrapping_add(1)
                };
                (address & !mask) | (low & mask)
            }
            _ => {
                return Err(Fault::PointerMode {
                    index: pointer.index,
                    modifier,
                });
            }
        };
        self.pointers[index] = next;
        Ok((bank, usize::from(address)))
    }

    pub(super) fn pointer_read(&self, index: u8) -> Result<u16, Fault> {
        if index & 3 == 3 {
            return Err(Fault::PointerMode { index, modifier: 0 });
        }
        Ok(u16::from(self.pointers[usize::from(index)]))
    }

    pub(super) fn pointer_write(&mut self, index: u8, value: u16) -> Result<(), Fault> {
        if index & 3 == 3 {
            return Err(Fault::PointerMode { index, modifier: 0 });
        }
        self.pointers[usize::from(index)] = value as u8;
        Ok(())
    }

    pub(super) fn double_address(&self, pointer: Pointer) -> Result<(usize, usize), Fault> {
        let index = usize::from(pointer.index);
        let address = if index & 3 == 3 {
            pointer.modifier
        } else {
            if pointer.modifier != 0 {
                return Err(Fault::PointerMode {
                    index: pointer.index,
                    modifier: pointer.modifier,
                });
            }
            self.pointers[index]
        };
        Ok((index / 4, usize::from(address)))
    }
}
