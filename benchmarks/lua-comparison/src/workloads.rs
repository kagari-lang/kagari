//! Matched workloads and independent Rust checksums; scripts do all measured work.

pub struct Workload {
    pub name: &'static str,
    pub size: i32,
    pub batch: usize,
    pub kagari: &'static str,
    pub lua: &'static str,
    pub reference: fn(i32) -> i32,
}

fn arithmetic(n: i32) -> i32 {
    (0..n).map(|i| i % 97).sum()
}

fn branches(n: i32) -> i32 {
    let mut state = 7;
    let mut sum = 0;
    for _ in 0..n {
        state = (state * 17 + 13) % 65_521;
        sum += if state % 3 == 0 {
            state % 97
        } else {
            state % 31
        };
    }
    sum
}

fn calls(n: i32) -> i32 {
    (0..n).map(|i| ((i % 97) * 17 + 13) % 65_521).sum()
}

fn fibonacci(n: i32) -> i32 {
    if n < 2 {
        n
    } else {
        fibonacci(n - 1) + fibonacci(n - 2)
    }
}

fn arrays(n: i32) -> i32 {
    (0..n).map(|i| (i % 97) * 3 + 1).sum()
}

fn maps(n: i32) -> i32 {
    (0..n).map(|i| i % 97 + 1).sum()
}

pub const WORKLOADS: &[Workload] = &[
    Workload {
        name: "entry",
        size: 0,
        batch: 1_000,
        kagari: include_str!("../workloads/entry.kgr"),
        lua: include_str!("../workloads/entry.lua"),
        reference: |_| 42,
    },
    Workload {
        name: "arithmetic",
        size: 50_000,
        batch: 1,
        kagari: include_str!("../workloads/arithmetic.kgr"),
        lua: include_str!("../workloads/arithmetic.lua"),
        reference: arithmetic,
    },
    Workload {
        name: "branches",
        size: 30_000,
        batch: 1,
        kagari: include_str!("../workloads/branches.kgr"),
        lua: include_str!("../workloads/branches.lua"),
        reference: branches,
    },
    Workload {
        name: "calls",
        size: 10_000,
        batch: 1,
        kagari: include_str!("../workloads/calls.kgr"),
        lua: include_str!("../workloads/calls.lua"),
        reference: calls,
    },
    Workload {
        name: "fibonacci",
        size: 20,
        batch: 1,
        kagari: include_str!("../workloads/fibonacci.kgr"),
        lua: include_str!("../workloads/fibonacci.lua"),
        reference: fibonacci,
    },
    Workload {
        name: "arrays",
        size: 2_000,
        batch: 1,
        kagari: include_str!("../workloads/arrays.kgr"),
        lua: include_str!("../workloads/arrays.lua"),
        reference: arrays,
    },
    Workload {
        name: "maps",
        size: 1_000,
        batch: 1,
        kagari: include_str!("../workloads/maps.kgr"),
        lua: include_str!("../workloads/maps.lua"),
        reference: maps,
    },
];
