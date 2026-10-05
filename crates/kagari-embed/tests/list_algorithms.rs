#![cfg(feature = "source")]
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;

fn execute(source: &str) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(SourceFile::new("lists.kgr", source), Default::default())
        .unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let program =
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::Bool(true)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn vec_algorithms_preserve_copies_aliases_and_order() {
    execute(
        r#"
fn main() -> bool {
    val a = [3, 1, 2, 1];
    val b = a.sorted();
    if a[0] != 3 || b[0] != 1 || b[3] != 3 { return false; }
    val alias = a;
    a.sort();
    if alias[0] != 1 || alias[3] != 3 { return false; }
    a.dedup();
    if a.len() != 3usize { return false; }
    a.reverse();
    a.retain(|x| x != 2);
    val c = [1, 2, 1, 3, 2].distinct();
    a.len() == 2usize && a[0] == 3 && a[1] == 1 && c.len() == 3usize && c[1] == 2
}
"#,
    );
}

#[test]
fn interface_methods_accept_comparators_and_keys() {
    execute(
        r#"use std::collections::{List, MutableList};

fn main() -> bool {
    val source = [3, 1, 2];
    val list: List<i32> = source;
    val ascending = list.sorted_by_key(|value| value);
    val descending = list.sorted_by(|a, b| b.cmp(a));
    val writable: MutableList<i32> = source;
    writable.sort_by_key(|value| -value);
    ascending[0] == 1 && descending[0] == 3 && source[0] == 3 && list.reversed()[0] == 1
}
"#,
    );
}

#[test]
fn unordered_elements_keep_unbounded_list_operations() {
    execute(
        r#"use std::collections::{List};

struct Item { val key: i32 }
fn main() -> bool {
    val values = [Item { key: 2 }, Item { key: 1 }];
    val list: List<Item> = values;
    val sorted = list.sorted_by_key(|item| item.key);
    sorted[0].key == 1 && list.reversed()[0].key == 1 && values[0].key == 2
}
"#,
    );
}

#[test]
fn custom_container_reuses_native_defaults_with_linear_read_traversal() {
    execute(
        r#"use std::collections::{List, MutableList};
use std::iter::{CollectionCursor};
use std::ops::{Index};

struct Sequence { val items: Vec<i32> }
impl Index<usize> for Sequence {
    type Output = i32;
    fn index(self, index: usize) -> i32 { 1 / 0 }
}
impl Iterable for Sequence {
    type Item = i32;
    type Iter = CollectionCursor<i32>;
    fn iter(self) -> CollectionCursor<i32> { self.items.iter() }
}
impl List<i32> for Sequence {
    fn len(self) -> usize { self.items.len() }
    fn is_empty(self) -> bool { self.items.is_empty() }
    fn get(self, index: usize) -> Option<i32> { self.items.get(index) }
}
impl MutableList<i32> for Sequence {
    fn push(self, value: i32) { self.items.push(value); }
    fn pop(self) -> Option<i32> { self.items.pop() }
    fn insert(self, index: usize, value: i32) { self.items.insert(index, value); }
    fn remove(self, index: usize) -> Option<i32> { self.items.remove(index) }
    fn clear(self) { self.items.clear(); }
    fn set(self, index: usize, value: i32) { self.items[index] = value; }
}
fn main() -> bool {
    val source = Sequence { items: [3, 1, 2, 1] };
    val list: List<i32> = source;
    val sorted = list.sorted_by_key(|item| item);
    if sorted[0] != 1 || source.items[0] != 3 { return false; }
    val writable: MutableList<i32> = source;
    writable.sort();
    writable.dedup();
    writable.reverse();
    writable.retain(|item| item != 2);
    source.items.len() == 2usize && source.items[0] == 3 && source.items[1] == 1
}
"#,
    );
}

