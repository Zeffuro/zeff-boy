use super::*;
use bulk_check::Count;

pub(super) fn main_paths(path: u8) -> (u32, u32) {
    let mut count = Count::default();
    for base in bulk_model::encoded() {
        for bank in [false, true] {
            for ccr in 0..32 {
                for value in bulk_model::values(base.size) {
                    let mut spec = base.bank(bank, ccr);
                    spec.operand = value;
                    for wait in if path == 0 { &[0, 3][..] } else { &[3][..] } {
                        spec.wait = *wait;
                        let (spec, device) = spec.seed();
                        let plan = bulk_plan::build(spec, &device);
                        assert_eq!(plan.address, bulk_model::address(spec, spec.extension));
                        assert_eq!(plan.operand, value);
                        if path == 0 {
                            count.planned(spec, device, plan);
                        } else {
                            for target in 0..spec.callbacks() {
                                if path == 2 && target <= plan.f_index {
                                    continue;
                                }
                                let mut device = device.clone();
                                if path == 3 {
                                    bulk_capacity::blocked(&plan, &mut device, target);
                                } else {
                                    device.fail = Some(target);
                                    device.deposit_failed = path != 2;
                                }
                                count.planned(spec, device, plan.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    count.result()
}
