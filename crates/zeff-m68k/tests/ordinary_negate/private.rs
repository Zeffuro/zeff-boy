use super::*;

pub(super) fn matches(cpu: &PrefetchCpu, expected: &fixture::Expected) -> bool {
    if !expected.frozen {
        let image =
            PrefetchCpu::from_prefetched(expected.r, expected.queue, CpuClocks(expected.clock))
                .unwrap();
        return *cpu == image;
    }
    let text = format!("{cpu:?}");
    let pipeline = format!(
        "pipeline: Some(Pipeline {{ address: {}, words: {:?} }}),",
        expected.queue_address, expected.queue
    );
    text.matches("pipeline:").count() == 1
        && text.contains(&pipeline)
        && text.matches("frozen:").count() == 1
        && text.contains(&format!("frozen: {},", expected.frozen))
}
