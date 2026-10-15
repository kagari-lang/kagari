//! Project print's checked service requirement into the ordinary native module graph.
use crate::source::lower::MirLoweringError;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, map::DefinitionContext, mapping::DefinitionMappingError},
};
use kagari_contract::{
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, ModuleContract},
};
use kagari_mir::function::MirModule;
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{NativeDeclaration, logging},
};

fn declaration(
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<NativeDeclaration, DefinitionMappingError> {
    let metadata = logging::declaration()?.import_into(context, cancel)?;
    metadata
        .records()
        .native_declarations(metadata.definitions())
        .map_err(|_| DefinitionMappingError::InvalidContract)?
        .into_iter()
        .next()
        .ok_or(DefinitionMappingError::InvalidContract)
}

pub(super) fn import(
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<NativeImport, DefinitionMappingError> {
    let declaration = declaration(context, cancel)?;
    let CallableImplementation::Native(binding) = declaration.function.implementation else {
        return Err(DefinitionMappingError::InvalidContract);
    };
    Ok(NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: None,
        instance: ConcreteFunctionIdentity {
            declaration: declaration.declaration,
            arguments: Vec::new(),
        },
        binding,
        signature: Signature {
            params: declaration
                .function
                .params
                .into_iter()
                .map(|parameter| parameter.ty)
                .collect(),
            result: declaration.function.return_type,
        },
        requirements: Vec::new(),
        callables: Vec::new(),
    })
}

pub(super) fn attach(
    modules: &mut Vec<MirModule>,
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<(), MirLoweringError> {
    let contract = logging::declaration()?;
    let definitions = context.snapshot();
    let Some(id) = definitions
        .lookup_root(&contract.records().identity)
        .and_then(|root| {
            definitions.lookup_child(
                root,
                DefinitionKind::Function,
                &contract.records().functions[0].name,
                0,
            )
        })
    else {
        return Ok(());
    };
    if !modules.iter().any(|module| {
        module
            .native_applications()
            .any(|import| import.instance.declaration == id)
    }) {
        return Ok(());
    }
    let target = import(context, cancel)?;
    let declaration = declaration(context, cancel)?;
    let owner = context
        .snapshot()
        .resolve(declaration.declaration)
        .map_err(|error| MirLoweringError::Identity(error.into()))?
        .module()
        .clone();
    let conflict = || MirLoweringError::MissingBinding("print's native provider contract");
    if let Some(provider) = modules.iter_mut().find(|module| module.identity == owner) {
        if provider
            .functions
            .iter()
            .any(|function| function.instance.declaration == declaration.declaration)
        {
            return Err(conflict());
        }
        if let Some(actual) = provider
            .abi
            .native_declarations
            .iter()
            .find(|actual| actual.declaration == declaration.declaration)
        {
            if actual != &declaration {
                return Err(conflict());
            }
        } else {
            provider.abi.native_declarations.push(declaration);
        }
        if let Some(actual) = provider
            .native_targets
            .iter()
            .find(|actual| actual.instance == target.instance)
        {
            if actual != &target {
                return Err(conflict());
            }
        } else {
            provider.native_targets.push(target);
        }
    } else {
        modules.push(MirModule {
            identity: owner,
            source_name: "builtin://host/log".into(),
            abi: ModuleContract {
                native_declarations: vec![declaration],
                ..Default::default()
            },
            native_targets: vec![target],
            interface_instances: Vec::new(),
            external_types: Vec::new(),
            dependencies: Vec::new(),
            structures: Vec::new(),
            enumerations: Vec::new(),
            module_slots: Default::default(),
            functions: Default::default(),
        });
    }
    Ok(())
}
