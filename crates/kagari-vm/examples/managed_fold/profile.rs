//! Diagnostics are separate from throughput samples and keep existing checks.

#[cfg(feature = "diagnostics")]
use kagari_runtime::{
    Runtime,
    diagnostics::{self, allocations},
    module::ExecutableModule,
    native::conversion::{arguments::IntoKagariArguments, context::ConversionContext},
};
use std::{
    hint::black_box,
    io::{self, Write},
    time::{Duration, Instant},
};

pub fn require_uninstrumented() {
    #[cfg(feature = "diagnostics")]
    panic!("do not time an instrumented binary");
}

pub fn sample(route: &str, expected: u64, execute: impl Fn() -> u64) {
    for _ in 0..3 {
        assert_eq!(black_box(execute()), expected);
    }
    println!("PROFILE_READY,route={route}");
    io::stdout().flush().unwrap();
    let start = Instant::now();
    let mut calls = 0;
    while start.elapsed() < Duration::from_secs(10) {
        assert_eq!(black_box(execute()), expected);
        calls += 1;
    }
    println!("PROFILE_DONE,route={route},calls={calls}");
}

#[cfg(feature = "diagnostics")]
pub fn counts(routes: &[&str], expected: u64, execute: impl Fn(&str) -> u64) {
    for route in routes {
        for _ in 0..3 {
            assert_eq!(black_box(execute(route)), expected);
        }
        let ((result, events), allocation) =
            allocations::measure(|| diagnostics::measure(|| execute(route)));
        assert_eq!(result, expected);
        println!("COUNTS,route={route},allocation={allocation:?},events={events:?}");
    }
}

#[cfg(feature = "diagnostics")]
pub fn mapping(runtime: &Runtime, owner: &ExecutableModule) {
    let mut conversion = ConversionContext::new(runtime, owner).unwrap();
    let ty = conversion.type_for::<u64>().unwrap();
    let (_, allocation) = allocations::measure(|| {
        for _ in 0..2048 {
            conversion.check_type::<u64>(black_box(&ty)).unwrap();
        }
    });
    println!("PROBE,name=scalar_mapping,operations=2048,allocation={allocation:?}");
    let expected = [ty.clone(), ty];
    let (_, allocation) = allocations::measure(|| {
        for _ in 0..2048 {
            drop(black_box(
                (1u64, 2u64)
                    .into_arguments(&mut conversion, &expected)
                    .unwrap(),
            ));
        }
    });
    println!("PROBE,name=scalar_argument_pack,operations=2048,allocation={allocation:?}");
}

#[cfg(not(feature = "diagnostics"))]
pub fn counts(_routes: &[&str], _expected: u64, _execute: impl Fn(&str) -> u64) {
    panic!("--counts requires --features diagnostics");
}
