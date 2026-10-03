//! Repeated dispatch must not copy metadata for unused interface methods.
mod native_allocations_counter;

use kagari_abi::declaration::ModuleDecl;
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::source_database::{SourceDatabase, SourceLayer};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{Runtime, value::Value};
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
        let method = ModuleDecl::method_id(&contract.declaration, "m0");
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
