//! Source-free free-function fixtures use common declarations and the ordinary frame driver.
use kagari_bytecode::{
    instruction::NativeImportId,
    module::{BytecodeModule, CallableTarget},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    metadata::DefinitionMetadata, scoped::DefinitionReferences, table::DefinitionTable,
};
use kagari_contract::{
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_runtime::{Runtime, module::LoadedModule, native::binding::NativeResult, value::Value};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::module::ModuleDecl,
};

/// Carry every free-function contract, including unused entries, without source analysis.
/// These fixtures deliberately have no generic proof, nominal layout or trait impl input.
pub fn free_function_program(
    declarations: &DefinitionMetadata<ModuleDecl>,
) -> DefinitionMetadata<BytecodeProgram> {
    let module = declarations.records();
    assert!(module.types.is_empty());
    assert!(module.traits.is_empty());
    assert!(module.implementations.is_empty());
    assert!(module.paths.is_empty());
    let native_declarations = module
        .native_declarations(declarations.definitions())
        .unwrap();
    let native_imports = native_declarations
        .iter()
        .map(|declaration| {
            let function = &declaration.function;
            assert!(function.generic_params.is_empty() && function.bounds.is_empty());
            assert!(declaration.callable_requirements.is_empty());
            let CallableImplementation::Native(binding) = function.implementation else {
                panic!("fixture requires an ordinary native entry")
            };
            NativeImport {
                receiver_access: function.method_policy.receiver_access,
                result_adapter: None,
                generic: None,
                instance: ConcreteFunctionIdentity {
                    declaration: declaration.declaration,
                    arguments: vec![],
                },
                binding,
                signature: Signature {
                    params: function
                        .params
                        .iter()
                        .map(|parameter| parameter.ty.clone())
                        .collect(),
                    result: function.return_type.clone(),
                },
                requirements: vec![],
                callables: vec![],
                host: None,
            }
        })
        .collect();
    let program = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            identity: module.identity.clone(),
            source_name: "native-contract-fixture".into(),
            public_items: native_declarations
                .iter()
                .filter(|declaration| !module.private_functions.contains(&declaration.declaration))
                .map(|declaration| PublicItem::Function(declaration.function.clone()))
                .collect(),
            native_declarations,
            native_imports,
            ..Default::default()
        }],
    };
    scoped(declarations.definitions(), program)
}

pub fn scoped<T>(definitions: &DefinitionTable, records: T) -> DefinitionMetadata<T>
where
    T: DefinitionReferences,
{
    DefinitionMetadata::checked(definitions.clone(), records, &Default::default()).unwrap()
}

/// Enter a linked native callable with frame roots, session checks and ordinary cleanup.
pub fn invoke(
    runtime: &Runtime,
    owner: &LoadedModule,
    slot: usize,
    args: &[Value],
) -> NativeResult<Value> {
    let stack = runtime.enter_execution_stack(owner)?;
    stack.push_callable(
        runtime,
        owner.clone(),
        CallableTarget::Native(NativeImportId::new(slot)),
        args,
        None,
    )?;
    stack.start_native_entry(runtime, |_, _, _, _| {
        unreachable!("fixture does not call script")
    })?;
    Ok(stack
        .finish_native_return(runtime)?
        .expect("root native return"))
}
