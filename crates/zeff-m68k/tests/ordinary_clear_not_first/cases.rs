use crate::device::{Device, Event, Fixed, Script};
use crate::literal::Value;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{TimedStep, TimingFault};

fn u16_value(value: &Value) -> u16 {
    value.number().try_into().unwrap()
}

fn u32_value(value: &Value) -> u32 {
    value.number().try_into().unwrap()
}

pub fn registers(value: &Value) -> Registers {
    let mut r = Registers::new();
    assert_eq!(value.get("d").list().len(), 8);
    assert_eq!(value.get("a").list().len(), 7);
    for (target, source) in r.d.iter_mut().zip(value.get("d").list()) {
        *target = u32_value(source);
    }
    for (target, source) in r.a.iter_mut().zip(value.get("a").list()) {
        *target = u32_value(source);
    }
    r.usp = u32_value(value.get("usp"));
    r.ssp = u32_value(value.get("ssp"));
    r.pc = u32_value(value.get("pc"));
    r.set_status_register(u16_value(value.get("sr")));
    assert_eq!(r.status_register(), u16_value(value.get("sr")));
    r
}

pub fn queue(value: &Value) -> [u16; 2] {
    let words = value.list();
    assert_eq!(words.len(), 2);
    [u16_value(&words[0]), u16_value(&words[1])]
}

pub fn private_debug(value: &Value) -> String {
    let r = registers(value.get("registers"));
    let private = value.get("private_cpu");
    let pipeline = private.get("pipeline");
    let words = queue(value.get("queue"));
    assert_eq!(words, queue(pipeline.get("words")));
    assert_eq!(r.pc, u32_value(pipeline.get("address")));
    assert!(private.get("physical_state").get("Idle").null());
    assert_eq!(
        private.get("physical_inspection").text(),
        "DiagnosticCompleteValues"
    );
    assert!(private.get("imported_boundary").null());
    let interrupts = private.get("interrupts");
    format!(
        "PrefetchCpu {{ registers: {:?}, clock: CpuClocks({}), pipeline: Some(Pipeline {{ address: {}, words: {:?} }}), frozen: {}, stopped: {}, interrupts: InterruptState {{ level: {}, level7_edge: {} }}, pending_trace: {}, physical_state: Idle, physical_inspection: DiagnosticCompleteValues, imported_boundary: None }}",
        r,
        value.get("clock_u64").number(),
        r.pc,
        words,
        private.get("frozen").boolean(),
        private.get("stopped").boolean(),
        interrupts.get("level").number(),
        interrupts.get("level7_edge").boolean(),
        private.get("pending_trace").boolean()
    )
}

fn access(value: &Value) -> BusAccess {
    BusAccess {
        logical_address: u32_value(value.get("logical_address")),
        bus_address: u32_value(value.get("bus_address")),
        function_code: match value.get("FC").number() {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            _ => panic!("function code"),
        },
        kind: match value.get("kind").text() {
            "Data" => AccessKind::Data,
            "Prefetch" => AccessKind::Prefetch,
            _ => panic!("access kind"),
        },
        lanes: match value.get("lanes").text() {
            "Upper" => ByteLanes::Upper,
            "Lower" => ByteLanes::Lower,
            "Both" => ByteLanes::Both,
            _ => panic!("lanes"),
        },
    }
}

fn direction(value: &Value) -> Direction {
    match value.text() {
        "Read" => Direction::Read,
        "Write" => Direction::Write,
        _ => panic!("direction"),
    }
}

pub fn outcome(value: &Value) -> Result<TimedStep, TimingFault<usize>> {
    if let Some(ok) = value.find("Ok") {
        let step = ok.get("TimedStep");
        assert!(step.get("exception").null());
        return Ok(TimedStep {
            opcode: u16_value(step.get("opcode")),
            clocks: step.get("clocks_u64").number(),
            exception: None,
        });
    }
    let err = value.get("Err");
    if let Some(bus) = err.find("TimingFault::Bus") {
        Err(TimingFault::Bus {
            access: access(bus.get("access")),
            direction: direction(bus.get("direction")),
            source: bus.get("source").number().try_into().unwrap(),
        })
    } else {
        assert!(err.get("TimingFault::ClockOverflow").null());
        Err(TimingFault::ClockOverflow)
    }
}

fn words(value: &Value) -> Fixed<(u32, u16)> {
    let mut words = Fixed::default();
    for pair in value.list() {
        let pair = pair.list();
        assert_eq!(pair.len(), 2);
        words.insert(u32_value(&pair[0]), u16_value(&pair[1]));
    }
    assert_eq!(words.len, value.list().len());
    words
}

pub fn initial_device(value: &Value) -> Device {
    let names = value.get("phases").list();
    assert!(names.len() <= 6);
    let mut phases = [None; 6];
    let mut waits = [0; 6];
    let mut fail = None;
    for (index, name) in names.iter().enumerate() {
        let name = name.text();
        assert!(!phases[..index].contains(&Some(name)));
        phases[index] = Some(name);
        waits[index] = value.get("waits").find(name).map_or(0, u32_value);
        if !value.get("fail_phase").null() && value.get("fail_phase").text() == name {
            fail = Some(index);
        }
    }
    assert!(value.get("fail_phase").null() || fail.is_some());
    Device {
        words: words(value.get("word_RAM")),
        events: Fixed::default(),
        scripts: Fixed::default(),
        clock: value.get("CPU").get("clock_u64").number(),
        transfers: 0,
        valid: true,
        phases,
        waits,
        phase_count: names.len(),
        fail,
        failed_write_deposit: value.get("failed_write_deposit").boolean(),
        idle_phase: value.get("idle_phase").text(),
    }
}

pub fn expected_device(initial: &Device, value: &Value) -> Device {
    let mut device = initial.clone();
    device.words = words(value.get("word_RAM"));
    device.clock = value.get("clock_u64").number();
    device.transfers = value.get("transfers").number().try_into().unwrap();
    for value in value.get("events").list() {
        let phase = value.get("phase").text();
        let start = value.get("start_clock_u64").number();
        if let Some(bus) = value.find("access") {
            let direction = direction(value.get("direction"));
            let wait = u32_value(value.get("wait_clocks"));
            let failed = match value.get("outcome").text() {
                "Complete" => false,
                "HostFailure" => true,
                _ => panic!("callback outcome"),
            };
            let index = value.get("index").number().try_into().unwrap();
            device.events.push(Event::Transfer {
                phase,
                access: access(bus),
                direction,
                value: u16_value(value.get("value")),
                start,
                wait,
                elapsed: value.get("elapsed").number(),
                failed,
                deposited: value.get("deposited").boolean(),
                index,
            });
            device.scripts.push(Script {
                phase,
                direction,
                wait,
                failure: failed.then_some(index),
            });
        } else {
            device.events.push(Event::Idle {
                phase,
                start,
                clocks: u32_value(value.get("elapsed")),
            });
        }
    }
    assert_eq!(device.transfers, device.scripts.len);
    assert_eq!(
        device.failed_write_deposit,
        value.get("failed_write_deposit").boolean()
    );
    for (index, phase) in device.phases[..device.phase_count].iter().enumerate() {
        let phase = phase.unwrap();
        assert_eq!(
            device.waits[index],
            value.get("waits").find(phase).map_or(0, u32_value)
        );
    }
    assert_eq!(
        device.fail.map(|index| device.phases[index].unwrap()),
        if value.get("fail_phase").null() {
            None
        } else {
            Some(value.get("fail_phase").text())
        }
    );
    device
}
