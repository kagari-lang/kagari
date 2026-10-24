//! Persistent state, fixed recipes and current helper calls through the source embedding API.
//! Run: cargo run -p kagari --example persistent_static --no-default-features --features source

use kagari::{engine::KagariEngine, runtime::Runtime};
use kagari_runtime::module::Program;
use kagari_source::source::SourceFile;

const V1: &str = r#"
struct Box { var value: i32 }
static var COUNT: i32 = 0;
static val BOX: Box = Box { value: 1 };
static val CALLBACK: fn() -> i32 = || version() + COUNT;
static val READY: i32 = version();
static val LATE: i32 = version();

fn version() -> i32 { 10 }
pub fn next() -> i32 { COUNT += 1; COUNT }
pub fn bump_object() -> i32 { BOX.value += 1; BOX.value }
pub fn object() -> i32 { BOX.value }
pub fn callback() -> i32 { CALLBACK() }
pub fn current_code() -> i32 { version() + COUNT }
pub fn ready() -> i32 { READY }
pub fn late() -> i32 { LATE }
"#;

fn call(runtime: &Runtime, module: &Program, entry: &str) -> i32 {
    runtime
        .execute_typed(module, entry, (), &Default::default())
        .unwrap()
}

fn main() {
    let engine = KagariEngine::default();
    let first = engine
        .compile_to_program(
            SourceFile::new("persistent-static.kgr", V1),
            Default::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(Default::default());
    let old = runtime.load_program(&first, Default::default()).unwrap();
    let retained_current_code = runtime
        .function::<(), i32>(&old.root(), "current_code")
        .unwrap();

    assert_eq!(call(&runtime, &old, "next"), 1);
    assert_eq!(call(&runtime, &old, "ready"), 10);
    let before = (
        call(&runtime, &old, "next"),
        call(&runtime, &old, "bump_object"),
        call(&runtime, &old, "callback"),
    );
    assert_eq!(before, (2, 2, 12));
    println!(
        "V1: counter={}, static val object's field={}, callback={}",
        before.0, before.1, before.2
    );

    // Keep every static declaration and direct initializer expression unchanged.
    // Ordinary helper bodies may change, including helpers called by cold recipes.
    let v2 = V1.replace("fn version() -> i32 { 10 }", "fn version() -> i32 { 20 }");
    let second = engine
        .compile_to_program(
            SourceFile::new("persistent-static.kgr", v2),
            Default::default(),
        )
        .unwrap();
    runtime
        .reload_program(&old, &second, Default::default())
        .unwrap();
    let current = old.clone();

    // Reload neither resets COUNT nor replaces BOX. A val binding still permits
    // mutation of the object's var field; old and new code share the same object.
    assert_eq!(call(&runtime, &current, "object"), 2);
    assert_eq!(call(&runtime, &current, "ready"), 10);
    let after = (
        call(&runtime, &current, "next"),
        call(&runtime, &current, "bump_object"),
        call(&runtime, &current, "callback"),
        call(&runtime, &current, "current_code"),
    );
    assert_eq!(after, (3, 3, 23, 23));
    assert_eq!(call(&runtime, &old, "object"), 3);
    assert_eq!(call(&runtime, &old, "current_code"), 23);
    // CALLBACK retains its closure body, but its call to version selects V2.
    // A named function binding likewise follows the current implementation.
    assert_eq!(runtime.call(&retained_current_code, ()).unwrap(), 23);
    println!(
        "After reload: counter={}, object={}, retained callback calling V2={}, V2 code={}",
        after.0, after.1, after.2, after.3
    );

    // LATE still enters V1's original recipe, whose ordinary call selects V2's helper.
    // In contrast, READY above keeps its already initialized value of 10.
    let late = call(&runtime, &current, "late");
    assert_eq!(late, 20);
    println!("First read of untouched LATE after reload: {late} (V1 recipe, V2 helper)");

    // A separate runtime installs new cells rather than reusing this installation.
    let mut fresh_runtime = engine.runtime(Default::default());
    let fresh = fresh_runtime
        .load_program(&second, Default::default())
        .unwrap();
    let installed = (
        call(&fresh_runtime, &fresh, "next"),
        call(&fresh_runtime, &fresh, "object"),
        call(&fresh_runtime, &fresh, "callback"),
        call(&fresh_runtime, &fresh, "late"),
    );
    assert_eq!(installed, (1, 1, 21, 20));
    assert_eq!(call(&fresh_runtime, &fresh, "ready"), 20);
    assert_eq!(call(&runtime, &current, "current_code"), 23);
    println!(
        "Fresh V2 installation: counter={}, object={}, callback={}, LATE={}",
        installed.0, installed.1, installed.2, installed.3
    );
}
