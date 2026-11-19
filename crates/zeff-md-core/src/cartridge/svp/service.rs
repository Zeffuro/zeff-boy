use super::controller::Controller;
use super::model::{Error, InstructionModel, Phase, Progress};
use super::ssp1601::{Core, Fault, Ports, ProgramRole, Step};
use std::{cell::RefCell, rc::Rc};

mod state;

pub(super) const MAX_CAPTURES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceRole {
    ResetVector,
    Opcode,
    InstructionWord,
    ProgramOperand,
    PortRead(u8),
    PortWrite(u8),
    StatusInputs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceResource {
    ProgramIram,
    ProgramIrom,
    SharedRom,
    SharedDram,
    ExternalIram,
    Controller,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceRequest {
    pub instruction: u64,
    pub pc: u16,
    pub ordinal: u8,
    pub role: ServiceRole,
    pub resource: ServiceResource,
    pub address: u32,
    pub mode: u16,
    pub status: u16,
    pub blind: bool,
    pub value: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceProgress {
    Waiting,
    Granted {
        request: ServiceRequest,
        response: u16,
    },
    Boundary(Progress),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Capture {
    pub request: ServiceRequest,
    pub response: u16,
}

struct Replay<'a> {
    captures: &'a [Capture],
    instruction: u64,
    pc: u16,
    validate: bool,
    model: &'a InstructionModel,
    cursor: usize,
    pending: Option<(ServiceRequest, Controller)>,
}

impl Replay<'_> {
    fn visit(&mut self, mut request: ServiceRequest, ports: &Controller) -> Result<u16, Fault> {
        request.instruction = self.instruction;
        request.pc = self.pc;
        request.ordinal = self.cursor as u8;
        if let Some(capture) = self.captures.get(self.cursor) {
            if capture.request != request {
                return Err(Fault::InvalidState("service request identity"));
            }
            self.cursor += 1;
            return Ok(capture.response);
        }
        self.pending = Some((request, ports.clone()));
        Err(Fault::ServicePending)
    }
}

#[derive(Clone)]
struct ReplayPorts<'a> {
    ports: Controller,
    replay: Rc<RefCell<Replay<'a>>>,
}

impl ReplayPorts<'_> {
    fn request(
        &self,
        role: ServiceRole,
        index: u8,
        status: u16,
        blind: bool,
        value: u16,
    ) -> Result<ServiceRequest, Fault> {
        let (resource, address, mode) = self.ports.service_target(
            index,
            status,
            blind,
            matches!(role, ServiceRole::PortWrite(_)),
        )?;
        Ok(ServiceRequest {
            instruction: 0,
            pc: 0,
            ordinal: 0,
            role,
            resource,
            address,
            mode,
            status,
            blind,
            value,
        })
    }
}

impl Ports for ReplayPorts<'_> {
    fn read(&mut self, index: u8, status: u16, blind: bool) -> Result<u16, Fault> {
        let request = self.request(ServiceRole::PortRead(index), index, status, blind, 0)?;
        let response = self.replay.borrow_mut().visit(request, &self.ports)?;
        let captured = if !self.replay.borrow().validate {
            Some(response)
        } else if request.resource == ServiceResource::SharedRom {
            Some(self.replay.borrow().model.historical_word(
                self.replay.borrow().captures[self.replay.borrow().cursor - 1].request,
            )?)
        } else {
            None
        };
        let actual = self
            .ports
            .read_captured(index, status, blind, captured)
            .map_err(|_| Fault::InvalidState("captured port read fault"))?;
        if self.replay.borrow().validate && actual != response {
            return Err(Fault::InvalidState("service read response"));
        }
        Ok(response)
    }

    fn write(&mut self, index: u8, status: u16, blind: bool, value: u16) -> Result<(), Fault> {
        let request = self.request(ServiceRole::PortWrite(index), index, status, blind, value)?;
        let response = self.replay.borrow_mut().visit(request, &self.ports)?;
        let captured = (!self.replay.borrow().validate).then_some(response);
        let actual = self
            .ports
            .write_captured(index, status, blind, value, captured)
            .map_err(|_| Fault::InvalidState("captured port write fault"))?;
        if actual != response {
            return Err(Fault::InvalidState("service write effect"));
        }
        Ok(())
    }

    fn status_inputs(&mut self) -> Result<u16, Fault> {
        let request = ServiceRequest {
            instruction: 0,
            pc: 0,
            ordinal: 0,
            role: ServiceRole::StatusInputs,
            resource: ServiceResource::Controller,
            address: 0,
            mode: 0,
            status: 0,
            blind: false,
            value: 0,
        };
        let response = self.replay.borrow_mut().visit(request, &self.ports)?;
        if self.replay.borrow().validate && response != self.ports.inputs {
            return Err(Fault::InvalidState("service status response"));
        }
        Ok(response)
    }
}

