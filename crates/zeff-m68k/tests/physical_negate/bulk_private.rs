use super::*;
use bulk_oracle::Expected;

pub(super) fn verify(cpu: &PrefetchCpu, expected: &Expected) -> bool {
    if expected.guard.is_none()
        && matches!(expected.result, Ok(PhysicalInstructionStep::Instruction(_)))
        && expected.state == PhysicalEntryState::Idle
    {
        let image = PrefetchCpu::from_prefetched(
            expected.r,
            expected.queue.unwrap(),
            CpuClocks(expected.clock),
        )
        .unwrap();
        return *cpu == image;
    }
    let text = format!("{cpu:?}");
    let pipeline = match expected.queue {
        Some(words) => format!(
            "pipeline: Some(Pipeline {{ address: {}, words: {words:?} }}),",
            expected.r.pc
        ),
        None => "pipeline: None,".to_owned(),
    };
    text.matches("pipeline:").count() == 1
        && text.contains(&pipeline)
        && text.matches("frozen:").count() == 1
        && text.contains(&format!(
            "frozen: {},",
            expected.guard == Some(TimingFault::Frozen)
        ))
}
