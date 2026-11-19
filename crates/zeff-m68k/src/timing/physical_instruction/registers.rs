use crate::Registers;
use crate::ea::Size;

#[derive(Clone, Copy)]
pub(super) enum MoveRegister {
    Data(usize),
    Address(usize),
}

impl MoveRegister {
    pub(super) fn read(self, registers: &Registers) -> u32 {
        match self {
            Self::Data(index) => registers.d[index],
            Self::Address(index) => registers.address_register(index).unwrap(),
        }
    }

    pub(super) fn publish(self, registers: &mut Registers, size: Size, value: u32) {
        match self {
            Self::Data(index) => {
                registers.d[index] = (registers.d[index] & !size.mask()) | size.truncate(value);
                crate::alu::logic(registers, size, value);
            }
            Self::Address(index) => {
                registers.set_address_register(index, size.sign_extend(value));
            }
        }
    }
}
