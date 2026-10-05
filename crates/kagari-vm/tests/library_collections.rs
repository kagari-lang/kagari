use kagari_bytecode::program::BytecodeProgram;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{
    analysis::AnalysisDatabase, declarations::DeclarationId, native::render::declaration_source,
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    gc::roots::RootedValue,
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, module::NativeModule, types::Type, views::ValueHandle,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_vm::vm::Vm;
use std::sync::Arc;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use {
    kagari_stdlib as foundation,
    kagari_stdlib::{catalog as foundation_catalog, declarations::StandardDeclarations},
};

mod library_mapping;
mod list_failures;

fn program(text: &str, modules: &[&NativeModule]) -> BytecodeProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("main.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation::modules()
            .unwrap()
            .iter()
            .filter(|installed| {
                modules
                    .iter()
                    .all(|module| module.declaration().identity != installed.declaration().identity)
            })
            .chain(modules.iter().copied())
            .map(|module| Arc::new(module.to_declaration().unwrap()))
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    lower_program_to_bytecode(&mir).unwrap()
}

fn run(text: &str) -> Value {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let loaded = runtime
        .load_program("sort", program(text, &[&library]))
        .unwrap();
    Vm::new(runtime)
        .execute(&loaded, "main")
        .unwrap()
        .return_value
}

#[test]
fn primitive_sort_and_supplied_comparator_preserve_shared_identity() {
    assert_eq!(
        run(r#"
        fn main() -> bool {
            val empty: Vec<i32> = []; empty.sort();
            val values = [3, 1, 2, 1]; val alias = values;
            values.sort();
            if alias[0] != 1 || alias[1] != 1 || alias[2] != 2 || alias[3] != 3 { return false; }
            values.sort_by(|a, b| b.cmp(a));
            alias[0] == 3 && alias[1] == 2 && alias[2] == 1 && alias[3] == 1
        }
    "#),
        Value::Bool(true)
    );
}

#[test]
fn script_ord_is_selected_and_equal_elements_remain_stable() {
    assert_eq!(
        run(r#"use std::cmp::{Ordering};

        struct Rank { val key: i32, val tag: i32, var visits: i32 }
        impl PartialEq for Rank { fn eq(self, other: Self) -> bool { self.key == other.key } }
        impl Eq for Rank {}
        impl PartialOrd for Rank { fn partial_cmp(self, other: Self) -> Option<Ordering> { self.key.partial_cmp(other.key) } }
        impl Ord for Rank { fn cmp(self, other: Self) -> Ordering { self.visits += 1; self.key.cmp(other.key) } }
        fn main() -> bool {
            val a = Rank { key: 2, tag: 0, visits: 0 };
            val b = Rank { key: 1, tag: 1, visits: 0 };
            val c = Rank { key: 1, tag: 2, visits: 0 };
            val d = Rank { key: 2, tag: 3, visits: 0 };
            val values = [a,b,c,d]; values.sort();
            values[0].tag == 1 && values[1].tag == 2 && values[2].tag == 0 && values[3].tag == 3
                && a.visits + b.visits + c.visits + d.visits > 0
        }
    "#),
        Value::Bool(true)
    );
}

struct Probe {
    module: NativeModule,
    retained: Arc<Mutex<Option<RootedValue>>>,
    calls: Arc<AtomicUsize>,
}

impl Probe {
    fn new() -> Self {
        let mut module = ModuleBuilder::new(
            "test::probe",
            &StandardDeclarations::default()
                .catalog()
                .expect("explicit standard providers"),
        );
        let retained = Arc::new(Mutex::new(None));
        let calls = Arc::new(AtomicUsize::new(0));
        let keep = module.define_function(FunctionDecl::new("keep")).unwrap();
        module
            .function(&keep, |function| {
                let item = function.type_parameter("T")?;
                function.parameter("value", item.ty());
                Ok(())
            })
            .unwrap();
        let capture = retained.clone();
        module
            .bind(
                keep,
                move |_cx: &mut CallContext<'_>, value: ValueHandle<'_>| -> NativeResult<()> {
                    *capture.lock().unwrap() = Some(value.root()?);
                    Ok(())
                },
            )
            .unwrap();
        let tick = module
            .define_function(FunctionDecl::new("tick").returns(Type::usize()))
            .unwrap();
        let count = calls.clone();
        module
            .bind(
                tick,
                move |cx: &mut CallContext<'_>| -> NativeResult<usize> {
                    count.fetch_add(1, Ordering::SeqCst);
                    cx.collect_garbage()?;
                    Ok(count.load(Ordering::SeqCst))
                },
            )
            .unwrap();
        Self {
            module: module.finish().unwrap(),
            retained,
            calls,
        }
    }
}

#[test]
fn comparator_failure_stops_callbacks_and_preserves_original_elements() {
    let probe = Probe::new();
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let source = r#"
        use test::probe::{keep, tick};
        struct Item { val key: i32, var visits: i32 }
        fn main() {
            val values = [Item { key: 3, visits: 0 }, Item { key: 1, visits: 0 }, Item { key: 2, visits: 0 }, Item { key: 0, visits: 0 }]; keep(values);
            values.sort_by(|a,b| { a.visits += 1; val count = tick(); if count == 3usize { val fail = 1 / 0; }; a.key.cmp(b.key) });
        }
    "#;
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    probe.module.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program("sort", program(source, &[&library, &probe.module]))
        .unwrap();
    let vm = Vm::new(runtime);
    assert!(vm.execute(&loaded, "main").is_err());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 3);
    let Value::Array(array) = probe
        .retained
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .value(vm.runtime().gc())
        .unwrap()
    else {
        panic!("array");
    };
    let values = vm.runtime().gc().array_snapshot(array).unwrap();
    let mut keys = Vec::new();
    let mut visits = 0;
    for value in &values {
        let Value::Struct(id) = value else {
            panic!("item");
        };
        let (_, fields) = vm.runtime().gc().struct_snapshot(*id).unwrap();
        for field in fields {
            if field.name == "key" {
                keys.push(field.value);
            } else if field.name == "visits" {
                let Value::I32(count) = field.value else {
                    panic!("visit count");
                };
                visits += count;
            }
        }
    }
    let mut keys: Vec<_> = keys
        .into_iter()
        .map(|value| match value {
            Value::I32(value) => value,
            _ => panic!("key"),
        })
        .collect();
    keys.sort();
    assert_eq!(keys, vec![0, 1, 2, 3]);
    assert_eq!(
        visits, 3,
        "completed effects on referenced payloads survive failure"
    );
    vm.runtime()
        .gc()
        .array_push(array, values[0].clone())
        .unwrap();
    probe.retained.lock().unwrap().take();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn callback_alias_writes_and_nested_edits_are_rejected_without_changing_slots() {
    for mutation in ["values[0] = 9;", "values.push(9);", "values.sort();"] {
        let probe = Probe::new();
        let library = foundation::modules()
            .unwrap()
            .into_iter()
            .find(|module| {
                module.declaration().identity
                    == kagari_stdlib::namespaces::module("std", "collections")
            })
            .unwrap();
        let source = format!(
            r#"
            use test::probe::keep;
            fn main() {{
                val values = [3,1,2]; keep(values);
                values.sort_by(|a,b| {{ {mutation} a.cmp(b) }});
            }}
        "#
        );
        let mut runtime = Runtime::default();
        kagari_runtime::native::module::NativeModule::install_all(
            &kagari_stdlib::modules().unwrap(),
            &mut runtime,
        )
        .unwrap();
        probe.module.install(&mut runtime).unwrap();
        let loaded = runtime
            .load_program("sort", program(&source, &[&library, &probe.module]))
            .unwrap();
        let vm = Vm::new(runtime);
        let error = vm.execute(&loaded, "main").unwrap_err();
        assert!(
            format!("{error:?}").contains("guarded callback"),
            "{error:?}"
        );
        let Value::Array(array) = probe
            .retained
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .value(vm.runtime().gc())
            .unwrap()
        else {
            panic!("array");
        };
        assert_eq!(
            vm.runtime().gc().array_snapshot(array),
            Some(vec![Value::I32(3), Value::I32(1), Value::I32(2)])
        );
        vm.runtime().gc().array_push(array, Value::I32(9)).unwrap();
    }
}

#[test]
fn primitive_selection_covers_unsigned_bounds_and_string_ordering() {
    assert_eq!(
        run(r#"
        fn main() -> bool {
            val values: Vec<u64> = [18446744073709551615u64, 0u64, 9223372036854775808u64];
            values.sort();
            if values[0] != 0u64 || values[2] != 18446744073709551615u64 { return false; }
            values.sort_by(|a,b| b.cmp(a));
            val text = ["z", "a", "a", "b"]; text.sort();
            values[0] == 18446744073709551615u64 && text[0] == "a" && text[3] == "z"
        }
    "#),
        Value::Bool(true)
    );
}

#[test]
fn scalar_ord_overrides_are_rejected_before_native_selection() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let mut sources = SourceDatabase::default();
    let file = sources
        .set(
            "invalid.kgr",
            r#"use std::cmp::{Ordering};

        impl Ord for i32 { fn cmp(self, other: Self) -> Ordering { Ordering::Equal } }
        fn main() { [1,3,2].sort(); }
    "#
            .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation_catalog::shared()
            .into_iter()
            .filter(|installed| installed.identity != library.declaration().identity)
            .chain([Arc::new(library.to_declaration().unwrap())])
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let error = snapshot
        .check_program(file, &Default::default())
        .unwrap_err();
    assert!(format!("{error:?}").contains("InvalidTraitImpl"));
}

#[test]
fn generated_library_declarations_supply_navigation_docs_and_exported_signatures() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let generated = declaration_source(
        &library.to_declaration().unwrap(),
        &foundation_catalog::shared(),
    )
    .unwrap();
    let text = "use std::collections::map; fn main() { val values = map([2,1], |value| value); }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("tooling.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation_catalog::shared()
            .into_iter()
            .filter(|installed| installed.identity != library.declaration().identity)
            .chain([Arc::new(library.to_declaration().unwrap())])
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let offset = text.find("map([2,1]").unwrap();
    let target = snapshot.definition_at(file, offset).unwrap();
    let source = snapshot.source(target.location.file).unwrap();
    assert_eq!(source.name(), generated.uri);
    assert_eq!(
        &source.text()[target.location.range.start..target.location.range.end],
        "map"
    );
    let documentation = snapshot.documentation_at(file, offset).unwrap();
    assert!(documentation.documentation.contains("Lazily transform"));
    assert!(documentation.written_signature.contains("MapIterator"));
    assert!(documentation.written_signature.contains("Vec"));
    // visible_bindings is a lexical-local query. Module functions are exposed
    // through the declaration inventory.
    for name in ["map", "MapIterator"] {
        let (id, site) = generated
            .sites
            .iter()
            .find(|(id, _)| id.path.last().is_some_and(|part| part.name == name))
            .unwrap();
        let declaration = snapshot
            .declaration(&DeclarationId::Definition(id.clone()))
            .unwrap();
        assert_eq!(declaration.name, name);
        assert_eq!(declaration.location.range, site.name_span);
    }
}
