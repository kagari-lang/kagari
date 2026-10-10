//! Protocol scaling probes, separate from the frozen Lua parity workloads.
use crate::diagnostics;
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_source::source::SourceFile;

const SOURCE: &str = r#"
use std::cmp::Ordering;
trait Step { fn step(self, value: i32) -> i32; }
impl Step for i32 { fn step(self, value: i32) -> i32 { value + 1 } }
impl Step for i64 { fn step(self, value: i32) -> i32 { value + 2 } }
trait Forward { fn forward<T>(self, value: T) -> T { value } }
impl Forward for i32 {}
impl Forward for i64 {}
trait Compare { fn less<T: Ord>(self, a: T, b: T) -> bool { a.cmp(b) == Ordering::Less } }
impl Compare for i32 {}
fn less<T: Ord>(a: T, b: T) -> bool { a.cmp(b) == Ordering::Less }
trait Relay { fn relay<T: Ord>(self, a: T, b: T) -> bool { less(a, b) } }
impl Relay for i32 {}
trait Append { fn append<T>(self, values: Vec<T>, value: T) { values.push(value); } }
impl Append for i32 {}
impl Append for i64 {}
struct Record { val value: i32 }
struct OtherRecord { val value: i32 }
struct Holder<T> { val value: T }
enum Wrapped<T> { Some(T) }
trait Package {
    fn pack<T>(self, value: T) -> Wrapped<Holder<T>> {
        Wrapped::Some(Holder { value: value })
    }
}
impl Package for i32 {}
fn scoped_layout(n: i32) -> i32 {
    val receiver: Package = 0; var sum = 0; var i = 0;
    while i < n {
        val item = Record { value: i }; val wrapped = receiver.pack(item);
        sum += match wrapped {
            Wrapped::Some(holder) => holder.value.value
        };
        i += 1;
    } sum
}
fn changing_scoped_layout(n: i32) -> i32 {
    val receiver: Package = 0; var sum = 0; var i = 0;
    while i < n {
        sum += if i % 2 == 0 {
            val item = Record { value: i }; val wrapped = receiver.pack(item);
            match wrapped {
                Wrapped::Some(holder) => holder.value.value
            }
        } else {
            val item = OtherRecord { value: i }; val wrapped = receiver.pack(item);
            match wrapped {
                Wrapped::Some(holder) => holder.value.value
            }
        };
        i += 1;
    } sum
}
fn native_application(n: i32) -> i32 {
    val receiver: Append = 0; val values: Vec<i32> = Vec::new(); var i = 0;
    while i < n { receiver.append(values, i); i += 1; }
    var sum = 0; for value in values { sum += value; } sum
}
fn changing_native_application(n: i32) -> i32 {
    val first: Append = 0; val second: Append = 0i64;
    val narrow: Vec<i32> = Vec::new(); val wide: Vec<i64> = Vec::new(); var i = 0;
    while i < n {
        val receiver = if i % 2 == 0 { first } else { second };
        if i % 4 < 2 { receiver.append(narrow, i); }
        else { receiver.append(wide, i as i64); }
        i += 1;
    }
    var sum = 0; for value in narrow { sum += value; }
    for value in wide { sum += value as i32; } sum
}
fn shared_application(n: i32) -> i32 {
    val receiver: Relay = 0; var sum = 0; var i = 0;
    while i < n { if receiver.relay(i, i + 1) { sum += i; } i += 1; } sum
}
fn witness_application(n: i32) -> i32 {
    val receiver: Compare = 0; var sum = 0; var i = 0;
    while i < n { if receiver.less(i, i + 1) { sum += i; } i += 1; } sum
}
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
        "witness_application",
        "shared_application",
        "native_application",
        "changing_native_application",
        "scoped_layout",
        "changing_scoped_layout",
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
    for n in [2_500, 5_000] {
        for entry in entries {
            let mut runtime = engine.runtime(context.clone());
            let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
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
