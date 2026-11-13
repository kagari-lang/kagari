//! A portable producer may emit a closed use of a shared native template. Keep
//! this source-free binding path covered independently of source specialization.
use crate::tests::execution::native_boundary::{
    compile_program,
    native_boundary_interfaces::generic_identity_module,
    support::{declaration_query, round_trip_program},
};
use kagari_runtime::{
    Runtime,
    native::{
        conversion::context::ConversionContext, function_handle::Function, module::NativeModule,
    },
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, NativeImportId},
    module::CallableTarget,
    program::updates::{self, EvidenceInputs},
};
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata};
use kagari_contract::callable::shared::SharedCall;
use kagari_types::callable::CallableImplementation;
use std::slice;

#[test]
fn source_free_closed_shared_application_retains_and_releases_its_environment() {
    let module = generic_identity_module();
    let declaration = declaration_query(&module, DefinitionKind::Function, "identity");
    let program = compile_program(
        include_str!("../../../../../tests/fixtures/native/function-shared-application.kgr"),
        Some(&module),
    );
    let (definitions, mut program) = program.into_parts();
    let scoped = definitions.lookup(&declaration).unwrap();
    let root_slot = program.root;
    let root = &mut program.modules[root_slot.index()];
    let concrete = root
        .native_imports
        .iter()
        .position(|import| import.instance.declaration == scoped && import.generic.is_none())
        .unwrap();
    let template = root
        .native_imports
        .iter()
        .position(|import| import.instance.declaration == scoped && import.generic.is_some())
        .unwrap();
    let application = root.native_imports[concrete].clone();
    let shared = root.native_imports[template].clone();
    assert!(application.callables.is_empty());
    let contract = SharedCall {
        constructions: vec![],
        receiver_access: shared.receiver_access,
        instance: shared.instance,
        implementation: CallableImplementation::Native(shared.binding),
        arguments: application.instance.arguments,
        signature: application.signature,
        operations: vec![],
    };
    // Remove the specialized import entirely so host binding must consume the
    // checked shared call. Retarget every root-local native operand after removal.
    root.native_imports.remove(concrete);
    let adjust = |index: usize| NativeImportId::new(index - usize::from(index > concrete));
    for function in &mut root.functions {
        for instruction in &mut function.instructions {
            let BytecodeInstruction::Call { callee, .. } = instruction else {
                continue;
            };
            match callee {
                CallTarget::Native(index) if index.index() == concrete => {
                    *callee = CallTarget::Shared {
                        module: root_slot,
                        target: CallableTarget::Native(adjust(template)),
                        contract: Box::new(contract.clone()),
                    };
                }
                CallTarget::Native(index) => *index = adjust(index.index()),
                CallTarget::Shared {
                    module,
                    target: CallableTarget::Native(index),
                    ..
                } if *module == root_slot => {
                    assert_ne!(index.index(), concrete);
                    *index = adjust(index.index());
                }
                _ => {}
            }
        }
    }
    assert!(
        root.interface_tables
            .iter()
            .flat_map(|table| &table.methods)
            .all(|method| matches!(method.target, CallableTarget::Script(_)))
    );
    // This legal producer rewrite changes the emitted call contract. Preserve
    // source/initializer inputs, but rebuild body evidence from the new bytecode.
    program.update_evidence = updates::build(
        &program.modules,
        EvidenceInputs {
            sources: program.update_evidence.sources.clone(),
            initializers: program.update_evidence.initializers.clone(),
            selections: program.update_evidence.selections.clone(),
            classifications: program
                .update_evidence
                .bodies
                .iter()
                .map(|body| (body.requirement.origin.clone(), body.classification.clone()))
                .collect(),
        },
        &definitions,
        &Default::default(),
    )
    .unwrap();
    let decoded = round_trip_program(
        DefinitionMetadata::checked(definitions, program, &Default::default()).unwrap(),
    );
    let mut runtime = Runtime::default();
    NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
    let owner = load_bytecode(&mut runtime, "shared-application", decoded).unwrap();
    let _owner_program = owner;
    let owner = runtime.select_program(&_owner_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_typed::<(), i32>(&_owner_program, "evidence", (), &Default::default())
            .unwrap(),
        42
    );
    assert_eq!(
        vm.execute_typed::<(), i32>(&_owner_program, "shared", (), &Default::default())
            .unwrap(),
        7
    );
    vm.collect_garbage().unwrap();
    let prepared = vm.gc_stats();
    // This closed scalar application is edge-free. Its checked facts do not
    // require a managed environment or a persistent Host root after invocation.
    assert_eq!(prepared.objects, baseline.objects);
    assert_eq!(prepared.provider_units, baseline.provider_units);
    assert_eq!(prepared.root_registrations, baseline.root_registrations);
    let argument = ConversionContext::new(&mut vm, &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let identity = vm
        .bind_function_application_declaration::<(i32,), i32>(
            &_owner_program.root(),
            &declaration,
            slice::from_ref(&argument),
        )
        .unwrap();
    let duplicate = vm
        .bind_function_application_declaration::<(i32,), i32>(
            &_owner_program.root(),
            &declaration,
            &[argument],
        )
        .unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, prepared.objects);
    assert_eq!(vm.gc_stats().provider_units, prepared.provider_units);
    assert_eq!(
        vm.gc_stats().root_registrations,
        prepared.root_registrations
    );
    assert_eq!(vm.call(&identity, (41,)).unwrap(), 41);
    drop(identity);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.call(&duplicate, (42,)).unwrap(), 42);
    let boxed: Function<(i32,), i32> = vm
        .execute_typed(
            &_owner_program,
            "echo",
            (duplicate.clone(),),
            &Default::default(),
        )
        .unwrap();
    drop(duplicate);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.call(&boxed, (44,)).unwrap(), 44);
    assert_eq!(
        vm.execute_typed::<_, i32>(
            &_owner_program,
            "invoke",
            (boxed.clone(),),
            &Default::default()
        )
        .unwrap(),
        43
    );
    // The linked program caches shared preparation; the named callable retains
    // its lexical environment while ordinary invocation selects current code.
    let candidate = vm
        .stage_reload_verified_program(
            &_owner_program,
            Some("shared-application".into()),
            owner.verified_program().clone(),
        )
        .unwrap();
    vm.publish_staged_reload(candidate).unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.call(&boxed, (45,)).unwrap(), 45);
    drop(boxed);
    vm.collect_garbage().unwrap();
    // The named callable's actual retained owner ends with its last alias.
    // Scalar preparation itself must not keep the obsolete revision alive.
    assert!(vm.validate_executable_module(&owner).is_err());
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    let current = vm.select_program(&_owner_program).unwrap();
    let candidate = vm
        .stage_reload_verified_program(&_owner_program, None, current.verified_program().clone())
        .unwrap();
    vm.publish_staged_reload(candidate).unwrap();
    vm.collect_garbage().unwrap();
    assert!(vm.validate_executable_module(&current).is_err());
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(vm.gc_stats().provider_units, baseline.provider_units);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