pub(super) enum Replayed {
    Pending(ServiceRequest, Controller),
    Ready(Box<Core>, Controller, Step),
}

impl InstructionModel {
    // This opt-in interface grants semantic accesses; it does not select a clock.
    pub fn new_synthetic_serviced(rom: Vec<u16>, irom: Vec<u16>) -> Result<Self, Error> {
        let mut model = Self::new_synthetic(rom, irom)?;
        model.enable_synthetic_services()?;
        Ok(model)
    }

    pub(crate) fn enable_synthetic_services(&mut self) -> Result<(), Error> {
        if !matches!(self.phase, Phase::Reset) || self.services.is_some() {
            return Err(Error::Busy);
        }
        self.services = Some(Vec::new());
        Ok(())
    }

    pub(super) fn program_request(&self, address: u16, role: ServiceRole) -> ServiceRequest {
        let resource = match address {
            0..=0x03ff => ServiceResource::ProgramIram,
            0xfc00..=0xffff => ServiceResource::ProgramIrom,
            _ => ServiceResource::SharedRom,
        };
        ServiceRequest {
            instruction: self.instructions,
            pc: self.core.pc,
            ordinal: 0,
            role,
            resource,
            address: u32::from(address),
            mode: 0,
            status: 0,
            blind: false,
            value: 0,
        }
    }

    pub(super) fn replay_services(&self, validate: bool) -> Result<Replayed, Fault> {
        let captures = self
            .services
            .as_deref()
            .ok_or(Fault::InvalidState("missing service owner"))?;
        let replay = Rc::new(RefCell::new(Replay {
            captures,
            instruction: self.instructions,
            pc: self.core.pc,
            validate,
            model: self,
            cursor: 0,
            pending: None,
        }));
        let ports = ReplayPorts {
            ports: self.ports.clone(),
            replay: replay.clone(),
        };
        let result = self.core.prepare_serviced(
            |address, role| {
                let role = match role {
                    ProgramRole::Opcode => ServiceRole::Opcode,
                    ProgramRole::InstructionWord => ServiceRole::InstructionWord,
                    ProgramRole::ProgramOperand => ServiceRole::ProgramOperand,
                };
                let response = replay
                    .borrow_mut()
                    .visit(self.program_request(address, role), &self.ports)?;
                let request = replay.borrow().captures[replay.borrow().cursor - 1].request;
                let expected = if request.resource == ServiceResource::SharedRom {
                    self.historical_word(request)?
                } else {
                    self.program(address)?
                };
                if validate && response != expected {
                    return Err(Fault::InvalidState("service program response"));
                }
                Ok(response)
            },
            &ports,
        );
        let mut replay = replay.borrow_mut();
        if replay.cursor != captures.len() {
            return Err(Fault::InvalidState("unused service captures"));
        }
        match result {
            Err(Fault::ServicePending) => {
                if captures.len() >= MAX_CAPTURES {
                    return Err(Fault::InvalidState("service capture bound"));
                }
                let (request, ports) = replay
                    .pending
                    .take()
                    .ok_or(Fault::InvalidState("missing service frontier"))?;
                Ok(Replayed::Pending(request, ports))
            }
            Err(error) => Err(error),
            Ok((core, ports, step)) => Ok(Replayed::Ready(Box::new(core), ports.ports, step)),
        }
    }

