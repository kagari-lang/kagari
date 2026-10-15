//! Analyze native declaration views normally, then attach checked registration metadata.
mod access;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod host_tests;
#[cfg(test)]
mod reference_tests;
use crate::{
    hir::{ids::FunctionId, item::function::FunctionKind, ty::TypeKind},
    lower::{LoweredModule, lower_module_controlled},
    native::{
        NativeBinding, NativeTypeKind,
        paths::module_path,
        render::{DeclarationSource, declaration_source},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        metadata::DefinitionMetadata,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_syntax::{
    ast::traits::AstNode,
    parser::{Parse, ParseLimits, parse_declarations},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, TypeDeclaration, TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeTypeConstructor,
        ownership::ReceiverOwners,
    },
    language,
};
use std::{collections::HashSet, sync::Arc};

/// Renders and imports a registered module through ordinary declaration parsing and lowering.
/// The provider batch must already share the analysis scope produced by Native adoption.
pub(crate) fn import(
    metadata: &DefinitionMetadata<ModuleDecl>,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(Parse, Arc<LoweredModule>), DeclarationError> {
    let generated = declaration_source(metadata, providers)?;
    import_source(metadata, providers, &generated, limits, cancel)
}

/// Validates supplied declaration text against registration, then attaches storage/native/role metadata to ordinary lowering.
/// Providers use one adopted scope; generated navigation metadata is not registration authority.
pub(crate) fn import_source(
    metadata: &DefinitionMetadata<ModuleDecl>,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
    generated: &DeclarationSource,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(Parse, Arc<LoweredModule>), DeclarationError> {
    let definition = metadata.records();
    let definitions = metadata.definitions();
    let owners = ReceiverOwners::from_types(
        providers
            .iter()
            .map(|module| module.records())
            .flat_map(|module| module.types.iter().map(|ty| (&module.identity, ty))),
    )?;
    definition.validate(&|receiver| owners.owner(receiver, definitions), definitions)?;
    let mut sources = SourceDatabase::default();
    sources
        .bind_module(&generated.uri, definition.identity.clone())
        .map_err(DeclarationError)?;
    let id = sources
        .set(&generated.uri, generated.text.clone(), SourceLayer::Base)
        .map_err(DeclarationError)?;
    let source = sources
        .snapshot()
        .file(id)
        .expect("native declaration source")
        .clone();
    let parsed = parse_declarations(&source, limits, cancel)
        .map_err(|_| DeclarationError("native declaration analysis cancelled".into()))?;
    if !parsed.diagnostics().is_empty() {
        return Err(DeclarationError(format!(
            "invalid native declaration syntax: {:?}",
            parsed.diagnostics().first()
        )));
    }
    validate_view(metadata, providers, &parsed, limits, cancel)?;
    let mut lowered = lower_module_controlled(source, &parsed.syntax(), cancel);
    lowered.registered_native_api = true;
    lowered.native_definitions = Some(definitions.clone());
    lowered.language_foundation = language::is_language_module(&definition.identity);
    lowered.native_package_alias = definition.package_alias.clone();
    lowered.native_prelude = definition.prelude;
    // The renderer emits authored aliases first, in this same ordered map traversal.
    // Exact syntax correspondence above binds these trusted categories to the leaves.
    lowered.native_import_namespaces = definition
        .exports
        .keys()
        .enumerate()
        .map(|(slot, name)| (slot, name.namespace))
        .collect();
    lowered.registered_traits = definition
        .traits
        .iter()
        .map(|contract| {
            Ok((
                definition.definition(DefinitionKind::Trait, &contract.name, definitions)?,
                contract.clone(),
            ))
        })
        .collect::<Result<_, DeclarationError>>()?;
    lowered.registered_declarations = definition.native_declarations(definitions)?;
    attach_types(definition, definitions, providers, &mut lowered)?;
    attach_functions(definition, &mut lowered)?;
    access::attach(definition, definitions, &mut lowered)?;
    attach_dependencies(metadata, providers, &mut lowered, cancel)?;
    Ok((parsed, Arc::new(lowered)))
}

/// Exact non-trivia syntax correspondence prevents attaching a Rust entry to a
/// changed declaration. Role attributes belong to the parsed owning declarations;
/// signatures still pass ordinary name and type analysis.
fn validate_view(
    metadata: &DefinitionMetadata<ModuleDecl>,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
    parsed: &Parse,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(), DeclarationError> {
    let expected = declaration_source(metadata, providers)?;
    let expected = parse_declarations(
        &SourceFile::new(&expected.uri, &expected.text),
        limits,
        cancel,
    )
    .map_err(|_| DeclarationError("native declaration correspondence cancelled".into()))?;
    let tokens = |parsed: &Parse| {
        parsed
            .syntax()
            .items()
            .map(|item| {
                item.syntax()
                    .descendants_with_tokens()
                    .filter_map(|element| element.into_token())
                    .filter(|token| !token.kind().is_trivia())
                    .map(|token| (token.kind(), token.text().to_string()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    if tokens(parsed) != tokens(&expected) {
        return Err(DeclarationError(
            "native declaration source differs from authoritative registration".into(),
        ));
    }
    Ok(())
}

fn attach_types(
    definition: &ModuleDecl,
    definitions: &DefinitionTable,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
    lowered: &mut LoweredModule,
) -> Result<(), DeclarationError> {
    let schemas = providers
        .iter()
        .flat_map(|provider| {
            provider
                .records()
                .types
                .iter()
                .filter(|ty| matches!(ty.kind, TypeDefKind::Host(_)))
                .map(|ty| {
                    Ok(TypeDeclaration {
                        declaration: provider.records().definition(
                            DefinitionKind::Struct,
                            &ty.name,
                            definitions,
                        )?,
                        definition: ty.clone(),
                    })
                })
        })
        .collect::<Result<Vec<_>, DeclarationError>>()?;
    let schemas: Arc<[TypeDeclaration]> = schemas.into();
    for ty in &definition.types {
        if ty.kind == TypeDefKind::Struct {
            if !lowered
                .module
                .structs
                .iter()
                .any(|item| item.name == ty.name)
            {
                return Err(DeclarationError(
                    "missing registered struct declaration".into(),
                ));
            }
            // Exact source correspondence leaves ordinary struct typing/layout intact.
            continue;
        }
        if ty.kind == TypeDefKind::Enum {
            let item = lowered
                .module
                .enums
                .iter()
                .find(|item| item.name == ty.name)
                .ok_or_else(|| DeclarationError("missing registered enum declaration".into()))?;
            for variant in &ty.variants {
                if variant.reports_failure {
                    let member = item
                        .variants
                        .iter()
                        .find(|member| member.name == variant.name)
                        .ok_or_else(|| {
                            DeclarationError("missing registered enum variant".into())
                        })?;
                    lowered.registered_enum_failures.insert(member.id);
                }
            }
            // Ordinary registered enums use the parsed nominal enum representation.
            continue;
        }
        let kind = match ty.kind {
            TypeDefKind::Host(_) => NativeTypeKind::Host {
                definition: Arc::new(ty.clone()),
                declaration: definition.definition(
                    DefinitionKind::Struct,
                    &ty.name,
                    definitions,
                )?,
                paths: definition
                    .paths
                    .iter()
                    .filter(|path| {
                        definitions
                            .resolve(path.root)
                            .ok()
                            .and_then(|view| view.last())
                            .is_some_and(|leaf| leaf.name == ty.name)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
                    .into(),
                schemas: schemas.clone(),
            },
            TypeDefKind::NativeStorage(layout) => NativeTypeKind::Storage {
                declaration: definition.definition(
                    DefinitionKind::AssociatedType,
                    &ty.name,
                    definitions,
                )?,
                arity: ty.generic_params.len(),
                layout,
            },
            TypeDefKind::Native(constructor) => match constructor {
                NativeTypeConstructor::Reference(access) => NativeTypeKind::Reference(access),
                NativeTypeConstructor::String => NativeTypeKind::String,
                NativeTypeConstructor::Map => NativeTypeKind::HashMap,
                NativeTypeConstructor::Set => NativeTypeKind::HashSet,
                NativeTypeConstructor::Iter => NativeTypeKind::Iter,
                NativeTypeConstructor::Range(kind) => NativeTypeKind::Range(kind),
            },
            _ => return Err(DeclarationError("missing native representation".into())),
        };
        let item = lowered
            .module
            .opaque_types
            .iter()
            .find(|item| item.name == ty.name)
            .ok_or_else(|| DeclarationError("missing native type declaration".into()))?;
        lowered.native_types.insert(item.id, kind);
    }
    Ok(())
}

fn attach_functions(
    definition: &ModuleDecl,
    lowered: &mut LoweredModule,
) -> Result<(), DeclarationError> {
    let mut bindings = vec![];
    for trait_ in &definition.traits {
        let item = lowered
            .module
            .traits
            .iter_mut()
            .find(|item| item.name == trait_.name)
            .ok_or_else(|| DeclarationError("missing native trait declaration".into()))?;
        if let Some(adapter) = &trait_.conversion_adapter {
            lowered
                .native_trait_adapters
                .insert(item.id, adapter.clone());
        }
        for method in &trait_.methods {
            let member = item
                .methods
                .iter_mut()
                .find(|item| item.name == method.name)
                .ok_or_else(|| DeclarationError("missing native trait member".into()))?;
            let registered_default = matches!(
                method.implementation,
                CallableImplementation::NativeDefault(_)
            );
            if member.has_default != registered_default {
                return Err(DeclarationError(
                    "native trait default body differs from registration".into(),
                ));
            }
            bindings.push((member.function, method));
        }
    }
    if lowered.module.impls.len() != definition.implementations.len() {
        return Err(DeclarationError(
            "native implementation inventory differs".into(),
        ));
    }
    for (item, implementation) in lowered.module.impls.iter().zip(&definition.implementations) {
        for method in &implementation.methods {
            let member = item
                .methods
                .iter()
                .find(|item| item.name == method.name)
                .ok_or_else(|| DeclarationError("missing native implementation member".into()))?;
            bindings.push((member.function, method));
        }
    }
    for function in &definition.functions {
        let item = lowered
            .module
            .functions
            .iter()
            .find(|item| item.kind == FunctionKind::User && item.name == function.name)
            .ok_or_else(|| DeclarationError("missing native function declaration".into()))?;
        bindings.push((item.id, function));
    }
    for (id, function) in bindings {
        attach_binding(lowered, id, function);
    }
    Ok(())
}

fn attach_binding(lowered: &mut LoweredModule, id: FunctionId, function: &FnDecl) {
    match &function.implementation {
        CallableImplementation::Native(binding) => {
            lowered
                .native_functions
                .insert(id, NativeBinding::Entry(*binding));
        }
        CallableImplementation::NativeDefault(application) => {
            lowered
                .native_functions
                .insert(id, NativeBinding::Default(application.clone()));
        }
        _ => {}
    }
    lowered.method_policies.insert(id, function.method_policy);
}

fn attach_dependencies(
    metadata: &DefinitionMetadata<ModuleDecl>,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
    lowered: &mut LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), DeclarationError> {
    let definition = metadata.records();
    let definitions = metadata.definitions();
    let mut imports = HashSet::new();
    let referenced: HashSet<_> = lowered
        .module
        .body
        .types
        .iter()
        .filter_map(|(_, ty)| match &ty.kind {
            TypeKind::Named(name) | TypeKind::Generic { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    for provider in providers
        .iter()
        .map(|provider| provider.records())
        .filter(|provider| provider.identity != definition.identity)
    {
        for ty in &provider.types {
            let alias = format!(
                "{}::{}",
                module_path(
                    &provider.identity,
                    providers.iter().map(|provider| provider.records())
                ),
                ty.name
            );
            if !referenced.contains(alias.as_str()) {
                continue;
            }
            let kind = ty.kind.definition_kind();
            let id = provider.definition(kind, &ty.name, definitions)?;
            imports.insert(
                definitions
                    .resolve(id)
                    .map_err(|error| DeclarationError(error.to_string()))?
                    .module()
                    .clone(),
            );
        }
    }
    definition
        .visit_definition_ids(
            &mut |id: &DefinitionId| {
                let view = definitions.resolve(*id)?;
                if view.module() != &definition.identity
                    && view.segments().count() == 1
                    && view.last().is_some_and(|part| {
                        matches!(
                            part.kind,
                            DefinitionKind::Trait
                                | DefinitionKind::AssociatedType
                                | DefinitionKind::Enum
                                | DefinitionKind::Struct
                        )
                    })
                {
                    imports.insert(view.module().clone());
                }
                Ok(())
            },
            cancel,
        )
        .map_err(|error| DeclarationError(error.to_string()))?;
    lowered.native_dependencies = definition
        .dependencies
        .iter()
        .cloned()
        .chain(imports)
        .collect();
    Ok(())
}
