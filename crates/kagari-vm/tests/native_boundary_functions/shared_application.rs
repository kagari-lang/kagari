//! A portable producer may emit a closed use of a shared native template. Keep
//! this source-free binding path covered independently of source specialization.
use crate::{compile_program, native_boundary_interfaces::generic_identity_module};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget, NativeImportId},
    module::CallableTarget,
};
use kagari_common::identity::DefinitionKind;
use kagari_contract::callable::shared::SharedCall;
use kagari_runtime::{
    Runtime,
    native::{
        conversion::context::ConversionContext, function_handle::PinnedFunction,
        module::NativeModule,
    },
};
use kagari_types::callable::CallableImplementation;
use kagari_vm::vm::Vm;
use std::slice;

#[test]
fn source_free_closed_shared_application_retains_and_releases_its_environment() {
    let module = generic_identity_module();
    let declaration = module
        .to_declaration()
        .unwrap()
        .definition(DefinitionKind::Function, "identity");
    let mut program = compile_program(
        r#"
        use example::generic_helper::identity;
        trait Forward { fn forward<T>(self, value: T) -> T { identity(value) } }
        impl Forward for i32 {}
        pub fn evidence() -> i32 { identity(42) }
        pub fn shared() -> i32 { val forward: Forward = 0; forward.forward(7) }
        pub fn echo(f: fn(i32) -> i32) -> fn(i32) -> i32 { f }
        pub fn invoke(f: fn(i32) -> i32) -> i32 { f(43) }
    "#,
        Some(&module),
    );
    let root_slot = program.root;
    let root = &mut program.modules[root_slot.index()];
    let concrete = root
        .native_imports
        .iter()
        .position(|import| import.instance.declaration == declaration && import.generic.is_none())
        .unwrap();
    let template = root
        .native_imports
        .iter()
        .position(|import| import.instance.declaration == declaration && import.generic.is_some())
        .unwrap();
    let application = root.native_imports[concrete].clone();
    let shared = root.native_imports[template].clone();
    assert!(application.callables.is_empty());
    let contract = SharedCall {
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
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
    let owner = runtime
        .load_program("shared-application", decoded.program)
        .unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute_typed::<(), i32>(&owner, "evidence", ()).unwrap(),
        42
    );
    assert_eq!(
        vm.execute_typed::<(), i32>(&owner, "shared", ()).unwrap(),
        7
    );
    vm.runtime().collect_garbage().unwrap();
    let prepared_environments = vm.runtime().gc().stats().environments;
    assert!(prepared_environments > 0);
    let argument = ConversionContext::new(vm.runtime(), &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(i32,), i32>(
            &owner,
            &declaration,
            slice::from_ref(&argument),
        )
        .unwrap();
    let duplicate = vm
        .runtime()
        .bind_function_application_declaration::<(i32,), i32>(&owner, &declaration, &[argument])
        .unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        vm.runtime().gc().stats().environments,
        prepared_environments
    );
    assert_eq!(vm.call(&identity, (41,)).unwrap(), 41);
    drop(identity);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.call(&duplicate, (42,)).unwrap(), 42);
    let boxed: PinnedFunction<(i32,), i32> = vm
        .execute_typed(&owner, "echo", (duplicate.clone(),))
        .unwrap();
    drop(duplicate);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.call(&boxed, (44,)).unwrap(), 44);
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "invoke", (boxed.clone(),))
            .unwrap(),
        43
    );
    // The linked program retains shared preparation after transient handles go
    // away. Reload retires that owner, while the old callable still pins it.
    let candidate = vm
        .runtime()
        .stage_reload_verified_program(
            &owner,
            "shared-application",
            owner.verified_program().clone(),
        )
        .unwrap();
    vm.runtime().publish_staged_reload(candidate).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.call(&boxed, (45,)).unwrap(), 45);
    drop(boxed);
    let collected = vm.runtime().collect_garbage().unwrap();
    assert_eq!(collected.live_objects, 0);
    assert!(collected.reclaimed_environments > 0);
    assert_eq!(vm.runtime().gc().stats().environments, 0);
}