    pub fn pending_service(&self) -> Result<Option<ServiceRequest>, Error> {
        if self.services.is_none() {
            return Err(Error::Image("missing service owner"));
        }
        if self.boundary_control == Some(true) {
            return Ok(None);
        }
        Ok(match self.phase {
            Phase::Reset => Some(self.program_request(0xfffc, ServiceRole::ResetVector)),
            Phase::Fetch => Some(self.program_request(self.core.pc, ServiceRole::Opcode)),
            Phase::Execute { .. } => match self.replay_services(false)? {
                Replayed::Pending(request, _) => Some(request),
                Replayed::Ready(..) => None,
            },
            Phase::Commit { .. } => None,
        })
    }

    pub fn advance_service(&mut self, granted: bool) -> Result<ServiceProgress, Error> {
        if self.clock.is_some() {
            return Err(Error::Image("functional clock owns service grants"));
        }
        self.advance_service_phase(granted)
    }

    pub(super) fn advance_service_phase(
        &mut self,
        granted: bool,
    ) -> Result<ServiceProgress, Error> {
        self.advance_service_current(granted, None)
    }

    pub(crate) fn advance_service_current(
        &mut self,
        granted: bool,
        rom: Option<super::history::CurrentRom<'_>>,
    ) -> Result<ServiceProgress, Error> {
        if self.services.is_none() {
            return Err(Error::Image("missing service owner"));
        }
        if !granted || self.boundary_control == Some(true) {
            return Ok(ServiceProgress::Waiting);
        }
        match self.phase {
            Phase::Reset | Phase::Fetch => {
                let request = self.pending_service()?.unwrap();
                let response = if request.resource == ServiceResource::SharedRom {
                    self.current_word(request, rom)?
                } else {
                    self.program(request.address as u16)?
                };
                match self.phase {
                    Phase::Reset => {
                        self.core.pc = response;
                        self.phase = Phase::Fetch;
                    }
                    Phase::Fetch => {
                        self.phase = Phase::Execute {
                            pc: self.core.pc,
                            opcode: response,
                        };
                        self.services
                            .as_mut()
                            .unwrap()
                            .push(Capture { request, response });
                    }
                    _ => unreachable!(),
                }
                Ok(ServiceProgress::Granted { request, response })
            }
            Phase::Execute { pc, opcode } => match self.replay_services(false)? {
                Replayed::Pending(request, mut ports) => {
                    let response = match request.role {
                        ServiceRole::PortRead(index) => {
                            let history_len = self.rom_history.len();
                            let captured = if request.resource == ServiceResource::SharedRom {
                                Some(self.current_word(request, rom)?)
                            } else {
                                None
                            };
                            match ports.read_captured(
                                index,
                                request.status,
                                request.blind,
                                captured,
                            ) {
                                Ok(response) => response,
                                Err(error) => {
                                    self.rom_history.truncate(history_len);
                                    return Err(error.into());
                                }
                            }
                        }
                        ServiceRole::PortWrite(index) => ports.write_captured(
                            index,
                            request.status,
                            request.blind,
                            request.value,
                            None,
                        )?,
                        ServiceRole::StatusInputs => ports.status_inputs()?,
                        _ => {
                            if request.resource == ServiceResource::SharedRom {
                                self.current_word(request, rom)?
                            } else {
                                self.program(request.address as u16)?
                            }
                        }
                    };
                    self.services
                        .as_mut()
                        .unwrap()
                        .push(Capture { request, response });
                    Ok(ServiceProgress::Granted { request, response })
                }
                Replayed::Ready(core, ports, step) => {
                    self.phase = Phase::Commit { core, ports, step };
                    if !self.has_rom_history() {
                        self.services.as_mut().unwrap().clear();
                    }
                    Ok(ServiceProgress::Boundary(Progress::Prepared { pc, opcode }))
                }
            },
            Phase::Commit { .. } => self.advance_phase(true).map(ServiceProgress::Boundary),
        }
    }
}
