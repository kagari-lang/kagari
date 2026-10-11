use super::{Probe, program, run};
use kagari_hir::{analysis::AnalysisDatabase, native::render::declaration_source};
use kagari_runtime::{Runtime, RuntimeConfig, value::Value};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{declaration::module::ModuleDecl, language::Protocol};
use kagari_vm::vm::Vm;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use {kagari_stdlib as foundation, kagari_stdlib::catalog as foundation_catalog};

#[test]
fn map_is_lazy_and_aliases_share_cursor_progress_and_gc_captures() {
    assert_eq!(
        run(r#"

        fn main() -> bool {
            val calls = Vec::from([0]); val offset = Vec::from([1]);
            val mapped = Vec::from([10,20,30]).map(|item| { calls[0] += 1; Vec::from([item + offset[0]]) });
            if calls[0] != 0 { return false; }
            val alias = mapped;
            val first = match mapped.next() { Some(value) => value[0], None => -1 };
            val second = match alias.next() { Some(value) => value[0], None => -1 };
            var rest = 0; for item in mapped { rest += item[0]; }
            val ended = match alias.next() { Some(_) => false, None => true };
            first == 11 && second == 21 && rest == 31 && calls[0] == 3 && ended
        }
    "#),
        Value::Bool(true)
    );
}

#[test]
fn break_allows_source_writes_and_dynamic_views_preserve_shared_progress() {
    for erased in [false, true] {
        let annotation = if erased { ": Iterator<Item = i32>" } else { "" };
        assert_eq!(
            run(&format!(
                r#"

            fn main() -> i32 {{
                val values = Vec::from([10,20,30]); val mapped{annotation} = values.map(|item| item + 1);
                var first = 0; for item in mapped {{ first = item; break; }}
                val second = match mapped.next() {{ Some(item) => item, None => -1 }};
                for item in mapped {{ break; }}
                values.push(40);
                first + second + values[3]
            }}
        "#
            )),
            Value::I32(72)
        );
    }
}

#[test]
fn wrapped_sources_commit_edits_then_fail_before_the_next_read() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("core", "iter")
        })
        .unwrap();
    let probe = Probe::new();
    let source = r#"
         use test::probe::keep;
        fn main() { val values = Vec::from([1,2]); keep(values); for item in [4,5].map(|item| item).zip(values.filter(|item| item > 0)).enumerate() { values.push(item[0] as i32); } }
    "#;
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    probe.module.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program("map", program(source, &[&library, &probe.module]))
        .unwrap();
    let vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();
    assert!(format!("{error:?}").contains("invalid or structurally modified iterator"));
    let Value::GcHandle(array) = probe
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
    assert_eq!(vm.runtime().gc().sequence_len(array), Some(3));
    vm.runtime()
        .gc()
        .sequence_push(array, Value::I32(3))
        .unwrap();
    probe.retained.lock().unwrap().take();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn callback_failure_consumes_once_and_releases_callback_and_iteration_scopes() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("core", "iter")
        })
        .unwrap();
    let probe = Probe::new();
    let source = r#"
         use test::probe::tick;
        fn make() -> Iterator<Item = (i32,i32)> {
            Vec::from([100,200]).map(|item| item).zip(Vec::from([10,20]).map(|item| { if tick() == 1usize { val fail = 1 / 0; }; item + 1 }))
        }
    "#;
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    probe.module.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program("map", program(source, &[&library, &probe.module]))
        .unwrap();
    let vm = Vm::new(runtime);
    let iterator = vm
        .execute(&loaded, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(iterator).unwrap();
    let definitions = vm.runtime().host().definitions();
    let next = ModuleDecl::method_id(
        Protocol::Iterator.definition(&definitions).unwrap(),
        "next",
        &definitions,
    )
    .unwrap();
    assert!(vm.invoke_interface_method(&iterator, &next, &[]).is_err());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    let result = vm.invoke_interface_method(&iterator, &next, &[]).unwrap();
    let Value::Enum(id) = result else {
        panic!("Option");
    };
    let result = vm.runtime().gc().enum_snapshot(id).unwrap();
    assert_eq!(result.tag.variant_name(), "Some");
    let Value::Tuple(pair) = result.fields[0] else {
        panic!("pair");
    };
    assert_eq!(
        &*vm.runtime().gc().tuple(pair).unwrap(),
        &[Value::I32(200), Value::I32(21)]
    );
    assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn map_invokes_the_selected_custom_source_and_releases_its_roots() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("core", "iter")
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
    let loaded = runtime.load_program("map", program(r#"struct Source { var done: bool } impl Iterator for Source { type Item=i32; fn next(self)->Option<i32> { if self.done { None } else { self.done=true; Some(42) } } } fn make()->Iterator<Item=i32> { Source { done:false }.map(|item|item) }"#, &[&library])).unwrap();
    let vm = Vm::new(runtime);
    let iterator = vm
        .execute(&loaded, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(iterator).unwrap();
    let definitions = vm.runtime().host().definitions();
    let next = ModuleDecl::method_id(
        Protocol::Iterator.definition(&definitions).unwrap(),
        "next",
        &definitions,
    )
    .unwrap();
    let result = vm.invoke_interface_method(&iterator, &next, &[]).unwrap();

    let Value::Enum(id) = result else {
        panic!("Option");
    };
    assert_eq!(
        vm.runtime().gc().enum_snapshot(id).unwrap().fields,
        vec![Value::I32(42)]
    );
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn recursive_next_is_rejected_and_unreachable_capture_cycles_are_collected() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("core", "iter")
        })
        .unwrap();
    let source = r#"
        use core::iter::MapIterator;
        struct Holder { var cursor: Option<MapIterator<i32,i32>> }
        fn main() {
            val holder = Holder { cursor: None };
            val mapped = Vec::from([1,2]).map(|item| { match holder.cursor { Some(cursor) => { cursor.next(); }, None => {} }; item });
            holder.cursor = Some(mapped); mapped.next();
        }
    "#;
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let loaded = runtime
        .load_program("map", program(source, &[&library]))
        .unwrap();
    let vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();
    assert!(
        format!("{error:?}").contains("recursive next on the same lazy iterator"),
        "{error:?}"
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn retained_zip_uses_both_original_custom_sources_and_callbacks_after_reload() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("core", "iter")
        })
        .unwrap();
    let source = r#"
fn bias()->i32{0}
struct Source { val values: Vec<i32>, var position: usize }
impl Iterator for Source {
    type Item=i32;
    fn next(self)->Option<i32> {
        val value=self.values.get(self.position); self.position+=1usize;
        match value { Some(item)=>Some(item+bias()), None=>None }
    }
}
fn make()->Iterator<Item=(i32,i32)> {
    val offset=Vec::from([1]);
    Source { values:Vec::from([10,20]),position:0usize }.map(|item|item+offset[0])
        .zip(Source {values:Vec::from([30,40]),position:0usize}.map(|item|item+offset[0]))
}
"#;
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let old = runtime
        .load_program("map", program(source, &[&library]))
        .unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let iterator = vm
        .execute(&old, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(iterator).unwrap();
    let replacement = vm
        .reload_program(
            &old,
            "map",
            program(
                &source
                    .replace("[1]", "[100]")
                    .replace("fn bias()->i32{0}", "fn bias()->i32{100}"),
                &[&library],
            ),
        )
        .unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let definitions = vm.runtime().host().definitions();
    let next = ModuleDecl::method_id(
        Protocol::Iterator.definition(&definitions).unwrap(),
        "next",
        &definitions,
    )
    .unwrap();
    let Value::Enum(id) = vm.invoke_interface_method(&iterator, &next, &[]).unwrap() else {
        panic!("Option");
    };
    let Value::Tuple(pair) = vm.runtime().gc().enum_snapshot(id).unwrap().fields[0] else {
        panic!("pair");
    };
    assert_eq!(
        &*vm.runtime().gc().tuple(pair).unwrap(),
        &[Value::I32(11), Value::I32(31)]
    );
    let fresh = vm
        .execute(&replacement, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let Value::Enum(id) = vm.invoke_interface_method(&fresh, &next, &[]).unwrap() else {
        panic!("Option");
    };
    let Value::Tuple(pair) = vm.runtime().gc().enum_snapshot(id).unwrap().fields[0] else {
        panic!("pair");
    };
    assert_eq!(
        &*vm.runtime().gc().tuple(pair).unwrap(),
        &[Value::I32(210), Value::I32(230)]
    );
    drop(root);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
    assert!(collected.reclaimed_modules.contains(&old_key));
    assert!(vm.runtime().modules().loaded(old_key).is_none());
}

#[test]
fn native_iterator_completion_navigates_to_the_generated_impl() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("core", "iter")
        })
        .unwrap();
    let generated = declaration_source(
        &library.to_declaration(),
        &foundation_catalog::shared().unwrap(),
    )
    .unwrap();
    let text = " fn main() { val cursor = Vec::from([1]).map(|x| x); cursor. }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("completion.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation_catalog::shared()
            .unwrap()
            .into_iter()
            .filter(|installed| installed.records().identity != library.declaration().identity)
            .chain([Arc::new(library.to_declaration())])
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let candidates = snapshot
        .file(file)
        .unwrap()
        .method_completions(text.find("cursor. }").unwrap() + 7);
    let next = candidates
        .iter()
        .find(|candidate| candidate.name == "next")
        .expect("next completion");
    let declaration = snapshot.declaration(&next.declaration).unwrap();
    let source = snapshot.source(declaration.location.file).unwrap();
    assert_eq!(source.name(), generated.uri);
    assert_eq!(
        &source.text()[declaration.location.range.start..declaration.location.range.end],
        "next"
    );
}
