//! Protocol scaling probes, separate from the frozen Lua parity workloads.
use crate::diagnostics;
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_source::source::SourceFile;

const SOURCE: &str = r#"
trait Step { fn step(self, value: i32) -> i32; }
impl Step for i32 { fn step(self, value: i32) -> i32 { value + 1 } }
impl Step for i64 { fn step(self, value: i32) -> i32 { value + 2 } }
trait Forward { fn forward<T>(self, value: T) -> T { value } }
impl Forward for i32 {}
impl Forward for i64 {}
fn fixed_interface(n: i32) -> i32 {
    val receiver: Step = 0; var sum = 0; var i = 0;
    while i < n { sum += receiver.step(i); i += 1; } sum
}
fn changing_interface(n: i32) -> i32 {
    val first: Step = 0; val second: Step = 0i64; var sum = 0; var i = 0;
    while i < n {
        val receiver = if i % 2 == 0 { first } else { second };
        sum += receiver.step(i); i += 1;
    } sum
}
fn fixed_application(n: i32) -> i32 {
    val receiver: Forward = 0; var sum = 0; var i = 0;
    while i < n { sum += receiver.forward(i); i += 1; } sum
}
fn changing_application(n: i32) -> i32 {
    val first: Forward = 0; val second: Forward = 0i64; var sum = 0; var i = 0;
    while i < n {
        val receiver = if i % 2 == 0 { first } else { second };
        sum += if i % 4 < 2 { receiver.forward(i) }
            else { receiver.forward(i as i64) as i32 };
        i += 1;
    } sum
}
"#;

pub(super) fn run() {
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    let entries = [
        "fixed_interface",
        "changing_interface",
        "fixed_application",
        "changing_application",
    ];
    let mut source = SOURCE.to_owned();
    for n in [2_500, 5_000] {
        for entry in entries {
            source.push_str(&format!("\nfn {entry}_{n}() -> i32 {{ {entry}({n}) }}"));
        }
    }
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("protocol_scaling.kgr", source),
            Default::default(),
        )
        .unwrap();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    for n in [2_500, 5_000] {
        for entry in entries {
            let expected = (0..n)
                .map(|i| match entry {
                    "fixed_interface" => i + 1,
                    "changing_interface" => i + 1 + i % 2,
                    _ => i,
                })
                .sum();
            diagnostics::measure(
                &runtime,
                &loaded,
                &context,
                &format!("{entry}_{n}"),
                &[],
                expected,
            );
        }
    }
}
