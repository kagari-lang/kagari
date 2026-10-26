//! Fixed-size records keep probe bookkeeping out of allocation snapshots.
use kagari::runtime::Runtime;
use std::time::Instant;

#[cfg(feature = "diagnostics")]
use kagari_runtime::diagnostics::allocations::{self, Counts};
#[cfg(not(feature = "diagnostics"))]
use std::array;

#[derive(Clone, Copy, Default)]
pub(super) struct Phase {
    label: &'static str,
    #[cfg(not(feature = "diagnostics"))]
    ns: u128,
    objects: usize,
    #[cfg(feature = "diagnostics")]
    counts: Counts,
}

#[derive(Default)]
pub(super) struct Record {
    phases: [Phase; 10],
    len: usize,
}

impl Record {
    pub(super) fn capture(&mut self, label: &'static str, runtime: &Runtime, _start: Instant) {
        self.phases[self.len] = Phase {
            label,
            #[cfg(not(feature = "diagnostics"))]
            ns: _start.elapsed().as_nanos(),
            objects: runtime.runtime().gc().stats().allocated_objects,
            #[cfg(feature = "diagnostics")]
            counts: allocations::checkpoint().unwrap(),
        };
        self.len += 1;
    }
}

#[cfg(not(feature = "diagnostics"))]
pub(super) fn run(name: &str, action: impl Fn() -> Record) {
    for _ in 0..3 {
        action();
    }
    let records: [_; 11] = array::from_fn(|_| action());
    for index in 0..records[0].len {
        let label = records[0].phases[index].label;
        let samples: [_; 11] = array::from_fn(|sample| {
            assert_eq!(records[sample].phases[index].label, label);
            records[sample].phases[index].ns
        });
        let mut ordered = samples;
        ordered.sort_unstable();
        println!(
            "family,{name},phase={label},median_ns={},min_ns={},max_ns={},objects={},samples=11,warmups=3,samples_ns={samples:?}",
            ordered[5], ordered[0], ordered[10], records[0].phases[index].objects
        );
    }
}

#[cfg(feature = "diagnostics")]
pub(super) fn run(name: &str, action: impl Fn() -> Record) {
    // Prepare shared owners once; the measured Runtime must then fully balance.
    let (_, first_use) = allocations::measure(&action);
    println!("family_first_use,{name},counts={first_use:?}");
    let mut previous = None;
    for repetition in 0..3 {
        let (record, total) = allocations::measure(&action);
        assert_eq!(total.net_bytes, 0, "{name} Runtime teardown");
        let baseline = record.phases[0].counts;
        let deltas = record
            .phases
            .map(|phase| phase.counts.net_bytes - baseline.net_bytes);
        if let Some(previous) = previous {
            assert_eq!(deltas, previous, "{name} repeated phase backing plateau");
        }
        previous = Some(deltas);
        for phase in &record.phases[..record.len] {
            println!(
                "family_memory,{name},repeat={repetition},phase={},objects={},net_bytes={},net_delta_bytes={},interval_peak_bytes={},requests_since_setup={},requested_bytes_since_setup={}",
                phase.label,
                phase.objects,
                phase.counts.net_bytes,
                phase.counts.net_bytes - baseline.net_bytes,
                phase.counts.interval_peak_net_bytes,
                phase.counts.requests - baseline.requests,
                phase.counts.requested_bytes - baseline.requested_bytes,
            );
        }
        let post_setup_peak = record.phases[1..record.len]
            .iter()
            .map(|phase| phase.counts.interval_peak_net_bytes)
            .chain([total.interval_peak_net_bytes])
            .max()
            .unwrap();
        println!(
            "family_peak,{name},repeat={repetition},setup_peak_bytes={},post_setup_peak_bytes={post_setup_peak},post_setup_peak_delta_bytes={},lifecycle_peak_bytes={},teardown_interval_peak_bytes={}",
            baseline.interval_peak_net_bytes,
            post_setup_peak - baseline.net_bytes,
            total.peak_net_bytes,
            total.interval_peak_net_bytes,
        );
        println!("family_teardown,{name},repeat={repetition},counts={total:?}");
    }
}
