//! Repeated dispatch must not copy metadata for unused interface methods.
mod native_allocations_counter;

use kagari_abi::declaration::ModuleDecl;
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::source_database::{SourceDatabase, SourceLayer};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{Runtime, module::LoadedModule, value::Value};
use kagari_vm::vm::Vm;
use native_allocations_counter::{measured, verify_counter};
use std::hint::black_box;

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
            "} fn make() -> Bench { Worker {} } fn call(value: Bench) -> i32 { value.m0() }",
        );
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("interface-width.kgr", source, SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let checked = snapshot.check_program(root, &Default::default()).unwrap();
        let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
        let program = lower_program_to_bytecode(&mir).unwrap();
        let main = program.modules[program.root.index()]
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
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("interface-width", program).unwrap();
        let contract = loaded
            .bytecode
            .trait_contracts
            .iter()
            .find(|contract| contract.abi.name == "Bench")
            .unwrap();
        let method = ModuleDecl::method_id(
            &loaded
                .definitions()
                .resolve(contract.declaration)
                .unwrap()
                .to_path(),
            "m0",
        );
        let mut vm = Vm::new(runtime);
        let receiver = vm.execute(&loaded, "make").unwrap().return_value;
        let root = vm.runtime().root_value(receiver.clone()).unwrap();
        for _ in 0..5 {
            assert_eq!(
                vm.invoke_interface_method(&receiver, &method, &[]).unwrap(),
                Value::I32(1)
            );
        }
        let (counts, elapsed) = measured(|| {
            for _ in 0..1000 {
                assert_eq!(
                    black_box(vm.invoke_interface_method(&receiver, &method, &[]).unwrap()),
                    Value::I32(1)
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
        drop(root);
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    }
}

#[test]
fn inherited_closed_dispatch_does_not_allocate_parent_wrappers_or_copy_unused_methods() {
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
        source.push_str("} fn make() -> Child { Worker {} }");
        let (mut vm, loaded) = load(&source);
        let declaration = loaded
            .bytecode
            .trait_contracts
            .iter()
            .find(|contract| contract.abi.name == "Parent")
            .unwrap();
        let method = ModuleDecl::method_id(
            &loaded
                .definitions()
                .resolve(declaration.declaration)
                .unwrap()
                .to_path(),
            "identity",
        );
        let receiver = vm.execute(&loaded, "make").unwrap().return_value;
        let root = vm.runtime().root_value(receiver.clone()).unwrap();
        for _ in 0..5 {
            assert_eq!(
                vm.invoke_interface_method(&receiver, &method, &[Value::I32(42)])
                    .unwrap(),
                Value::I32(42)
            );
        }
        let live = vm.runtime().collect_garbage().unwrap().live_objects;
        let before = vm.runtime().gc().stats();
        let (counts, _) = measured(|| {
            for _ in 0..1000 {
                assert_eq!(
                    vm.invoke_interface_method(&receiver, &method, &[Value::I32(42)])
                        .unwrap(),
                    Value::I32(42)
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
        // Include reclaimed objects: automatic collection must not hide wrappers.
        let after = vm.runtime().gc().stats();
        assert_eq!(
            after.allocated_objects + after.reclaimed_objects,
            before.allocated_objects + before.reclaimed_objects
        );
        // Virtual parent dispatch must not publish additional GC interface values.
        assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, live);
        drop(root);
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
        assert_eq!(
            vm.runtime()
                .modules()
                .retention_counts(loaded.key())
                .runtime_values,
            0
        );
    }
}

#[test]
fn cached_native_receiver_defaults_release_their_program_and_gc_values() {
    let (mut vm, loaded) = load(
        r#"
struct Sequence { val items: ArrayList<i32> }
impl Index<usize> for Sequence {
    type Output = i32;
    fn index(self, index: usize) -> i32 { self.items[index] }
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
fn make() -> List<i32> { Sequence { items: [3, 1, 2] } }
"#,
    );
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
    let receiver = vm.execute(&loaded, "make").unwrap().return_value;
    let root = vm.runtime().root_value(receiver.clone()).unwrap();
    for _ in 0..3 {
        let reversed = vm
            .invoke_interface_method(&receiver, &methods[0], &[])
            .unwrap();
        let result_root = vm.runtime().root_value(reversed.clone()).unwrap();
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(
            vm.invoke_interface_method(&reversed, &methods[1], &[])
                .unwrap(),
            Value::U64(3)
        );
        drop(result_root);
    }
    drop(root);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    for generation in generations {
        assert_eq!(
            vm.runtime()
                .modules()
                .retention_counts(generation)
                .runtime_values,
            0
        );
    }
}

fn load(source: &str) -> (Vm, LoadedModule) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("interface-cache.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime.load_program("interface-cache", program).unwrap();
    (Vm::new(runtime), loaded)
}
