mod clock;
mod controller;
mod historical_state;
mod history;
mod history_service;
mod model;
mod service;
pub(crate) mod ssp1601;
mod state;
pub(crate) use history::CurrentRom;

#[cfg(test)]
mod tests;

pub use service::{ServiceProgress, ServiceRequest, ServiceResource, ServiceRole};

pub use clock::{FunctionalProfile, FunctionalTiming};
pub(crate) use controller::DramReadView;

pub use model::{Error, InstructionModel, Progress, Registers};
