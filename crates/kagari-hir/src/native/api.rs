//! Analyze native declaration views normally, then attach checked registration metadata.
#[cfg(test)]
mod tests;
use crate::{
    hir::{ids::FunctionId, item::function::FunctionKind, ty::TypeKind},
    lower::{LoweredModule, lower_module_controlled},
    native::{
        NativeBinding, NativeTypeKind,
        paths::{array_interfaces, module_path},
        render::{DeclarationSource, declaration_source},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, mapping::DefinitionRecord},
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
        FnDecl, TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeTypeConstructor,
        ownership::ReceiverOwners,
    },
    language,
};
use std::{collections::HashSet, sync::Arc};

/// Renders and imports a registered module through ordinary declaration parsing and lowering.
pub(crate) fn import(
    definition: &ModuleDecl,
    providers: &[Arc<ModuleDecl>],
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(Parse, Arc<LoweredModule>), DeclarationError> {
    let generated = declaration_source(definition, providers)?;
    import_source(definition, providers, &generated, limits, cancel)
}

/// Validates supplied declaration text against registration, then attaches storage/native/role metadata to ordinary lowering.
pub(crate) fn import_source(
    definition: &ModuleDecl,
    providers: &[Arc<ModuleDecl>],
    generated: &DeclarationSource,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(Parse, Arc<LoweredModule>), DeclarationError> {
    let owners = ReceiverOwners::from_types(
        providers
            .iter()
            .flat_map(|module| module.types.iter().map(|ty| (&module.identity, ty))),
    )?;
    definition.validate(&|receiver| owners.owner(receiver))?;
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
    validate_view(definition, providers, &parsed, limits, cancel)?;
    let mut lowered = lower_module_controlled(source, &parsed.syntax(), cancel);
    lowered.registered_native_api = true;
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
    lowered.native_array_interfaces = array_interfaces(providers)?;
    lowered.registered_traits = definition
        .traits
        .iter()
        .map(|contract| {
            (
                definition.definition(DefinitionKind::Trait, &contract.name),
                contract.clone(),
            )
        })
        .collect();
    lowered.registered_declarations = definition.native_declarations();
    attach_types(definition, &mut lowered)?;
    attach_functions(definition, &mut lowered)?;
    attach_dependencies(definition, providers, &mut lowered, cancel)?;
    Ok((parsed, Arc::new(lowered)))
}

/// Exact non-trivia syntax correspondence prevents attaching a Rust entry to a
/// changed declaration. Role attributes belong to the parsed owning declarations;
/// signatures still pass ordinary name and type analysis.
fn validate_view(
    definition: &ModuleDecl,
    providers: &[Arc<ModuleDecl>],
    parsed: &Parse,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(), DeclarationError> {
    let expected = declaration_source(definition, providers)?;
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
    lowered: &mut LoweredModule,
) -> Result<(), DeclarationError> {
    for ty in &definition.types {
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
            TypeDefKind::NativeStorage(layout) => NativeTypeKind::Storage {
                declaration: definition.definition(DefinitionKind::AssociatedType, &ty.name),
                arity: ty.generic_params.len(),
                layout,
            },
            TypeDefKind::Native(constructor) => match constructor {
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
        if let Some(access) = trait_.storage_access {
            lowered.native_trait_access.insert(item.id, access);
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
                .insert(id, NativeBinding::Entry(binding.clone()));
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
    definition: &ModuleDecl,
    providers: &[Arc<ModuleDecl>],
    lowered: &mut LoweredModule,
    cancel: &CancellationToken,
) -> Result<(), DeclarationError> {
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
        .filter(|provider| provider.identity != definition.identity)
    {
        for ty in &provider.types {
            let alias = format!(
                "{}::{}",
                module_path(&provider.identity, providers),
                ty.name
            );
            if !referenced.contains(alias.as_str()) {
                continue;
            }
            let kind = ty.kind.definition_kind();
            imports.insert(provider.definition(kind, &ty.name));
        }
    }
    definition
        .visit_definitions(
            &mut |id: &DefinitionPath| {
                if id.module != definition.identity
                    && id.path.len() == 1
                    && matches!(
                        id.path[0].kind,
                        DefinitionKind::Trait
                            | DefinitionKind::AssociatedType
                            | DefinitionKind::Enum
                            | DefinitionKind::Struct
                    )
                {
                    imports.insert(id.clone());
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
        .chain(imports.into_iter().map(|id| id.module))
        .collect();
    Ok(())
}
