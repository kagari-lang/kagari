//! Focused characterization of the current candidate fold, without GC changes.

mod data;
mod profile;
mod script;

use crate::data::{ActiveData, Data};
#[cfg(feature = "diagnostics")]
use kagari_runtime::diagnostics::allocations::Allocator;
use kagari_runtime::native::value_handle::ScriptValue;
use std::{env, hint::black_box, rc::Rc, time::Instant};

const ITEMS: usize = 2048;
const WARMUPS: usize = 3;
const SAMPLES: usize = 11;
const ROUTES: [&str; 10] = [
    "rust",
    "managed_rust_scalar",
    "managed_rust_references",
    "bridge",
    "scalar",
    "references",
    "production",
    "bridge_gc",
    "references_gc",
    "production_gc",
];

const DIAGNOSE_ROUTES: [&str; 10] = [
    "managed_rust_scalar",
    "managed_rust_references",
    "bridge",
    "bridge_prepared",
    "bridge_raw",
    "bridge_raw_values",
    "script_loop",
    "script_sum",
    "scalar",
    "references",
];

#[cfg(feature = "diagnostics")]
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let process = args
        .first()
        .and_then(|arg| arg.parse::<usize>().ok())
        .unwrap_or(0);
    let expected = (ITEMS as u64 * (ITEMS as u64 + 1)) / 2;
    let start = Instant::now();
    let data = Rc::new(Data::new(ITEMS).unwrap());
    eprintln!("setup,candidate_data_ns={}", start.elapsed().as_nanos());
    let _active = ActiveData::enter(data.clone());
    let (vm, program) = script::prepare();
    let owner = vm.runtime().select_program(&program).unwrap();
    let start = Instant::now();
    let values: ScriptValue = vm.execute_typed(&owner, "values", (ITEMS as u64,)).unwrap();
    vm.runtime().collect_garbage().unwrap();
    eprintln!("setup,production_data_ns={}", start.elapsed().as_nanos());

    let execute = |route: &str| match route {
        "rust" => black_box(&data.plain)
            .iter()
            .fold(0, |sum, item| sum + black_box(*item)),
        "managed_rust_scalar" => data
            .scalar(&mut |sum, item| Ok(sum + black_box(item)))
            .unwrap(),
        "managed_rust_references" => data
            .references(&mut |sum, item| Ok(sum + black_box(item)))
            .unwrap(),
        "production" | "production_gc" => vm
            .execute_typed::<_, u64>(&owner, route, (values.clone(),))
            .unwrap(),
        _ => vm.execute_typed::<_, u64>(&owner, route, ()).unwrap(),
    };
    if args.iter().any(|arg| arg == "--counts") {
        profile::counts(&DIAGNOSE_ROUTES, expected, execute);
        #[cfg(feature = "diagnostics")]
        profile::mapping(vm.runtime(), &owner);
        return;
    }
    profile::require_uninstrumented();
    if let Some(route) = args.iter().find_map(|arg| arg.strip_prefix("--profile=")) {
        assert!(DIAGNOSE_ROUTES.contains(&route));
        profile::sample(route, expected, || execute(route));
        return;
    }
    let routes = if args.iter().any(|arg| arg == "--diagnose") {
        &DIAGNOSE_ROUTES
    } else {
        &ROUTES
    };
    let mut rows = Vec::new();
    // Rotate route order between samples and independent processes. Validation,
    // cleanup and output stay outside the measured batch; callback GC stays in it.
    for sample in 0..WARMUPS + SAMPLES {
        for offset in 0..routes.len() {
            let route = routes[(offset + sample + process) % routes.len()];
            let folds = match route {
                "rust" => 256,
                "managed_rust_scalar" | "managed_rust_references" => 16,
                _ => 1,
            };
            vm.runtime().collect_garbage().unwrap();
            data.memory.collect().unwrap();
            let cycles = data.memory.stats().unwrap().completed_cycles;
            let start = Instant::now();
            let mut checksum = 0;
            for _ in 0..folds {
                checksum += black_box(execute(black_box(route)));
            }
            let ns = start.elapsed().as_nanos();
            assert_eq!(checksum, expected * folds as u64);
            assert_eq!(data.memory.stats().unwrap().objects, ITEMS + 2);
            assert_eq!(data.memory.stats().unwrap().root_registrations, 2);
            let collections = if route.ends_with("_gc") {
                ITEMS / 256
            } else {
                0
            };
            assert_eq!(
                data.memory.stats().unwrap().completed_cycles - cycles,
                collections as u64
            );
            rows.push((route, sample, folds, ns, checksum));
        }
    }
    println!("route,sample,folds,items,ns,checksum");
    for (route, sample, folds, ns, checksum) in rows {
        println!("{route},{sample},{folds},{ITEMS},{ns},{checksum}");
    }
}
