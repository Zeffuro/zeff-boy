use super::*;
use crate::state::{Reader, Writer, read_index, write_option_u8};

impl BusState {
    pub(crate) fn write_continuation(&self, w: &mut Writer) {
        self.prefix.write_continuation(w);
        w.write_bool(self.transfer.is_some());
        let Some(transfer) = self.transfer else {
            return;
        };
        w.write_u16(transfer.pc);
        match transfer.operation {
            Operation::Single { opcode, index } => {
                w.write_u8(0);
                w.write_u8(opcode);
                write_option_u8(index, w);
            }
            Operation::Load(value) => {
                w.write_u8(1);
                value.write_continuation(w);
            }
            Operation::Stack(value) => {
                w.write_u8(2);
                value.write_continuation(w);
            }
            Operation::Rmw(value) => {
                w.write_u8(3);
                value.write_continuation(w);
            }
            Operation::IndexedMemory(value) => {
                w.write_u8(4);
                value.write_continuation(w);
            }
            Operation::IndexedCb(value) => {
                w.write_u8(5);
                value.write_continuation(w);
            }
            Operation::CbFetch => w.write_u8(6),
            Operation::EdFetch { pc } => {
                w.write_u8(7);
                w.write_u16(pc);
            }
            Operation::EdInternal(value) => {
                w.write_u8(8);
                value.write_continuation(w);
            }
            Operation::EdIo(value) => {
                w.write_u8(9);
                value.write_continuation(w);
            }
            Operation::EdBlock(value) => {
                w.write_u8(10);
                value.write_continuation(w);
            }
            Operation::Interrupt(value) => {
                w.write_u8(11);
                value.write_continuation(w);
            }
        }
    }

    pub(crate) fn read_continuation(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        let prefix = PrefixState::read_continuation(r)?;
        let transfer = if r.read_bool()? {
            let pc = r.read_u16()?;
            let operation = match r.read_u8()? {
                0 => {
                    let opcode = r.read_u8()?;
                    let index = if r.read_bool()? {
                        Some(read_index(r)?)
                    } else {
                        None
                    };
                    anyhow::ensure!(
                        single_transfer(opcode, index.is_some()),
                        "invalid Z80 single transfer opcode"
                    );
                    Operation::Single { opcode, index }
                }
                1 => Operation::Load(load::Load::read_continuation(r)?),
                2 => Operation::Stack(stack::Stack::read_continuation(r)?),
                3 => Operation::Rmw(rmw::Rmw::read_continuation(r)?),
                4 => Operation::IndexedMemory(indexed_memory::IndexedMemory::read_continuation(r)?),
                5 => Operation::IndexedCb(indexed_cb::IndexedCb::read_continuation(r)?),
                6 => Operation::CbFetch,
                7 => Operation::EdFetch { pc: r.read_u16()? },
                8 => Operation::EdInternal(ed::Internal::read_continuation(r)?),
                9 => Operation::EdIo(ed::Io::read_continuation(r)?),
                10 => Operation::EdBlock(ed::Block::read_continuation(r)?),
                11 => Operation::Interrupt(interrupt::Service::read_continuation(r)?),
                _ => anyhow::bail!("invalid Z80 bus operation tag"),
            };
            Some(Transfer { pc, operation })
        } else {
            None
        };
        Ok(Self { prefix, transfer })
    }
}
