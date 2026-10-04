use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget, NativeImportId, Register},
    program::{BytecodeProgram, verify_program},
};
use kagari_compiler::bytecode::lower_program_to_bytecode;
use kagari_contract::{
    callable::{CallableImplementation, witness::OperationWitness},
    scalar::BuiltinType,
    types::Ty,
};
use kagari_runtime::{Runtime, native::foundation, value::Value};
use kagari_vm::vm::Vm;
use std::sync::Arc;
use {
    kagari_common::identity::{ModuleIdentity, PackageId},
    kagari_source::source_database::{SourceDatabase, SourceLayer},
};

fn fixture(dependency_source: &str) -> BytecodeProgram {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        ("dependency", dependency_source),
        (
            "root",
            "use pkg::dependency::answer; fn main() -> i32 { answer() }",
        ),
    ] {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        root = Some(sources.set(&uri, text.into(), SourceLayer::Base).unwrap());
    }
    let mut analysis = kagari_hir::analysis::AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation::modules()
            .unwrap()
            .iter()
            .map(|module| Arc::new(module.to_declaration().unwrap()))
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let ir = kagari_compiler::source::program::lower_program_to_mir(&checked, &Default::default())
        .unwrap();
    lower_program_to_bytecode(&ir).unwrap()
}

#[test]
fn direct_native_imports_run_from_source_and_decoded_artifacts() {
    let program = fixture(
        r#"
        pub fn answer() -> i32 {
            val values: Vec<i32> = Vec::new();
            values.push(40); values.push(2);
            if values.len() == 2usize { values[0] + values[1] } else { 0 }
        }
    "#,
    );
    let imports: Vec<_> = program
        .modules
        .iter()
        .flat_map(|module| &module.native_imports)
        .collect();
    for expected in [
        "$foundation_list_new",
        "$foundation_list_push_fluent",
        "$foundation_list_len",
        "$foundation_list_index",
    ] {
        assert!(
            imports.iter().any(|import| import
                .binding
                .path
                .last()
                .is_some_and(|part| part.name == expected)),
            "missing {expected}"
        );
    }
    assert!(
        imports
            .iter()
            .all(|import| import.host.is_none() && import.structurally_valid())
    );
    let artifact = KbcArtifact::from_program(program.clone(), Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    for program in [program, decoded.program] {
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("native-imports", program).unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(vm.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn forged_native_imports_reject_bindings_signatures_and_obligations() {
    let program = fixture(
        "use std::collections::{HashSet};\npub fn answer() -> i32 { val set: HashSet<i32> = HashSet::new(); set.insert(42); if set.contains(42) { 42 } else { 0 } }",
    );
    let (member, slot) = program
        .modules
        .iter()
        .enumerate()
        .find_map(|(member, module)| {
            module
                .native_imports
                .iter()
                .position(|import| {
                    import
                        .binding
                        .path
                        .last()
                        .is_some_and(|part| part.name == "$foundation_set_contains")
                })
                .map(|slot| (member, slot))
        })
        .unwrap();
    assert!(
        !program.modules[member].native_imports[slot]
            .requirements
            .is_empty()
    );
    assert!(
        !program.modules[member].native_imports[slot]
            .instance
            .arguments
            .is_empty()
    );
    for corrupt in 0..7 {
        let mut forged = program.clone();
        let import = &mut forged.modules[member].native_imports[slot];
        match corrupt {
            0 => import.binding.path.last_mut().unwrap().name = "uninstalled".into(),
            1 => {
                import.signature.params.pop().unwrap();
            }
            2 => import.signature.result = Ty::Builtin(BuiltinType::I32),
            3 => import.requirements.clear(),
            4 => import.instance.arguments.clear(),
            5 => import.instance.declaration.path.last_mut().unwrap().name = "unpublished".into(),
            6 => {
                let instruction = forged.modules[member]
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                    .find(|instruction| {
                        matches!(
                            instruction,
                            BytecodeInstruction::Call {
                                callee: CallTarget::Native(_),
                                ..
                            }
                        )
                    })
                    .unwrap();
                let BytecodeInstruction::Call { callee, .. } = instruction else {
                    unreachable!()
                };
                *callee = CallTarget::Native(NativeImportId::new(usize::MAX));
            }
            _ => unreachable!(),
        }
        assert!(
            verify_program(&forged).is_err(),
            "accepted corruption {corrupt}"
        );
        assert!(
            KbcArtifact::from_program(forged.clone(), Default::default()).is_err(),
            "artifact accepted corruption {corrupt}"
        );
        let mut runtime = Runtime::default();
        assert!(
            runtime.load_program("forged-native", forged).is_err(),
            "runtime accepted corruption {corrupt}"
        );
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}

#[test]
fn concrete_collection_native_signatures_preserve_element_types() {
    let program = fixture(
        r#"pub fn answer() -> i32 {
        val values = [40]; values.push(2);
        values[0] + match values.get(1usize) { Some(value) => value, None => 0 }
    }"#,
    );
    assert!(
        program
            .modules
            .iter()
            .flat_map(|module| &module.native_imports)
            .any(|import| matches!(
                import.signature.params.first(),
                Some(kagari_contract::types::Ty::Array(_, _))
            ))
    );
    let artifact = KbcArtifact::from_program(program.clone(), Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    for program in [program, decoded.program] {
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("native-collections", program).unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(vm.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn hash_storage_native_imports_carry_and_validate_selected_callables() {
    let program = fixture(
        r#"use std::collections::{HashSet};
pub fn answer() -> i32 {
        val values: HashSet<i32> = HashSet::new();
        values.insert(42);
        if values.contains(42) { 42 } else { 0 }
    }"#,
    );
    let (owner, import) = program
        .modules
        .iter()
        .enumerate()
        .find_map(|(index, module)| {
            module
                .native_imports
                .iter()
                .position(|import| !import.callables.is_empty())
                .map(|import| (index, import))
        })
        .unwrap();
    assert!(
        program.modules[owner].native_imports[import]
            .callables
            .len()
            >= 2
    );
    let mut missing = program.clone();
    missing.modules[owner].native_imports[import]
        .callables
        .clear();
    assert!(verify_program(&missing).is_err());
    let mut forged = program.clone();
    let OperationWitness::Selected(selected) =
        &mut forged.modules[owner].native_imports[import].callables[0]
    else {
        panic!("concrete callable");
    };
    selected.implementation = CallableImplementation::Required;
    assert!(verify_program(&forged).is_err());
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_program("native-witnesses", decoded.program)
        .unwrap();
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn native_storage_writes_reject_element_type_forgery() {
    let mut program = fixture(
        r#"pub fn answer() -> i32 {
        val values = [[42]];
        val wrong = ["bad"];
        wrong.push("still bad");
        values.push([1]);
        values[0][0]
    }"#,
    );
    let member = program
        .modules
        .iter()
        .position(|module| {
            module.native_imports.iter().any(|import| {
                import
                    .binding
                    .path
                    .last()
                    .is_some_and(|part| part.name == "$foundation_list_push_fluent")
            })
        })
        .unwrap();
    let imports = program.modules[member].native_imports.clone();
    let function = program.modules[member]
        .functions
        .iter_mut()
        .find(|function| function.name == "answer")
        .unwrap();
    let wrong = function
        .metadata
        .semantic
        .registers
        .iter()
        .find_map(|(index, ty)| {
            matches!(ty,Ty::Array(item,_) if item.as_ref() == &Ty::Builtin(BuiltinType::String))
                .then_some(Register::new(*index))
        })
        .unwrap();
    let instruction = function.instructions.iter_mut().find(|instruction| matches!(instruction, BytecodeInstruction::Call {callee: CallTarget::Native(id), ..} if imports[id.index()].binding.path.last().is_some_and(|part| part.name == "$foundation_list_push_fluent") && matches!(&imports[id.index()].signature.params[0], Ty::Array(item, _) if matches!(item.as_ref(), Ty::Array(_, _))))).unwrap();
    let BytecodeInstruction::Call { args, .. } = instruction else {
        unreachable!()
    };
    // The values share a physical heap-handle representation; the concrete
    // element contract must reject substituting Vec<String> for Vec<i32>.
    args[1] = wrong;
    assert!(verify_program(&program).is_err());
    assert!(KbcArtifact::from_program(program.clone(), Default::default()).is_err());
    let mut runtime = Runtime::default();
    assert!(runtime.load_program("forged-storage", program).is_err());
    assert_eq!(runtime.gc().active_roots(), 0);
}
