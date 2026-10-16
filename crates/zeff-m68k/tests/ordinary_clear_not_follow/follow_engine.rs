use crate::device::{Datum, Device, Event, Script};
use crate::fixed::Fixed;
use crate::follow_axes::{FRAME, FollowAxis};
use crate::oracle::{self, Expected as Primary};
use crate::seed::{self, LIMIT};
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{TimedStep, TimingFault};

#[derive(Clone, Debug)]
pub struct Expected {
    pub axis: FollowAxis,
    pub origin: Primary,
    pub initial_device: Device,
    pub origin_device: Device,
    pub device: Device,
    pub r: Registers,
    pub queue: Option<[u16; 2]>,
    pub stopped: bool,
    pub error: Option<TimingFault<usize>>,
    pub step: Option<TimedStep>,
    pub attempts: Fixed<&'static str>,
    pub callbacks: Fixed<&'static str>,
    pub fault: Option<&'static str>,
    pub blocked: Option<&'static str>,
}

fn configure(device: &mut Device, word: u16, supervisor: bool) {
    assert_eq!(device.reply, crate::device::Reply::parse("Autovector"));
    let phases: &[&str] = if matches!(word, 0x4e71 | 0x4a45) {
        &["NX_F"]
    } else if word == 0x4e72 && supervisor {
        &[]
    } else {
        &FRAME
    };
    for phase in phases {
        device.phases[device.phase_count] = Some(phase);
        device.phase_count += 1;
    }
}

impl Expected {
    pub fn result(&self) -> Result<TimedStep, TimingFault<usize>> {
        self.error
            .clone()
            .map_or_else(|| Ok(self.step.unwrap()), Err)
    }

    pub fn cpu_image(&self) -> Primary {
        let mut image = self.origin.clone();
        image.r = self.r;
        image.queue = self.queue.unwrap_or([0, 0]);
        image.device.clock = self.device.clock;
        image.error = self.error.clone();
        image.trace = false;
        image
    }

    pub fn debug(&self) -> String {
        if !self.stopped {
            return self.cpu_image().debug(false);
        }
        format!(
            "PrefetchCpu {{ registers: {:?}, clock: CpuClocks({}), pipeline: None, frozen: false, stopped: true, interrupts: InterruptState {{ level: 0, level7_edge: false }}, pending_trace: false, physical_state: Idle, physical_inspection: DiagnosticCompleteValues, imported_boundary: None }}",
            self.r, self.device.clock
        )
    }

    fn fail(&mut self, phase: &'static str, error: TimingFault<usize>) {
        self.error = Some(error);
        self.fault = Some(phase);
    }

    fn idle(&mut self, phase: &'static str, clocks: u32) -> bool {
        let start = self.device.clock;
        let Some(end) = start.checked_add(u64::from(clocks)) else {
            self.blocked = Some(phase);
            self.fail(phase, TimingFault::ClockOverflow);
            return false;
        };
        self.device.events.push(Event::Idle {
            phase,
            start,
            clocks,
        });
        self.device.clock = end;
        self.device.entry_idle_count += 1;
        true
    }

