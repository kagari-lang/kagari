//! Repeated dispatch must not copy metadata for unused interface methods.
use crate::bytecode_fixture::load_bytecode;

use super::native_allocations_counter::{measured, verify_counter};
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::identity::DefinitionKind;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    module::{ExecutableModule, InstalledProgram},
    native::module::NativeModule,
    value::carriers::RootedValue,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib as foundation;

use std::{hint::black_box, sync::Arc};

#[test]
fn unused_interface_methods_do_not_increase_repeated_dispatch_allocations() {
    verify_counter();
    let mut baseline = None;
    for width in [1, 8, 32] {
        let mut source = String::from("trait Bench {");
        for index in 0..width {
            source.push_str(&format!("fn m{index}(self) -> i32;"));
        }
        source.push_str("} struct Worker {} impl Bench for Worker {");
        for index in 0..width {
            source.push_str(&format!("fn m{index}(self) -> i32 {{ 1 }}"));
        }
        source.push_str(
            "} pub fn make() -> Bench { Worker {} } fn call(value: Bench) -> i32 { value.m0() }",
        );
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("interface-width.kgr", source, SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let checked = snapshot.check_program(root, &Default::default()).unwrap();
        let mir = lower_program_to_mir(
            &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
            &Default::default(),
        )
        .unwrap();
        let program = lower_program_to_bytecode(&mir, &Default::default())
            .map(|program| program.into_unverified())
            .unwrap();
        let main = program.records().modules[program.records().root.index()]
            .functions
            .iter()
            .find(|function| function.name == "call")
            .unwrap();
        assert!(main.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::Call {
                callee: CallTarget::InterfaceMethod { .. },
                ..
            }
        )));
        let mut runtime = dispatch_runtime();
        NativeModule::install_all(&foundation::modules().unwrap(), &mut runtime).unwrap();
        let loaded = load_bytecode(&mut runtime, "interface-width", program).unwrap();
        let _loaded_program = loaded;
        let installed = _loaded_program.clone();
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let contract = loaded
            .bytecode
            .trait_contracts
            .iter()
            .find(|contract| contract.abi.name == "Bench")
            .unwrap();
        let method = loaded
            .definitions()
            .lookup_child(contract.declaration, DefinitionKind::Method, "m0", 0)
            .unwrap();
        let mut vm = runtime;
        vm.collect_garbage().unwrap();
        let published = vm.gc_stats();
        let receiver = vm
            .execute(&installed, "make", &[], &Default::default())
            .unwrap()
            .return_value;
        for _ in 0..5 {
            assert_eq!(
                vm.invoke_interface_method(&receiver, &method, &[])
                    .unwrap()
                    .as_i32(),
                Some(1)
            );
        }
        let (counts, elapsed) = measured(|| {
            for _ in 0..1000 {
                assert_eq!(
                    black_box(
                        vm.invoke_interface_method(&receiver, &method, &[])
                            .unwrap()
                            .as_i32()
                    ),
                    Some(1)
                );
            }
        });
        println!(
            "width={width},calls=1000,ns={},counts={counts:?}",
            elapsed.as_nanos()
        );
        let allocation_shape = (counts.allocations, counts.requested_bytes);
        if let Some(expected) = baseline {
            assert_eq!(allocation_shape, expected, "unused method width {width}");
        } else {
            baseline = Some(allocation_shape);
        }
        drop(receiver);
        retire_preparation(&mut vm, &installed);
        assert_eq!(
            vm.gc_stats().root_registrations,
            published.root_registrations
        );
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc_stats().objects, published.objects);
    }
}

