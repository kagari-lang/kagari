use super::{Probe, program, run};
use super::{after_failure_source, array_i32, inspect_retained};
use kagari_runtime::{
    Runtime, RuntimeConfig, module::entry::ProgramEntry,
    native::conversion::context::ConversionContext,
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::{catalog as foundation_catalog, tests::support::bytecode::reload_bytecode};

use crate as foundation;
use kagari_hir::{analysis::AnalysisDatabase, native::render::declaration_source};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{declaration::module::ModuleDecl, language::Protocol};
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[test]
fn map_is_lazy_and_aliases_share_cursor_progress_and_gc_captures() {
    assert!(run::<bool>(include_str!(
        "../../../../tests/fixtures/collections/map-aliases.kgr"
    )));
}

#[test]
fn break_allows_source_writes_and_dynamic_views_preserve_shared_progress() {
    for source in [
        include_str!("../../../../tests/fixtures/collections/mapped-break-static.kgr"),
        include_str!("../../../../tests/fixtures/collections/mapped-break-dynamic.kgr"),
    ] {
        assert_eq!(run::<i32>(source), 72);
    }
}

#[test]
fn wrapped_sources_commit_edits_then_fail_before_the_next_read() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| module.declaration().identity == crate::namespaces::module("core", "iter"))
        .unwrap();
    let probe = Probe::new();
    let source = include_str!("../../../../tests/fixtures/collections/wrapped-source-failure.kgr");
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    probe.module.install(&mut runtime).unwrap();
    let loaded = load_bytecode(
        &mut runtime,
        "map",
        program(
            &format!("{source} {}", after_failure_source("i32", 3)),
            &[&library, &probe.module],
        ),
    )
    .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let error = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap_err();
    assert!(format!("{error:?}").contains("invalid or structurally modified iterator"));
    let snapshot = inspect_retained(&mut vm, &loaded, &probe);
    assert_eq!(array_i32(&mut vm, &loaded, &snapshot).len(), 3);
    drop(snapshot);
    probe.retained.lock().unwrap().take();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn callback_failure_consumes_once_and_releases_callback_and_iteration_scopes() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| module.declaration().identity == crate::namespaces::module("core", "iter"))
        .unwrap();
    let probe = Probe::new();
    let source = include_str!("../../../../tests/fixtures/collections/mapped-callback-failure.kgr");
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    probe.module.install(&mut runtime).unwrap();
    let loaded = load_bytecode(
        &mut runtime,
        "map",
        program(source, &[&library, &probe.module]),
    )
    .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let iterator = vm
        .execute_module(&loaded, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let definitions = vm.host().definitions();
    let next = ModuleDecl::method_id(
        Protocol::Iterator.definition(&definitions).unwrap(),
        "next",
        &definitions,
    )
    .unwrap();
    assert!(vm.invoke_interface_method(&iterator, &next, &[]).is_err());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    let observed = {
        let result = vm.invoke_interface_method(&iterator, &next, &[]).unwrap();
        ConversionContext::new(&mut vm, &loaded)
            .unwrap()
            .decode::<Option<(i32, i32)>>(&result)
            .unwrap()
    };
    assert_eq!(observed, Some((200, 21)));
    assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
    drop(iterator);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn map_invokes_the_selected_custom_source_and_releases_its_roots() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| module.declaration().identity == crate::namespaces::module("core", "iter"))
        .unwrap();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let loaded = load_bytecode(
        &mut runtime,
        "map",
        program(
            include_str!("../../../../tests/fixtures/collections/mapped-custom-source.kgr"),
            &[&library],
        ),
    )
    .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let iterator = vm
        .execute_module(&loaded, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let definitions = vm.host().definitions();
    let next = ModuleDecl::method_id(
        Protocol::Iterator.definition(&definitions).unwrap(),
        "next",
        &definitions,
    )
    .unwrap();
    let observed = {
        let result = vm.invoke_interface_method(&iterator, &next, &[]).unwrap();
        ConversionContext::new(&mut vm, &loaded)
            .unwrap()
            .decode::<Option<i32>>(&result)
            .unwrap()
    };
    assert_eq!(observed, Some(42));
    drop(iterator);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn recursive_next_is_rejected_and_unreachable_capture_cycles_are_collected() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| module.declaration().identity == crate::namespaces::module("core", "iter"))
        .unwrap();
    let source = include_str!("../../../../tests/fixtures/collections/mapped-recursive-next.kgr");
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let loaded = load_bytecode(&mut runtime, "map", program(source, &[&library])).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let error = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("recursive next on the same lazy iterator"),
        "{error:?}"
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn retained_zip_keeps_sources_and_closures_but_calls_current_methods() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| module.declaration().identity == crate::namespaces::module("core", "iter"))
        .unwrap();
    let source = include_str!("../../../../tests/fixtures/collections/retained-custom-zip.kgr");
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let old = load_bytecode(&mut runtime, "map", program(source, &[&library])).unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let old_key = old.key();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let iterator = vm
        .execute_module(&old, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let bias = "fn bias() -> i32 {\n    0\n}";
    assert_eq!(source.matches("[1]").count(), 1);
    assert_eq!(source.matches(bias).count(), 1);
    let updated_source = source
        .replace("[1]", "[100]")
        .replace(bias, "fn bias() -> i32 {\n    100\n}");
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("map".into()),
        program(&updated_source, &[&library]),
    )
    .unwrap();
    let replacement = vm.select_program(&_old_program).unwrap();
    drop(old);
    vm.collect_garbage().unwrap();
    assert!(vm.modules().loaded(old_key).is_some());
    let definitions = vm.host().definitions();
    let next = ModuleDecl::method_id(
        Protocol::Iterator.definition(&definitions).unwrap(),
        "next",
        &definitions,
    )
    .unwrap();
    let observed = {
        let result = vm.invoke_interface_method(&iterator, &next, &[]).unwrap();
        ConversionContext::new(&mut vm, &replacement)
            .unwrap()
            .decode::<Option<(i32, i32)>>(&result)
            .unwrap()
    };
    assert_eq!(observed, Some((111, 131)));
    let fresh = vm
        .execute_module(&replacement, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let observed = {
        let result = vm.invoke_interface_method(&fresh, &next, &[]).unwrap();
        ConversionContext::new(&mut vm, &replacement)
            .unwrap()
            .decode::<Option<(i32, i32)>>(&result)
            .unwrap()
    };
    assert_eq!(observed, Some((210, 230)));
    drop((iterator, fresh));
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(vm.modules().loaded(old_key).is_none());
}

#[test]
fn native_iterator_completion_navigates_to_the_generated_impl() {
    let library = foundation::modules()
        .unwrap()
        .into_iter()
        .find(|module| module.declaration().identity == crate::namespaces::module("core", "iter"))
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
