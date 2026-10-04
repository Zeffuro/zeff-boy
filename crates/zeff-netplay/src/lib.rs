#![forbid(unsafe_code)]

#[cfg(not(target_arch = "wasm32"))]
pub mod endpoint;

pub mod datagram;
#[cfg(any(feature = "native-proof", feature = "test-support"))]
#[path = "proof/fixture.rs"]
pub mod fixture;
pub mod lockstep;
#[cfg(all(not(target_arch = "wasm32"), feature = "native-proof"))]
pub mod proof;
pub mod rollback;
pub mod wire;
