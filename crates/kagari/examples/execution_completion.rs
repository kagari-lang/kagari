//! Manual resumed-completion attribution; frozen lifecycle controls own acceptance.
use kagari::{
    context::ExecutionContext,
    engine::KagariEngine,
    error::{EmbeddingError, RuntimeFailureKind},
    runtime::Runtime,
};
use kagari_runtime::{
    Runtime as CoreRuntime,
    error::RuntimeError,
    frame::ExecutionFrame,
    module::Program,
    session::{ExecutionEvent, ExecutionObserver, owned::OwnedExecution},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_vm::vm::owned::DriveResult;
use std::{
    env,
    hint::black_box,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

const SOURCE: &str = r#"
pub fn hold(n: i32) -> i32 {
    var sum = 0; var i = 0;
    while i < 8 { sum += n; i += 1; }
    sum
}
"#;
const POPULATION: usize = 32;

#[derive(Debug)]
struct InstructionCount(Arc<AtomicUsize>);

impl ExecutionObserver for InstructionCount {
    fn observe(
        &mut self,
        _runtime: &CoreRuntime,
        event: ExecutionEvent,
        _frames: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        if event == ExecutionEvent::BeforeInstruction {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Route {
    Complete,
    Body,
    Return,
    Cancel,
}

impl Route {
    fn name(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Body => "body",
            Self::Return => "return",
            Self::Cancel => "cancel",
        }
    }
}

struct Fixture {
    runtime: Runtime,
    module: Program,
    context: ExecutionContext,
    instructions: usize,
}

impl Fixture {
    fn new() -> Self {
        let engine = KagariEngine::default();
        let program = engine
            .compile_to_program(
                SourceFile::new("completion.kgr", SOURCE),
                Default::default(),
            )
            .unwrap();

        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let module = runtime.load_program(&program, Default::default()).unwrap();
        let _selected_module = runtime.runtime().select_program(&module).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        runtime
            .runtime()
            .set_execution_observer(InstructionCount(count.clone()))
            .unwrap();
        let owner = unsafe {
            runtime.backend().start_transport(
                &_selected_module,
                "hold",
                &[Value::I32(1)],
                context.interpreter_options().unwrap(),
            )
        }
        .unwrap();
        let DriveResult::Complete(result) = runtime.backend().drive(&owner, slice(10_000)).unwrap()
        else {
            panic!("bounded instruction count must complete");
        };
        assert_eq!(
            result
                .unwrap()
                .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
                .unwrap(),
            Value::I32(8)
        );
        drop(owner);
        let instructions = count.load(Ordering::Relaxed);
        assert!(instructions > 2);
        runtime.runtime().clear_execution_observer().unwrap();
        runtime.runtime().collect_garbage().unwrap();
        Self {
            runtime,
            module,
            context,
            instructions,
        }
    }

    fn park(&self, roots: &mut Vec<OwnedExecution>, route: Route) {
        assert!(roots.is_empty());
        let steps = if matches!(route, Route::Return) {
            self.instructions - 1
        } else {
            1
        };
        for index in 0..POPULATION {
            let owner = unsafe {
                self.runtime.backend().start_transport(
                    &self.runtime.runtime().select_program(&self.module).unwrap(),
                    "hold",
                    &[Value::I32(index as i32 + 1)],
                    self.context.interpreter_options().unwrap(),
                )
            }
            .unwrap();
            assert!(matches!(
                self.runtime.backend().drive(&owner, slice(steps)).unwrap(),
                DriveResult::Runnable
            ));
            roots.push(owner);
        }
    }

    fn finish(&self, owner: &OwnedExecution, index: usize, cancel: bool) -> i64 {
        if cancel {
            owner.cancel();
        }
        let DriveResult::Complete(result) =
            self.runtime.backend().drive(owner, slice(10_000)).unwrap()
        else {
            panic!("bounded diagnostic must complete");
        };
        if cancel {
            assert!(matches!(
                result.map_err(EmbeddingError::from),
                Err(EmbeddingError::Runtime {
                    kind: RuntimeFailureKind::Cancelled,
                    ..
                })
            ));
            1
        } else {
            let expected = (index as i32 + 1) * 8;
            assert_eq!(
                result
                    .unwrap()
                    .inspect(self.runtime.runtime(), |view| view.as_i32().map(Value::I32))
                    .unwrap(),
                Value::I32(expected)
            );
            i64::from(expected)
        }
    }

    fn cycle(&self, roots: &mut Vec<OwnedExecution>, route: Route) -> (u128, i64) {
        self.park(roots, route);
        let start = Instant::now();
        let mut checksum = 0;
        if matches!(route, Route::Body) {
            for owner in roots.iter().rev() {
                assert!(matches!(
                    self.runtime
                        .backend()
                        .drive(owner, slice(self.instructions - 2))
                        .unwrap(),
                    DriveResult::Runnable
                ));
            }
        } else {
            for index in (0..POPULATION).rev() {
                let owner = roots.pop().unwrap();
                checksum += self.finish(&owner, index, matches!(route, Route::Cancel));
            }
        }
        let elapsed = start.elapsed().as_nanos();
        if matches!(route, Route::Body) {
            for index in (0..POPULATION).rev() {
                let owner = roots.pop().unwrap();
                checksum += self.finish(&owner, index, false);
            }
        }
        assert_eq!(
            self.runtime.runtime().drain_retired_executions().unwrap(),
            0
        );
        assert_eq!(
            self.runtime
                .runtime()
                .resources()
                .counters()
                .current_call_depth,
            0
        );
        self.runtime.runtime().collect_garbage().unwrap();
        (elapsed, checksum)
    }
}

fn slice(instructions: usize) -> NonZeroUsize {
    NonZeroUsize::new(instructions).unwrap()
}

fn main() {
    let mut reverse = false;
    let mut batch = 16;
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--reverse" => reverse = true,
            "--batch" => {
                batch = match arguments.next().as_deref() {
                    Some("1") => 1,
                    Some("16") => 16,
                    _ => panic!("expected --batch 1 or --batch 16"),
                };
            }
            _ => panic!("expected optional --reverse and --batch 1|16"),
        }
    }
    let fixture = Fixture::new();
    println!("INSTRUCTIONS,{}", fixture.instructions);
    let mut routes = [Route::Complete, Route::Body, Route::Return, Route::Cancel];
    if reverse {
        routes.reverse();
    }
    let mut roots = Vec::with_capacity(POPULATION);
    for route in routes {
        for _ in 0..3 {
            black_box(fixture.cycle(&mut roots, route));
        }
        for sample in 0..11 {
            let mut elapsed = 0;
            let mut checksum = 0;
            for _ in 0..batch {
                let result = fixture.cycle(&mut roots, route);
                elapsed += result.0;
                checksum += result.1;
            }
            println!(
                "COMPLETION,{},{POPULATION},{batch},{sample},{elapsed},{checksum}",
                route.name()
            );
        }
    }
}
