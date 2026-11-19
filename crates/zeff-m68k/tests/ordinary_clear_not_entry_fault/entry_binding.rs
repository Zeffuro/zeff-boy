use crate::binding::{self, list, n, object, opt, s};
use crate::entry_engine::{Completion, Expected};
use crate::literal::Value;

pub fn wrapper(expected: &Expected) -> Value {
    let base = binding::wrapper(&expected.origin, expected.axis.value.clone());
    let result = match expected.result() {
        Err(error) => binding::outcome(Err(error)),
        Ok(completion) => {
            let body = match completion {
                Completion::Origin(_) => object(vec![(
                    "origin_instruction",
                    base.get("expected").get("typed_result").get("Ok").clone(),
                )]),
                Completion::Instruction(step) => {
                    let typed = binding::outcome(Ok(step));
                    object(vec![(
                        "instruction",
                        typed.get("Ok").get("TimedStep").clone(),
                    )])
                }
                Completion::Trace(clocks) => {
                    object(vec![("trace_clocks_u64", opt(clocks, Value::Decimal))])
                }
                Completion::Interrupt {
                    level,
                    vector,
                    clocks,
                } => object(vec![
                    ("interrupt_level", n(level)),
                    ("vector", n(vector)),
                    ("clocks_u64", Value::Decimal(clocks)),
                ]),
            };
            object(vec![("Ok", body)])
        }
    };
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
            "expected_entry",
            object(vec![
                ("typed_result", result),
                ("CPU", binding::cpu(&expected.cpu_image(), false)),
                (
                    "device",
                    binding::expanded(&expected.origin, &expected.device),
                ),
            ]),
        ),
        (
            "entry_reachability",
            object(vec![
                (
                    "reserve_attempts",
                    list(expected.attempts.iter().map(|phase| s(phase))),
                ),
                (
                    "consumed_callbacks",
                    list(expected.callbacks.iter().map(|phase| s(phase))),
                ),
                (
                    "requested_phase",
                    opt(expected.axis.value.find("entry_fail_phase"), Clone::clone),
                ),
                ("fault_phase", opt(expected.fault, s)),
                ("blocked_phase", opt(expected.blocked, s)),
            ]),
        ),
    ])
}