    fn transfer(
        &mut self,
        phase: &'static str,
        address: u32,
        payload: Option<u16>,
        fc: FunctionCode,
        kind: AccessKind,
    ) -> Option<u16> {
        if address & 1 != 0 {
            self.fail(phase, TimingFault::AddressError { address });
            return None;
        }
        self.attempts.push(phase);
        if self.device.clock > LIMIT {
            self.blocked = Some(phase);
            self.fail(phase, TimingFault::ClockOverflow);
            return None;
        }
        let access = BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK,
            function_code: fc,
            kind,
            lanes: ByteLanes::Both,
        };
        let direction = if payload.is_some() {
            Direction::Write
        } else {
            Direction::Read
        };
        let sampled = self.device.words.sample(address);
        let index = self.device.transfers;
        if let Some(value) = payload {
            self.device.words.insert(address, value);
        }
        self.device.events.push(Event::Transfer {
            phase,
            access,
            direction,
            value: Datum::Word(payload.unwrap_or(sampled)),
            start: self.device.clock,
            wait: 0,
            elapsed: 4,
            failed: false,
            deposited: payload.is_some(),
            index,
        });
        self.device.scripts.push(Script {
            phase,
            access,
            direction,
            wait: 0,
            result: Ok(payload.is_none().then_some(Datum::Word(sampled))),
        });
        self.device.transfers += 1;
        self.device.clock += 4;
        self.callbacks.push(phase);
        Some(sampled)
    }

    fn exception(&mut self, vector: u8, pc: u32) {
        let (sp, sr) = (self.r.ssp, self.r.status_register());
        if !self.idle("NextExceptionI4", 4) {
            return;
        }
        for (phase, address, value) in [
            ("NX_PCL", sp.wrapping_sub(2), pc as u16),
            ("NX_SR", sp.wrapping_sub(6), sr),
            ("NX_PCH", sp.wrapping_sub(4), (pc >> 16) as u16),
        ] {
            if self
                .transfer(
                    phase,
                    address,
                    Some(value),
                    FunctionCode::SupervisorData,
                    AccessKind::Stack,
                )
                .is_none()
            {
                return;
            }
        }
        let address = u32::from(vector) * 4;
        let Some(high) = self.transfer(
            "NX_VH",
            address,
            None,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
        ) else {
            return;
        };
        let Some(low) = self.transfer(
            "NX_VL",
            address + 2,
            None,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
        ) else {
            return;
        };
        let target = u32::from(high) << 16 | u32::from(low);
        let Some(first) = self.transfer(
            "NX_H0",
            target,
            None,
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        ) else {
            return;
        };
        if !self.idle("NextExceptionI2", 2) {
            return;
        }
        let Some(second) = self.transfer(
            "NX_H1",
            target.wrapping_add(2),
            None,
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        ) else {
            return;
        };
        self.r.pc = target;
        self.r.ssp = sp.wrapping_sub(6);
        self.r.set_status_register((sr | 0x2000) & !0x8000);
        self.queue = Some([first, second]);
    }

    fn execute(&mut self) {
        let start = self.device.clock;
        let pc = self.r.pc;
        let queue = self.queue.unwrap();
        let word = queue[0];
        assert_eq!(word, self.axis.next);
        let mut exception = None;
        if matches!(word, 0x4e71 | 0x4a45) {
            if word == 0x4a45 {
                self.r.set_status_register(oracle::flags(
                    self.r.status_register(),
                    16,
                    self.r.d[5] & 65535,
                ));
            }
            let fc = if self.r.status_register() & 0x2000 != 0 {
                FunctionCode::SupervisorProgram
            } else {
                FunctionCode::UserProgram
            };
            let Some(fetched) =
                self.transfer("NX_F", pc.wrapping_add(4), None, fc, AccessKind::Prefetch)
            else {
                return;
            };
            self.r.pc = pc.wrapping_add(2);
            self.queue = Some([queue[1], fetched]);
        } else if word == 0x4e72 && self.r.status_register() & 0x2000 != 0 {
            if !self.idle("NextStopI4", 4) {
                return;
            }
            self.r.set_status_register(queue[1]);
            self.r.pc = pc.wrapping_add(4);
            self.queue = None;
            self.stopped = true;
        } else {
            let vector = match word {
                0xffff => 11,
                0x4afc => 4,
                0x4e72 => 8,
                _ => panic!("next opcode"),
            };
            exception = Some(vector);
            self.exception(vector, pc);
            if self.error.is_some() {
                return;
            }
        }
        self.step = Some(TimedStep {
            opcode: word,
            clocks: self.device.clock - start,
            exception,
        });
    }
}

pub fn predict(axis: FollowAxis) -> Expected {
    let origin = oracle::predict(seed::build(&axis.primary));
    assert!(origin.importable && origin.error.is_none() && !origin.trace);
    let mut initial_device = origin.initial.device.clone();
    let mut device = origin.device.clone();
    let supervisor = origin.r.status_register() & 0x2000 != 0;
    configure(&mut initial_device, axis.next, supervisor);
    configure(&mut device, axis.next, supervisor);
    let mut expected = Expected {
        r: origin.r,
        queue: Some(origin.queue),
        origin,
        axis,
        initial_device,
        origin_device: device.clone(),
        device,
        stopped: false,
        error: None,
        step: None,
        attempts: Fixed::default(),
        callbacks: Fixed::default(),
        fault: None,
        blocked: None,
    };
    expected.execute();
    expected
}
