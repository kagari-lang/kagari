//! EL00-only models. These do not select production layouts or establish VM safety.
//! Retire this module at EL01 after transferring independent assertions to owners.
mod layout;
mod retention;
mod spans;

use kagari_runtime::{diagnostics::allocations, value::carriers::StoredValue};
use std::{hint::black_box, mem::size_of, time::Instant};

#[derive(Clone, Copy)]
#[repr(C)]
struct Inline32 {
    descriptor: u64,
    storage: [u8; 24],
}

#[allow(dead_code)]
enum Widened {
    Existing(StoredValue),
    Inline(Inline32),
}

fn copy_cost<const N: usize>() {
    const COUNT: usize = 262_144;
    const COPIES: usize = 64;
    let ((source, mut target), allocations) =
        allocations::measure(|| (vec![[37u8; N]; COUNT], vec![[0u8; N]; COUNT]));
    let mut samples = [0; 5];
    target.copy_from_slice(&source);
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..COPIES {
            black_box(&mut target).copy_from_slice(black_box(&source));
        }
        *sample = start.elapsed().as_nanos();
    }
    black_box(&target);
    samples.sort_unstable();
    assert_eq!(target[COUNT - 1], [37; N]);
    println!(
        "copy_model stride={N} entries={COUNT} copies={COPIES} backing_bytes={} alloc_requests={} median_ns={}",
        allocations.net_bytes, allocations.requests, samples[2],
    );
}

pub(super) fn run() {
    println!(
        "transport_model current_StoredValue={} inline_carrier={} wrapped_StoredValue={} aligned_u64={}",
        size_of::<StoredValue>(),
        size_of::<Inline32>(),
        size_of::<Widened>(),
        size_of::<(u64, u8)>(),
    );
    layout::run();
    retention::run();
    spans::run();
    copy_cost::<1>();
    copy_cost::<8>();
    copy_cost::<16>();
    copy_cost::<17>();
    copy_cost::<24>();
    copy_cost::<32>();
    copy_cost::<40>();
}