#[test]
fn inherited_closed_dispatch_cost_does_not_scale_with_unused_methods() {
    verify_counter();
    let mut baseline = None;
    for width in [1, 8, 32] {
        let mut source = String::from(
            "trait Parent<T> { fn identity(self, value: T) -> T { value } } trait Child: Parent<i32> {",
        );
        for index in 0..width {
            source.push_str(&format!("fn m{index}(self) -> i32;"));
        }
        source
            .push_str("} struct Worker {} impl Parent<i32> for Worker {} impl Child for Worker {");
        for index in 0..width {
            source.push_str(&format!("fn m{index}(self) -> i32 {{ 1 }}"));
        }
        source.push_str("} pub fn make() -> Child { Worker {} }");
        let (mut vm, installed, loaded) = load(&source);
        vm.collect_garbage().unwrap();
        let published = vm.gc_stats();
        let declaration = loaded
            .bytecode
            .trait_contracts
            .iter()
            .find(|contract| contract.abi.name == "Parent")
            .unwrap();
        let method = loaded
            .definitions()
            .lookup_child(
                declaration.declaration,
                DefinitionKind::Method,
                "identity",
                0,
            )
            .unwrap();
        let receiver = vm
            .execute(&installed, "make", &[], &Default::default())
            .unwrap()
            .return_value;
        for _ in 0..5 {
            assert_eq!(
                vm.invoke_interface_method(&receiver, &method, &[RootedValue::from(42i32)])
                    .unwrap()
                    .as_i32(),
                Some(42)
            );
        }
        vm.collect_garbage().unwrap();
        let (counts, _) = measured(|| {
            for _ in 0..1000 {
                assert_eq!(
                    vm.invoke_interface_method(&receiver, &method, &[RootedValue::from(42i32)])
                        .unwrap()
                        .as_i32(),
                    Some(42)
                );
            }
        });
        println!("inherited-width={width},calls=1000,counts={counts:?}");
        let shape = (counts.allocations, counts.requested_bytes);
        if let Some(expected) = baseline {
            assert_eq!(shape, expected);
        } else {
            baseline = Some(shape);
        }
        // Complete-graph collection also counts per-call admitted computation
        // objects. Exact parent-provider reuse is checked at Runtime's private
        // owner; this consumer measures width and eventual publication cleanup.
        drop(receiver);
        retire_preparation(&mut vm, &installed);
        assert_eq!(
            vm.gc_stats().root_registrations,
            published.root_registrations
        );
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc_stats().objects, published.objects);
        assert_eq!(
            vm.modules().retention_counts(loaded.key()).runtime_values,
            0
        );
    }
}

#[test]
fn cached_native_receiver_defaults_release_their_program_and_gc_values() {
    let (mut vm, installed, loaded) = load(include_str!(
        "../fixtures/native/allocation-native-receiver-default.kgr"
    ));
    vm.collect_garbage().unwrap();
    let published = vm.gc_stats();
    let methods = ["reversed", "len"].map(|name| {
        loaded
            .members()
            .find_map(|member| {
                member
                    .bytecode
                    .interface_tables
                    .iter()
                    .flat_map(|table| &table.methods)
                    .find(|slot| {
                        let view = member.definitions().resolve(slot.method).unwrap();
                        let segments = view.segments().collect::<Vec<_>>();
                        segments.last().is_some_and(|part| part.name == name)
                            && segments
                                .iter()
                                .rev()
                                .nth(1)
                                .is_some_and(|part| part.name == "List")
                    })
                    .map(|slot| slot.method)
            })
            .unwrap()
    });
    let generations = loaded
        .members()
        .map(|member| member.key())
        .collect::<Vec<_>>();
    let receiver = vm
        .execute(&installed, "make", &[], &Default::default())
        .unwrap()
        .return_value;
    for _ in 0..3 {
        let reversed = vm
            .invoke_interface_method(&receiver, &methods[0], &[])
            .unwrap();
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.invoke_interface_method(&reversed, &methods[1], &[])
                .unwrap()
                .as_u64(),
            Some(3)
        );
        drop(reversed);
    }
    drop(receiver);
    retire_preparation(&mut vm, &installed);
    assert_eq!(
        vm.gc_stats().root_registrations,
        published.root_registrations
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, published.objects);
    for generation in generations {
        assert_eq!(vm.modules().retention_counts(generation).runtime_values, 0);
    }
}

fn retire_preparation(runtime: &mut Runtime, installed: &InstalledProgram) {
    let selected = runtime.select_program(installed).unwrap();
    let replacement = runtime
        .stage_reload_verified_program(installed, None, selected.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(replacement).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.validate_executable_module(&selected).is_err());
}

fn load(source: &str) -> (Runtime, InstalledProgram, ExecutableModule) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("interface-cache.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation::modules()
            .unwrap()
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let program = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut runtime = dispatch_runtime();
    NativeModule::install_all(&foundation::modules().unwrap(), &mut runtime).unwrap();
    let loaded = load_bytecode(&mut runtime, "interface-cache", program).unwrap();
    let _loaded_program = loaded;
    let _installed = _loaded_program.clone();
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    (runtime, _loaded_program, loaded)
}

fn dispatch_runtime() -> Runtime {
    // Isolate repeated dispatch from whole-program metadata collection. Wider
    // declarations legitimately make a GC traversal larger; explicit collections
    // below still verify that dispatch leaves no wrappers or roots behind.
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = None;
    Runtime::new(config)
}