#[test]
fn shared_calls_keep_key_types_and_stable_object_order_under_gc() {
    execute(
        r#"use std::cmp::{Ordering};
use std::collections::{List};

struct Item { val key: i32, val label: String, val ordinal: i32 }
struct Counter { var calls: i32 }
struct Rank { val value: i32 }
impl PartialEq for Rank { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl Eq for Rank {}
impl PartialOrd for Rank { fn partial_cmp(self, other: Self) -> Option<Ordering> { self.value.partial_cmp(other.value) } }
impl Ord for Rank { fn cmp(self, other: Self) -> Ordering { self.value.cmp(other.value) } }
trait Sorter {
    fn order<T, K: Ord>(self, values: List<T>, key: fn(T) -> K) -> List<T> {
        values.sorted_by_key(key)
    }
}
impl Sorter for i32 {}
fn main() -> bool {
    val values: List<Item> = [
        Item { key: 2, label: "b", ordinal: 0 },
        Item { key: 1, label: "a", ordinal: 1 },
        Item { key: 2, label: "b", ordinal: 2 },
        Item { key: 1, label: "a", ordinal: 3 }
    ];
    val calls = Counter { calls: 0 };
    val sorter: Sorter = 0;
    val numeric = sorter.order(values, |item| { calls.calls += 1; item.key });
    val text = sorter.order(values, |item| item.label);
    val objects = sorter.order(values, |item| Rank { value: item.key });
    numeric[0].ordinal == 1 && numeric[1].ordinal == 3 && numeric[2].ordinal == 0 && numeric[3].ordinal == 2
        && text[0].ordinal == 1 && text[1].ordinal == 3 && objects[0].ordinal == 1 && objects[1].ordinal == 3 && calls.calls > 4 && values[0].ordinal == 0
}
"#,
    );
}

#[test]
fn ordering_and_equality_bounds_are_required_at_the_method_call() {
    let engine = KagariEngine::default();
    for source in [
        "use std::collections::{List};\nstruct Item { val key: i32 } fn main() { val list: List<Item> = [Item { key: 1 }]; list.sorted(); }",
        "use std::collections::{List};\nfn main() { val list: List<f32> = [1.0f32]; list.distinct(); }",
    ] {
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("bounds.kgr", source), Default::default())
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn cancellation_during_callbacks_restores_storage_and_releases_roots() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    use {
        kagari_runtime::{
            gc::roots::RootedValue,
            native::{
                binding::NativeResult, builder::ModuleBuilder, context::CallContext,
                declarations::FunctionDecl, views::ValueHandle,
            },
        },
        kagari_stdlib::declarations::StandardDeclarations,
    };
    for (operation, retained_values) in [
        ("sort_by(|a,b| { visit(); a.cmp(b) })", vec![0, 1, 2, 3]),
        ("retain(|value| { visit(); value != 1 })", vec![0, 2, 3]),
    ] {
        let context = ExecutionContext::default();
        let mut module = ModuleBuilder::new(
            "test::cancellation",
            &StandardDeclarations::default()
                .catalog()
                .expect("explicit standard providers"),
        );
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let token = context.cancellation.clone();
        let visit = module.define_function(FunctionDecl::new("visit")).unwrap();
        module
            .bind(
                visit,
                move |_cx: &mut CallContext<'_>| -> NativeResult<()> {
                    count.set(count.get() + 1);
                    if count.get() == 3 {
                        token.cancel();
                    }
                    Ok(())
                },
            )
            .unwrap();
        let retained: Rc<RefCell<Option<RootedValue>>> = Rc::new(RefCell::new(None));
        let capture = retained.clone();
        let keep = module.define_function(FunctionDecl::new("keep")).unwrap();
        module
            .function(&keep, |function| {
                let parameter = function.type_parameter("T")?.ty();
                function.parameter("value", parameter);
                Ok(())
            })
            .unwrap();
        module
            .bind(
                keep,
                move |_cx: &mut CallContext<'_>, value: ValueHandle<'_>| -> NativeResult<()> {
                    *capture.borrow_mut() = Some(value.root()?);
                    Ok(())
                },
            )
            .unwrap();
        let mut builder = KagariEngine::builder().unwrap();
        builder.install(module.finish().unwrap()).unwrap();
        let engine = builder.build().unwrap();
        let source = format!(
            "use test::cancellation::{{keep, visit}}; fn main() {{ val values = [1,3,2,0]; keep(values); values.{operation}; }}"
        );
        let artifact = engine
            .compile_to_artifact(SourceFile::new("cancel.kgr", source), Default::default())
            .unwrap();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap_err()
                .code(),
            "KG_RUNTIME_CANCELLED"
        );
        assert_eq!(calls.get(), 3);
        let Value::Array(array) = retained
            .borrow()
            .as_ref()
            .unwrap()
            .value(runtime.runtime().gc())
            .unwrap()
        else {
            panic!("array")
        };
        let mut actual = runtime
            .runtime()
            .gc()
            .array_snapshot(array)
            .unwrap()
            .into_iter()
            .map(|value| {
                let Value::I32(value) = value else {
                    panic!("element")
                };
                value
            })
            .collect::<Vec<_>>();
        actual.sort();
        assert_eq!(actual, retained_values);
        runtime
            .runtime()
            .gc()
            .array_push(array, Value::I32(99))
            .unwrap();
        retained.borrow_mut().take();
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
        assert!(!runtime.runtime().is_quarantined());
    }
}

#[test]
fn generic_custom_receiver_uses_its_associated_iterator_in_default_calls() {
    execute(
        r#"use std::collections::{List};
use std::ops::{Index};

struct Sequence<T> { val items: Vec<T> }
struct Cursor<T> { val items: Vec<T>, var position: usize }
impl<T> Iterator for Cursor<T> {
    type Item = T;
    fn next(self) -> Option<T> {
        val value = self.items.get(self.position);
        self.position += 1usize;
        value
    }
}
impl<T> Index<usize> for Sequence<T> {
    type Output = T;
    fn index(self, index: usize) -> T { self.items[index] }
}
impl<T> Iterable for Sequence<T> {
    type Item = T;
    type Iter = Cursor<T>;
    fn iter(self) -> Cursor<T> { Cursor { items: self.items, position: 0usize } }
}
impl<T> List<T> for Sequence<T> {
    fn len(self) -> usize { self.items.len() }
    fn is_empty(self) -> bool { self.items.is_empty() }
    fn get(self, index: usize) -> Option<T> { self.items.get(index) }
}
fn order<T: Ord>(values: List<T>) -> List<T> { values.sorted() }
trait Sorter {
    fn order<T: Ord>(self, values: List<T>) -> List<T> { values.sorted() }
}
impl Sorter for i32 {}
fn main() -> bool {
    val source = Sequence { items: [3,1,2] };
    val sorted = source.sorted();
    val list: List<i32> = source;
    val sorter: Sorter = 0;
    val text: List<String> = Sequence { items: ["b", "a"] };
    sorted[0] == 1 && order(list)[0] == 1 && sorter.order(list)[0] == 1
        && sorter.order(text)[0] == "a" && list.reversed()[0] == 2
}
"#,
    );
}
