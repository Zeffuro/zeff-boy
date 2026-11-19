use super::*;

fn operator(level: u16, state: Envelope, increment: u8) -> Operator {
    let mut operator = Operator {
        attenuation: level,
        envelope: state,
        keyed: true,
        ..Operator::default()
    };
    operator.pipeline.sampled = true;
    operator.pipeline.csm = true;
    operator.pipeline.total_level = 63;
    operator.pipeline.level_add = 504;
    operator.pipeline.prepared.increment = increment;
    operator.pipeline.prepared.sustain = 448;
    operator
}

#[test]
fn csm_transfer_precedes_the_signed_attack_increment() {
    let mut operator = operator(100, Envelope::Attack, 2);
    operator.clock_pipeline_envelope();
    assert_eq!(operator.attenuation, (100 | 504) - 13);
    assert_ne!(operator.attenuation, (100 - 13) | 504);
    assert_eq!(operator.envelope, Envelope::Attack);
    assert!(!operator.pipeline.reset);
}

#[test]
fn instant_csm_attack_uses_captured_tl_without_changing_manual_desire() {
    let mut operator = operator(1023, Envelope::Off, 8);
    operator.keyed = false;
    operator.pipeline.desired = false;
    operator.pipeline.prepared.maximum = true;
    operator.write(4, 1);
    operator.clock_pipeline_envelope();
    assert_eq!(operator.attenuation, 504);
    assert!(operator.keyed && operator.pipeline.reset);
    assert!(!operator.pipeline.desired);
    assert_eq!(operator.envelope, Envelope::Attack);
}

#[test]
fn csm_transfer_is_bitwise_and_repeats_without_a_key_edge() {
    let mut operator = operator(211, Envelope::Sustain, 2);
    operator.clock_pipeline_envelope();
    assert_eq!(operator.attenuation, (211 | 504) + 2);
    assert!(!operator.pipeline.reset);
    operator.clock_pipeline_envelope();
    assert_eq!(operator.attenuation, (((211 | 504) + 2) | 504) + 2);
    assert!(!operator.pipeline.reset);
}

#[test]
fn csm_off_decision_uses_the_pretransfer_level_before_increment_and_mask() {
    let mut below = operator(511, Envelope::Decay, 1);
    below.ssg.enabled = true;
    below.pipeline.total_level = 127;
    below.pipeline.level_add = 1016;
    below.clock_pipeline_envelope();
    assert_eq!(below.attenuation, ((511 | 1016) + 4) & ENVELOPE_MAX);
    assert_eq!(below.envelope, Envelope::Decay);
    let mut off = operator(512, Envelope::Decay, 1);
    off.ssg.enabled = true;
    off.clock_pipeline_envelope();
    assert_eq!(off.attenuation, ENVELOPE_MAX);
    assert_eq!(off.envelope, Envelope::Off);
}
