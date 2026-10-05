use super::{Probe, program, run};
use kagari_hir::{analysis::AnalysisDatabase, native::render::declaration_source};
use kagari_runtime::{Runtime, RuntimeConfig, value::Value};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{declaration::module::ModuleDecl, language, language::Protocol};
use kagari_vm::vm::Vm;
use std::sync::Arc;
use {kagari_stdlib as foundation, kagari_stdlib::catalog as foundation_catalog};

#[test]
fn map_is_lazy_and_aliases_share_cursor_progress_and_gc_captures() {
    assert_eq!(
        run(r#"
        use std::collections::map;
        fn main() -> bool {
            val calls = [0]; val offset = [1];
            val mapped = map([10,20,30], |item| { calls[0] += 1; [item + offset[0]] });
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
fn break_releases_wrapped_sources_and_dynamic_views_preserve_shared_progress() {
    for erased in [false, true] {
        let annotation = if erased { ": Iterator<Item = i32>" } else { "" };
        assert_eq!(
            run(&format!(
                r#"
            use std::collections::map;
            fn main() -> i32 {{
                val values = [10,20,30]; val mapped{annotation} = map(values, |item| item + 1);
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
fn for_scope_protects_wrapped_sources_and_failure_releases_the_guards() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let probe = Probe::new();
    let source = r#"
        use std::collections::map; use test::probe::keep;
        fn main() { val values = [1,2]; keep(values); for item in map(values, |item| item) { values.push(item); } }
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
    let mut vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();
    assert!(format!("{error:?}").contains("structural modification during iteration"));
    let Value::Array(array) = probe
        .retained
        .borrow()
        .as_ref()
        .unwrap()
        .value(vm.runtime().gc())
        .unwrap()
    else {
        panic!("array");
    };
    assert_eq!(vm.runtime().gc().array_len(array), Some(2));
    vm.runtime().gc().array_push(array, Value::I32(3)).unwrap();
    probe.retained.borrow_mut().take();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn callback_failure_consumes_once_and_releases_callback_and_iteration_scopes() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let probe = Probe::new();
    let source = r#"
        use std::collections::map; use test::probe::tick;
        fn make() -> Iterator<Item = i32> {
            map([10,20], |item| { if tick() == 1usize { val fail = 1 / 0; }; item + 1 })
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
    let mut vm = Vm::new(runtime);
    let iterator = vm.execute(&loaded, "make").unwrap().return_value;
    let root = vm.runtime().root_value(iterator.clone()).unwrap();
    let next = ModuleDecl::method_id(&language::identity(Protocol::Iterator), "next");
    assert!(vm.invoke_interface_method(&iterator, &next, &[]).is_err());
    assert_eq!(probe.calls.get(), 1);
    let result = vm.invoke_interface_method(&iterator, &next, &[]).unwrap();
    let Value::Enum(id) = result else {
        panic!("Option");
    };
    let result = vm.runtime().gc().enum_snapshot(id).unwrap();
    assert_eq!(result.tag.variant_name(), "Some");
    assert_eq!(result.fields, vec![Value::I32(21)]);
    assert_eq!(probe.calls.get(), 2);
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn native_map_next_does_not_allocate_an_intermediate_option() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = None;
    let mut runtime = Runtime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let loaded = runtime.load_program("map", program("use std::collections::map; fn make() -> Iterator<Item = i32> { map([42], |item| item) }", &[&library])).unwrap();
    let mut vm = Vm::new(runtime);
    let iterator = vm.execute(&loaded, "make").unwrap().return_value;
    let root = vm.runtime().root_value(iterator.clone()).unwrap();
    let next = ModuleDecl::method_id(&language::identity(Protocol::Iterator), "next");
    let before = vm.runtime().gc().stats();
    let result = vm.invoke_interface_method(&iterator, &next, &[]).unwrap();
    let after = vm.runtime().gc().stats();
    assert_eq!(after.allocated_objects - before.allocated_objects, 1);

    let Value::Enum(id) = result else {
        panic!("Option");
    };
    assert_eq!(
        vm.runtime().gc().enum_snapshot(id).unwrap().fields,
        vec![Value::I32(42)]
    );
    drop(root);
}

#[test]
fn recursive_next_is_rejected_and_unreachable_capture_cycles_are_collected() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let source = r#"
        use std::collections::{map, MapIterator};
        struct Holder { var cursor: Option<MapIterator<i32,i32>> }
        fn main() {
            val holder = Holder { cursor: None };
            val mapped = map([1,2], |item| { match holder.cursor { Some(cursor) => { cursor.next(); }, None => {} }; item });
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
    let mut vm = Vm::new(runtime);
    let error = vm.execute(&loaded, "main").unwrap_err();
    assert!(
        format!("{error:?}").contains("recursive next on the same lazy iterator"),
        "{error:?}"
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn retained_map_uses_its_original_callback_after_reload() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| {
            module.declaration().identity == kagari_stdlib::namespaces::module("std", "collections")
        })
        .unwrap();
    let source = "use std::collections::map; fn make() -> Iterator<Item = i32> { val offset = [1]; map([10,20], |item| item + offset[0]) }";
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
    let mut vm = Vm::new(runtime);
    let iterator = vm.execute(&old, "make").unwrap().return_value;
    let root = vm.runtime().root_value(iterator.clone()).unwrap();
    let replacement = vm
        .reload_program(
            &old,
            "map",
            program(&source.replace("[1]", "[100]"), &[&library]),
        )
        .unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values
            > 0
    );
    let next = ModuleDecl::method_id(&language::identity(Protocol::Iterator), "next");
    let Value::Enum(id) = vm.invoke_interface_method(&iterator, &next, &[]).unwrap() else {
        panic!("Option");
    };
    assert_eq!(
        vm.runtime().gc().enum_snapshot(id).unwrap().fields,
        vec![Value::I32(11)]
    );
    let fresh = vm.execute(&replacement, "make").unwrap().return_value;
    let Value::Enum(id) = vm.invoke_interface_method(&fresh, &next, &[]).unwrap() else {
        panic!("Option");
    };
    assert_eq!(
        vm.runtime().gc().enum_snapshot(id).unwrap().fields,
        vec![Value::I32(110)]
    );
    drop(root);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
}

#[test]
fn native_iterator_completion_navigates_to_the_generated_impl() {
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
    let text = "use std::collections::map; fn main() { val cursor = map([1], |x| x); cursor. }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("completion.kgr", text.into(), SourceLayer::Base)
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
