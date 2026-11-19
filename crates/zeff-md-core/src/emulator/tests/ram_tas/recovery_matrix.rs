use super::*;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::sound::schedule::SoundInterval;
use crate::hardware::sound::{BankAccess, CandidateBus, SoundMemory};
use zeff_m68k::timing::TimingFault;

#[path = "recovery_matrix/common.rs"]
mod common;
#[path = "recovery_matrix/exclusion.rs"]
mod exclusion;
#[path = "recovery_matrix/failure.rs"]
mod failure;
#[cfg(feature = "sound-shadow-observer")]
#[path = "observer.rs"]
mod observer;
#[path = "recovery_matrix/oracle.rs"]
mod oracle;
#[path = "recovery_matrix/prior_dma.rs"]
mod prior_dma;
#[path = "recovery_matrix/replay_abort.rs"]
mod replay_abort;
#[path = "recovery_matrix/state.rs"]
mod state;
use common::Case;

macro_rules! matrix_test {
    ($name:ident, $body:block) => {
        #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
        #[cfg_attr(not(target_arch = "wasm32"), test)]
        fn $name() $body
    };
}

matrix_test!(actual_cpu_ram_tas_recovery_source_first, {
    failure::abort(true, failure::Failure::IdleAfter);
});
matrix_test!(actual_cpu_ram_tas_recovery_prior_180, {
    prior_dma::prior_rows();
});
matrix_test!(actual_cpu_ram_tas_recovery_dma_prior_28, {
    prior_dma::dma_prior_rows();
});
matrix_test!(actual_cpu_ram_tas_recovery_vdp_34, {
    prior_dma::vdp_rows();
});
matrix_test!(actual_cpu_ram_tas_recovery_failures_37_other, {
    failure::rows();
});
matrix_test!(actual_cpu_ram_tas_recovery_state_23, {
    state::rows();
});
matrix_test!(actual_cpu_ram_tas_recovery_exclusions_44, {
    exclusion::rows();
});

matrix_test!(
    actual_cpu_ram_tas_recovery_generic_idle_overflow_additional_2,
    {
        for concurrent in [false, true] {
            failure::abort(concurrent, failure::Failure::GenericIdleOverflow);
        }
    }
);
matrix_test!(actual_cpu_ram_tas_recovery_replay_abort_additional_4, {
    replay_abort::rows();
});
matrix_test!(
    actual_cpu_ram_tas_recovery_failed_second_prime_additional_2,
    {
        state::failed_second_prime();
    }
);
