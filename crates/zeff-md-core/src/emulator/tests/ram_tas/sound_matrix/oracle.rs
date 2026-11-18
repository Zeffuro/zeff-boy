use super::super::sound_oracle::{CasePolicy, Oracle as SharedOracle};
use super::*;
use crate::hardware::bus::Bus;

pub(super) type Oracle = SharedOracle<Case>;

impl CasePolicy for Case {
    const GATE_RESET_REQUEST: bool = false;

    fn prefix_t(&self) -> u64 {
        Case::prefix_t(self)
    }

    fn expected_target(&self) -> (u32, bool, bool) {
        Case::expected_target(self)
    }

    fn after_publication(&self, bus: &mut Bus, any_bank_access: bool) {
        if self.kind == Kind::BankRegister && any_bank_access {
            bus.cartridge.registers.as_mut().unwrap().write(0, 0xa6);
        }
    }
}

impl SharedOracle<Case> {
    pub fn end(&mut self) {
        assert_eq!(self.bus.now.get(), self.lock.unwrap().1);
        self.lock = None;
        self.checkpoint = None;
    }
}
