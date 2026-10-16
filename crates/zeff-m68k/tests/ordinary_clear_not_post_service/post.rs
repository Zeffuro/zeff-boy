use crate::binding::{self, b, list, object, opt, s};
use crate::entry_axes::{self, Api, EntryAxis};
use crate::entry_binding;
use crate::entry_engine::{self, Completion, Expected as Entry};
use crate::literal::Value;
use zeff_m68k::timing::TimingFault;

#[derive(Clone, Debug)]
pub struct Expected {
    pub before: Entry,
    pub after: Entry,
    pub result: Result<Option<u64>, TimingFault<usize>>,
    pub unchanged: bool,
    pub outcome: &'static str,
}

fn put(value: &mut Value, key: &'static str, item: Value) {
    let Value::Object(fields) = value else {
        panic!("object");
    };
    let (_, old) = fields.iter_mut().find(|(name, _)| *name == key).unwrap();
    *old = item;
}

pub fn predict(value: &Value) -> Expected {
    let axis = EntryAxis::from_value(value);
    assert_eq!(axis.section, 28);
    assert!(axis.trace && matches!(axis.api, Api::Instruction | Api::Boundary | Api::Step));
    let before = entry_engine::predict(axis);
    let (after, result, outcome) = if before.error.is_some() {
        (before.clone(), Err(TimingFault::Frozen), "Frozen")
    } else if !before.trace {
        (before.clone(), Ok(None), "None")
    } else {
        assert_eq!(before.axis.api, Api::Instruction);
        let mut service = before.axis.value.clone();
        put(&mut service, "api", s("service_pending_trace"));
        let after = entry_engine::predict(EntryAxis::from_value(&service));
        assert_eq!(after.origin_device, before.device);
        let result = match after.result() {
            Ok(Completion::Trace(clocks)) => Ok(clocks),
            Err(error) => Err(error),
            _ => panic!("service result"),
        };
        let outcome = match result {
            Ok(Some(_)) => "Some",
            Err(TimingFault::AddressError { .. }) => "AddressError",
            _ => panic!("service route"),
        };
        (after, result, outcome)
    };
    let unchanged = before.cpu_image().debug(false) == after.cpu_image().debug(false)
        && before.device == after.device;
    assert_eq!(unchanged, matches!(outcome, "None" | "Frozen"));
    Expected {
        before,
        after,
        result,
        unchanged,
        outcome,
    }
}

pub fn wrapper(expected: &Expected) -> Value {
    let before = entry_binding::wrapper(&expected.before);
    let after = entry_binding::wrapper(&expected.after);
    let mut service = after.get("expected_entry").clone();
    let result = match &expected.result {
        Ok(clocks) => object(vec![(
            "Ok",
            object(vec![("trace_clocks_u64", opt(*clocks, Value::Decimal))]),
        )]),
        Err(TimingFault::Frozen) => object(vec![(
            "Err",
            object(vec![("TimingFault::Frozen", Value::Null)]),
        )]),
        Err(error) => binding::outcome(Err(error.clone())),
    };
    put(&mut service, "typed_result", result);
    let offset = expected.before.device.events.len;
    let events = service.get("device").get("events").list()[offset..].to_vec();
    let attempts = if expected.unchanged {
        list([])
    } else {
        list(expected.after.attempts.iter().map(|phase| s(phase)))
    };
    let callbacks = if expected.unchanged {
        list([])
    } else {
        list(expected.after.callbacks.iter().map(|phase| s(phase)))
    };
    object(vec![
        ("axis", expected.before.axis.value.clone()),
        ("initial", before.get("initial").clone()),
        ("expected_origin", before.get("expected_entry").clone()),
        ("expected_service", service),
        (
            "service_reachability",
            object(vec![
                ("outcome", s(expected.outcome)),
                ("unchanged", b(expected.unchanged)),
                ("reserve_attempts", attempts),
                ("consumed_callbacks", callbacks),
                ("events", Value::List(events)),
            ]),
        ),
    ])
}

pub fn source_rows(mut visit: impl FnMut(Value)) {
    entry_axes::each(|axis| visit(wrapper(&predict(&axis.value))));
}
pub fn input_rows(mut visit: impl FnMut(Value)) {
    entry_axes::each(|axis| visit(axis.value));
}
pub fn all() -> (usize, usize) {
    let mut count = 0;
    let mut counts = [0; 4];
    let mut maxima = [0; 3];
    entry_axes::each(|axis| {
        let expected = predict(&axis.value);
        counts[match expected.outcome {
            "Some" => 0,
            "None" => 1,
            "AddressError" => 2,
            "Frozen" => 3,
            _ => panic!("outcome"),
        }] += 1;
        for (slot, value) in maxima.iter_mut().zip([
            expected.after.device.words.len,
            expected.after.device.events.len,
            expected.after.device.scripts.len,
        ]) {
            *slot = (*slot).max(value);
        }
        count += 1;
    });
    assert_eq!(counts, [6752, 13504, 160, 320]);
    assert_eq!(maxima, [33, 14, 12]);
    assert_eq!(count, 20736);
    (count, 0)
}
pub fn literals(log: bool) -> (usize, usize) {
    let rows = crate::literal::anchors();
    let mut bad = 0;
    for row in &rows {
        if wrapper(&predict(row.get("axis"))) != *row {
            bad += 1;
            if log {
                eprintln!(
                    "post-service literal {} mismatch",
                    row.get("axis").get("id").text()
                );
            }
        }
    }
    (rows.len(), bad)
}
