//! Source-free common free-function contracts, shared with VM boundary fixtures.
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata};
use kagari_contract::{
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{TypeDeclaration, TypeDefKind, module::ModuleDecl},
};

/// Carry every free-function contract, including unused entries, without source analysis.
/// External schemas retain their nominal owners. Generic proofs, GC layouts and trait impls
/// remain at their dedicated fixtures.
pub fn free_function_program(
    declarations: &DefinitionMetadata<ModuleDecl>,
) -> DefinitionMetadata<BytecodeProgram> {
    let module = declarations.records();
    assert!(
        module
            .types
            .iter()
            .all(|ty| matches!(ty.kind, TypeDefKind::Host(_)))
    );
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
    let external_types = module
        .types
        .iter()
        .map(|ty| TypeDeclaration {
            declaration: module
                .definition(DefinitionKind::Struct, &ty.name, declarations.definitions())
                .unwrap(),
            definition: ty.clone(),
        })
        .collect();
    let program = BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            identity: module.identity.clone(),
            source_name: "native-contract-fixture".into(),
            external_types,
            public_items: native_declarations
                .iter()
                .filter(|declaration| !module.private_functions.contains(&declaration.declaration))
                .map(|declaration| PublicItem::Function(declaration.function.clone()))
                .chain(module.types.iter().cloned().map(PublicItem::Type))
                .collect(),
            native_declarations,
            native_imports,
            ..Default::default()
        }],
    };
    DefinitionMetadata::checked(
        declarations.definitions().clone(),
        program,
        &Default::default(),
    )
    .unwrap()
}
