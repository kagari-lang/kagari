//! HIR timing driver. Deliberately has no global allocator wrapper.

mod report;
mod scenarios;
mod timing;
mod workloads;

use crate::{
    report::{Options, measure, print},
    scenarios::Setup,
};

fn main() {
    let options = Options::parse();
    if options.inputs {
        print(&options.manifest(false));
        return;
    }
    let (setup, provider_setup_ns) = measure(Setup::initialize);
    if !options.check {
        // Warm the process once, never the scenario databases used for measurement.
        drop(timing::suite(&options.workload, &setup));
    }
    let scenarios = timing::suite(&options.workload, &setup);
    print(&options.result(false, provider_setup_ns, scenarios));
}
