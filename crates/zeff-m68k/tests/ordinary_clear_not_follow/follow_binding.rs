use crate::binding::{self, b, list, object, opt, s};
use crate::follow_axes::{self, Api};
use crate::follow_engine::Expected;
use crate::literal::Value;

pub fn cpu(expected: &Expected) -> Value {
    let mut value = binding::cpu(&expected.cpu_image(), false);
    if expected.stopped {
        follow_axes::set(&mut value, "queue", Value::Null);
        let Value::Object(fields) = &mut value else {
            panic!("CPU object");
        };
        let private = &mut fields
            .iter_mut()
            .find(|(name, _)| *name == "private_cpu")
            .unwrap()
            .1;
        follow_axes::set(private, "pipeline", Value::Null);
        follow_axes::set(private, "stopped", b(true));
    }
    value
}

pub fn wrapper(expected: &Expected) -> Value {
    let base = binding::wrapper(&expected.origin, expected.axis.value.clone());
    let mut typed = binding::outcome(expected.result());
    if expected.error.is_none() && expected.axis.api == Api::Boundary {
        let step = typed.get("Ok").get("TimedStep").clone();
        typed = object(vec![(
            "Ok",
            object(vec![("TimedOutcome::Instruction", step)]),
        )]);
    }
    object(vec![
        ("axis", expected.axis.value.clone()),
        (
            "initial",
            object(vec![
                (
                    "constructor_input",
                    base.get("initial").get("constructor_input").clone(),
                ),
                ("CPU", base.get("initial").get("CPU").clone()),
                (
                    "device",
                    binding::expanded(&expected.origin, &expected.initial_device),
                ),
            ]),
        ),
        ("origin", base.get("expected").clone()),
        (
            "expected_follow",
            object(vec![
                ("typed_result", typed),
                ("CPU", cpu(expected)),
                (
                    "device",
                    binding::expanded(&expected.origin, &expected.device),
                ),
            ]),
        ),
        (
            "follow_reachability",
            object(vec![
                (
                    "reserve_attempts",
                    list(expected.attempts.iter().map(|phase| s(phase))),
                ),
                (
                    "consumed_callbacks",
                    list(expected.callbacks.iter().map(|phase| s(phase))),
                ),
                ("requested_phase", Value::Null),
                ("fault_phase", opt(expected.fault, s)),
                ("blocked_phase", opt(expected.blocked, s)),
            ]),
        ),
    ])
}
