//! Analyze native declaration views normally, then attach checked registration metadata.
#[cfg(test)]
mod tests;
use crate::{
    hir::{
        ids::FunctionId,
        item::{function::FunctionKind, module::Import, storage::Visibility},
        ty::TypeKind,
    },
    language::source::{module_source, trait_source},
    lower::{LoweredModule, lower_module_controlled},
    native::{
        NativeBinding, NativeTypeKind,
        render::{DeclarationSource, declaration_source_with_providers},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, mapping::DefinitionRecord},
    span::Span,
};
use kagari_contract::library::namespaces;
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_syntax::{
    ast::{item::Item, traits::AstNode},
    parser::{Parse, ParseLimits, parse_declarations},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeTypeConstructor,
    },
    language::{Protocol, role::LangRole},
};
use std::{collections::HashSet, sync::Arc};

pub(crate) fn import(
    definition: &ModuleDecl,
    providers: &[Arc<ModuleDecl>],
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(Parse, Arc<LoweredModule>), DeclarationError> {
    let generated = declaration_source_with_providers(definition, providers)?;
    import_source(definition, providers, &generated, limits, cancel)
}

pub(crate) fn import_source(
    definition: &ModuleDecl,
    providers: &[Arc<ModuleDecl>],
    generated: &DeclarationSource,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(Parse, Arc<LoweredModule>), DeclarationError> {
    definition.validate(&namespaces::receiver_owner)?;
    let mut sources = SourceDatabase::default();
    sources
        .bind_module(&generated.uri, definition.identity.clone())
        .map_err(DeclarationError)?;
    let id = sources
        .set(&generated.uri, generated.text.clone(), SourceLayer::Base)
        .map_err(DeclarationError)?;
    let mut source = sources
        .snapshot()
        .file(id)
        .expect("native declaration source")
        .clone();
    if namespaces::is_language_module(&definition.identity) {
        let (uri, authored_source) = module_source(&definition.identity.path[0]);
        let original = Arc::new(SourceFile::new(uri, authored_source));
        let view = Arc::make_mut(&mut source);
        for role in LangRole::ALL
            .into_iter()
            .filter(|role| namespaces::trait_owner(role.protocol().name()) == definition.identity)
        {
            let text = trait_source(role);
            if let Some(start) = view.text().find(text) {
                let authored = authored_source
                    .find(text)
                    .expect("handwritten trait fragment");
                view.add_copy(
                    Span::new(start, start + text.len()),
                    original.clone(),
                    Span::new(authored, authored + text.len()),
                )
                .map_err(|error| DeclarationError(error.into()))?;
            }
        }
    }
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
    lowered.language_foundation = namespaces::is_language_module(&definition.identity);
    lowered.native_package_alias = definition.package_alias.clone();
    lowered.registered_declarations = definition.native_declarations();
    attach_types(definition, &mut lowered)?;
    attach_functions(definition, &mut lowered)?;
    attach_dependencies(definition, providers, &mut lowered, cancel)?;
    Ok((parsed, Arc::new(lowered)))
}

/// Exact non-trivia syntax correspondence prevents attaching a Rust entry to a
/// changed declaration. The handwritten core portion is checked by role/shape
/// validation instead. Signatures still pass ordinary name and type analysis.
fn validate_view(
    definition: &ModuleDecl,
    providers: &[Arc<ModuleDecl>],
    parsed: &Parse,
    limits: ParseLimits,
    cancel: &CancellationToken,
) -> Result<(), DeclarationError> {
    let expected = declaration_source_with_providers(definition, providers)?;
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
            .filter(|item| {
                if !namespaces::is_language_module(&definition.identity) {
                    return true;
                }
                let Item::TraitDef(item) = item else {
                    return true;
                };
                let owner = definition
                    .definition(DefinitionKind::Trait, &item.name_text().unwrap_or_default());
                Protocol::from_id(&owner)
                    .and_then(LangRole::from_protocol)
                    .is_none()
            })
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
        let kind = match ty.kind {
            TypeDefKind::NativeStorage(layout) => NativeTypeKind::Storage {
                declaration: definition.definition(DefinitionKind::AssociatedType, &ty.name),
                arity: ty.generic_params.len(),
                layout,
            },
            TypeDefKind::Native(constructor) => match constructor {
                NativeTypeConstructor::Array => NativeTypeKind::Vec,
                NativeTypeConstructor::String => NativeTypeKind::String,
                NativeTypeConstructor::Map => NativeTypeKind::HashMap,
                NativeTypeConstructor::Set => NativeTypeKind::HashSet,
                NativeTypeConstructor::Iter => NativeTypeKind::Iter,
                NativeTypeConstructor::Range(kind) => NativeTypeKind::Range(kind),
                NativeTypeConstructor::Enum(kind) => NativeTypeKind::Enum(kind),
            },
            _ => return Err(DeclarationError("missing native representation".into())),
        };
        if let NativeTypeKind::Enum(_) = kind {
            let item = lowered
                .module
                .enums
                .iter()
                .find(|item| item.name == ty.name)
                .ok_or_else(|| DeclarationError("missing native enum declaration".into()))?;
            lowered.native_enums.insert(item.id, kind);
        } else {
            let item = lowered
                .module
                .opaque_types
                .iter()
                .find(|item| item.name == ty.name)
                .ok_or_else(|| DeclarationError("missing native type declaration".into()))?;
            lowered.native_types.insert(item.id, kind);
        }
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
            member.has_default = matches!(
                method.implementation,
                CallableImplementation::NativeDefault(_)
            );
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
                namespaces::source_module(&provider.identity),
                ty.name
            );
            if !referenced.contains(alias.as_str()) {
                continue;
            }
            let kind = match ty.kind {
                TypeDefKind::Native(NativeTypeConstructor::Enum(_)) => DefinitionKind::Enum,
                _ => DefinitionKind::AssociatedType,
            };
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
    for id in imports {
        let path = format!("{}::{}", id.module, id.path[0].name);
        let alias = format!(
            "{}::{}",
            namespaces::source_module(&id.module),
            id.path[0].name
        );
        lowered.module.imports.push(Import {
            visibility: Visibility::Private,
            alias,
            path,
            span: Span::default(),
            glob: false,
        });
    }
    let mut occupied: HashSet<_> = lowered
        .module
        .imports
        .iter()
        .map(|item| item.alias.clone())
        .chain(lowered.module.exports.iter().map(|item| item.name.clone()))
        .chain(
            lowered
                .module
                .functions
                .iter()
                .map(|item| item.name.clone()),
        )
        .collect();
    for (index, identity) in definition.dependencies.iter().enumerate() {
        let mut alias = format!("__native_dependency_{index}");
        while !occupied.insert(alias.clone()) {
            alias.push('_');
        }
        lowered.module.imports.push(Import {
            visibility: Visibility::Private,
            alias,
            path: format!("{}::{}", identity.package.0, identity.path.join("::")),
            span: Span::default(),
            glob: false,
        });
    }
    Ok(())
}
