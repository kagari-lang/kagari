//! Replayable operation traces for graph-model failures.

use crate::graph::{Runner, model::Operation};
use kagari_gc::error::GcError;
use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::Path,
};

pub(super) struct Trace {
    operations: Vec<Operation>,
}

impl Trace {
    pub fn execute(&mut self, runner: &mut Runner, op: Operation) -> Result<(), GcError> {
        self.operations.push(op);
        runner.execute(op)
    }
}

fn target(id: Option<usize>) -> String {
    id.map_or_else(|| "-".into(), |id| id.to_string())
}

fn encode(op: Operation) -> String {
    match op {
        Operation::Alloc(slot, id) => format!("alloc {slot} {}", target(id)),
        Operation::Edge(owner, id) => format!("edge {owner} {}", target(id)),
        Operation::Root(slot, id) => format!("root {slot} {}", target(id)),
        Operation::Dynamic(slot, id) => format!("dynamic {slot} {}", target(id)),
        Operation::Write(id, tag) => format!("write {id} {tag}"),
        Operation::Label(value) => format!("label {value}"),
        Operation::Push(id) => format!("push {id}"),
        Operation::Remove(index) => format!("remove {index}"),
        Operation::Step(budget) => format!("step {budget}"),
        Operation::Reset => "reset".into(),
        Operation::Collect => "collect".into(),
        Operation::Safepoint => "safepoint".into(),
    }
}

fn decode(line: &str) -> Operation {
    let fields: Vec<_> = line.split_whitespace().collect();
    let id = |text: &str| text.parse::<usize>().expect("numeric replay ID/index");
    let target = |text: &str| (text != "-").then(|| id(text));
    match fields.as_slice() {
        ["alloc", slot, next] => Operation::Alloc(id(slot), target(next)),
        ["edge", owner, next] => Operation::Edge(id(owner), target(next)),
        ["root", slot, next] => Operation::Root(id(slot), target(next)),
        ["dynamic", slot, next] => Operation::Dynamic(id(slot), target(next)),
        ["write", owner, tag] => Operation::Write(id(owner), tag.parse().expect("u32 tag")),
        ["label", value] => Operation::Label(value.parse().expect("u64 label")),
        ["push", owner] => Operation::Push(id(owner)),
        ["remove", index] => Operation::Remove(id(index)),
        ["step", budget] => Operation::Step(id(budget)),
        ["reset"] => Operation::Reset,
        ["collect"] => Operation::Collect,
        ["safepoint"] => Operation::Safepoint,
        _ => panic!("invalid replay syntax: {line}"),
    }
}

pub(super) fn run(seed: u64, action: impl FnOnce(&mut Trace) -> Result<(), GcError>) {
    let mut trace = Trace {
        operations: Vec::new(),
    };
    eprintln!("graph model seed={seed}");
    let result = catch_unwind(AssertUnwindSafe(|| {
        action(&mut trace).expect("valid graph operation")
    }));
    if let Err(panic) = result {
        let text = format!(
            "# graph trace v1 seed={seed}\n{}\n",
            trace
                .operations
                .iter()
                .copied()
                .map(encode)
                .collect::<Vec<_>>()
                .join("\n")
        );
        // Miri isolates filesystem access. Print before best-effort persistence
        // so diagnostics survive both isolation and an unwritable artifact path.
        eprintln!("{text}");
        if !cfg!(miri) {
            let directory =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gc-graph-failures");
            let path = directory.join(format!("seed-{seed}.trace"));
            if fs::create_dir_all(&directory)
                .and_then(|_| fs::write(&path, text))
                .is_ok()
            {
                eprintln!("Replay with KAGARI_GC_REPLAY={}", path.display());
            }
        }
        resume_unwind(panic);
    }
}

pub(super) fn replay(path: &str) {
    let text = fs::read_to_string(path).expect("read replay trace");
    let operations: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(decode)
        .collect();
    run(0, |trace| {
        let mut runner = Runner::new()?;
        for op in operations {
            trace.execute(&mut runner, op)?;
        }
        runner.finish()?;
        Ok(())
    });
}
