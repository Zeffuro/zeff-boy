use crate::bus::{BusAccess, BusFault, BusOperation, FunctionCode, M68kBus};

struct PlainBus;

impl M68kBus for PlainBus {
    type Error = ();
    fn read(&mut self, _: BusAccess) -> Result<u16, ()> {
        panic!("unexpected ordinary read")
    }
    fn write(&mut self, _: BusAccess, _: u16) -> Result<(), ()> {
        panic!("unexpected ordinary write")
    }
}

#[test]
fn missing_bus_contracts_are_explicit_and_do_not_fall_back_to_ordinary_transfers() {
    let mut bus = PlainBus;
    assert_eq!(
        crate::bus::reset_output(&mut bus, 124),
        Err(BusFault::UnsupportedOperation {
            operation: BusOperation::ResetOutput
        })
    );
    for address in [0x2000, 0x2001] {
        assert_eq!(
            crate::bus::test_and_set_byte(&mut bus, address, FunctionCode::UserData),
            Err(BusFault::UnsupportedOperation {
                operation: BusOperation::TestAndSet
            })
        );
    }
}
