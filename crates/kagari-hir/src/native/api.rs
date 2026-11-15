//! Lower owned registration records directly; retain complete supplied tool source when present.
mod correspondence;
pub(crate) mod preparation;

use crate::{
    hir::{
        ids::FunctionId,
        item::{
            Item,
            function::{FunctionImplementation, NativeContract},
        },
        ty::TypeKind,
    },
    lower::{LoweredModule, lower_module_controlled},
    native::{
        NativeBinding, NativeTypeKind,
        api::preparation::PreparedRegistration,
        paths::module_path,
        render::DeclarationSource,
        structured::{self, RegistrationMap},
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
use kagari_syntax::{
    ast::{views::ItemView, visit::NodeRef},
    kind::SyntaxKind,
    lexer::lex_with_cancellation,
    parser::Parse,
    token::TokenKind,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, TypeDeclaration, TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeTypeConstructor,
    },
    language,
};
use std::{collections::HashSet, sync::Arc};

/// Attaches trusted registration facts to a fresh lowering, never to a copied ID map.
fn attach(
    prepared: &PreparedRegistration,
    cancel: &CancellationToken,
) -> Result<LoweredModule, DeclarationError> {
    let metadata = &prepared.metadata;
    let providers = &prepared.providers;
    let definition = metadata.records();
    let definitions = metadata.definitions();
    let (mut lowered, correspondence) = if let Some(parsed) = prepared.parsed() {
        debug_assert!(prepared.source_surface().is_some());
        let lowered = lower_module_controlled(prepared.source.clone(), &parsed.module(), cancel);
        let correspondence = correspondence::supplied(definition, definitions, &lowered)?;
        (lowered, correspondence)
    } else {
        structured::lower(metadata, providers, prepared.source.clone(), cancel)?
    };
    lowered.registered_native_api = true;
    lowered.native_definitions = Some(definitions.clone());
    lowered.language_foundation = language::is_language_module(&definition.identity);
    lowered.native_package_alias = definition.package_alias.clone();
    lowered.native_prelude = definition.prelude;
    lowered.native_trait_references = definition.trait_references.clone();
    // Both constructors allocate authored aliases first in this ordered traversal.
    // Namespace categories belong to registration, not to an inferred source spelling.
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
    attach_types(
        definition,
        definitions,
        providers,
        &correspondence,
        &mut lowered,
    )?;
    attach_functions(definition, definitions, &correspondence, &mut lowered)?;
    attach_dependencies(metadata, providers, &mut lowered, cancel)?;
    Ok(lowered)
}

/// Exact non-trivia syntax correspondence prevents attaching a Rust entry to a
/// changed declaration. Role attributes belong to the parsed owning declarations;
/// signatures still pass ordinary name and type analysis.
fn validate_view(
    expected: &DeclarationSource,
    parsed: &Parse,
    definition: &ModuleDecl,
    cancel: &CancellationToken,
) -> Result<(), DeclarationError> {
    let mut script_bodies = Vec::new();
    let mut implementation_index = 0;
    for item in parsed.module().items() {
        match item {
            ItemView::FnDef(function) => {
                if definition.functions.iter().any(|registered| {
                    registered.name == function.name_text().unwrap_or_default()
                        && registered.implementation == CallableImplementation::Script
                }) {
                    let body = function.body().ok_or_else(|| {
                        DeclarationError(
                            "Script registration requires complete supplied body".into(),
                        )
                    })?;
                    script_bodies.push(body.body_span());
                }
            }
            ItemView::TraitDef(contract) => {
                if let Some(registered) = definition
                    .traits
                    .iter()
                    .find(|registered| registered.name == contract.name_text().unwrap_or_default())
                {
                    for method in contract.methods() {
                        if registered.methods.iter().any(|registered| {
                            registered.name == method.name_text().unwrap_or_default()
                                && registered.implementation == CallableImplementation::Script
                        }) {
                            let body = method.body().ok_or_else(|| {
                                DeclarationError(
                                    "Script registration requires complete supplied body".into(),
                                )
                            })?;
                            script_bodies.push(body.body_span());
                        }
                    }
                }
            }
            ItemView::ImplBlock(implementation) => {
                if let Some(registered) = definition.implementations.get(implementation_index) {
                    for method in implementation.methods() {
                        if registered.methods.iter().any(|registered| {
                            registered.name == method.name_text().unwrap_or_default()
                                && registered.implementation == CallableImplementation::Script
                        }) {
                            let body = method.body().ok_or_else(|| {
                                DeclarationError(
                                    "Script registration requires complete supplied body".into(),
                                )
                            })?;
                            script_bodies.push(body.body_span());
                        }
                    }
                }
                implementation_index += 1;
            }
            _ => {}
        }
    }
    let supplied_tokens = parsed
        .module()
        .items()
        .flat_map(|item| {
            NodeRef::from(item)
                .tokens()
                .filter(|token| !token.kind().is_trivia())
                .filter_map(|token| {
                    if let Some(body) = script_bodies.iter().find(|body| {
                        token.span().start >= body.start && token.span().end <= body.end
                    }) {
                        return (token.span().start == body.start)
                            .then(|| (SyntaxKind::Semi, ";".to_owned()));
                    }
                    Some((token.kind(), token.text().to_owned()))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // Lex the tool projection, not an incomplete Script signature as a program.
    // Only the supplied complete source is parsed/lowered and retained as HIR.
    let expected_tokens = lex_with_cancellation(&expected.text, cancel)
        .map_err(|_| DeclarationError("native declaration correspondence cancelled".into()))?
        .iter()
        .filter(|token| !token.kind.is_trivia() && token.kind != TokenKind::Eof)
        .map(|token| {
            (
                token.kind.to_syntax_kind(),
                expected.text[token.span.start..token.span.end].to_owned(),
            )
        })
        .collect::<Vec<_>>();
    if supplied_tokens != expected_tokens {
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
    correspondence: &RegistrationMap,
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
        let declaration =
            definition.definition(ty.kind.definition_kind(), &ty.name, definitions)?;
        let item = correspondence.item(declaration)?;
        if let Item::Struct(_) = item {
            continue;
        }
        if let Item::Enum(id) = item {
            let item = &lowered.module.enums[id.index()];
            if item.variants.len() != ty.variants.len() {
                return Err(DeclarationError("registered enum inventory differs".into()));
            }
            for (member, variant) in item.variants.iter().zip(&ty.variants) {
                if variant.reports_failure {
                    lowered.registered_enum_failures.insert(member.id);
                }
            }
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

                NativeTypeConstructor::Range(kind) => NativeTypeKind::Range(kind),
            },
            _ => return Err(DeclarationError("missing native representation".into())),
        };
        let Item::NativeType(id) = item else {
            return Err(DeclarationError(
                "registered native type kind differs".into(),
            ));
        };
        lowered.native_types.insert(id, kind);
    }
    Ok(())
}

fn attach_functions(
    definition: &ModuleDecl,
    definitions: &DefinitionTable,
    correspondence: &RegistrationMap,
    lowered: &mut LoweredModule,
) -> Result<(), DeclarationError> {
    for contract in &definition.traits {
        let declaration =
            definition.definition(DefinitionKind::Trait, &contract.name, definitions)?;
        let Item::Trait(id) = correspondence.item(declaration)? else {
            return Err(DeclarationError("registered trait kind differs".into()));
        };
        if let Some(adapter) = &contract.conversion_adapter {
            lowered.native_trait_adapters.insert(id, adapter.clone());
        }
    }
    let functions =
        definition
            .functions
            .iter()
            .map(|function| {
                Ok((
                    definition.definition(DefinitionKind::Function, &function.name, definitions)?,
                    function,
                ))
            })
            .chain(definition.traits.iter().flat_map(|contract| {
                let owner =
                    definition.definition(DefinitionKind::Trait, &contract.name, definitions);
                contract.methods.iter().map(move |method| {
                    Ok((
                        ModuleDecl::method_id(owner.clone()?, &method.name, definitions)?,
                        method,
                    ))
                })
            }))
            .chain(definition.implementations.iter().enumerate().flat_map(
                |(index, implementation)| {
                    let owner = definition.implementation_id(index, definitions);
                    implementation.methods.iter().map(move |method| {
                        Ok((
                            ModuleDecl::method_id(owner.clone()?, &method.name, definitions)?,
                            method,
                        ))
                    })
                },
            ))
            .collect::<Result<Vec<_>, DeclarationError>>()?;
    for (declaration, function) in functions {
        let Item::Function(id) = correspondence.item(declaration)? else {
            return Err(DeclarationError("registered function kind differs".into()));
        };
        attach_binding(definition, lowered, id, declaration, function);
    }
    Ok(())
}

fn attach_binding(
    definition: &ModuleDecl,
    lowered: &mut LoweredModule,
    id: FunctionId,
    declaration: DefinitionId,
    function: &FnDecl,
) {
    match &function.implementation {
        CallableImplementation::Native(binding) => {
            lowered.module.functions[id.index()].implementation =
                FunctionImplementation::Native(NativeContract {
                    callable_requirements: definition
                        .callable_requirements
                        .get(&declaration)
                        .cloned()
                        .unwrap_or_default(),
                    concrete_result: definition.concrete_results.get(&declaration).cloned(),
                });
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
            TypeKind::Named(name)
            | TypeKind::Generic { name, .. }
            | TypeKind::Definition { name, .. } => Some(name.as_str()),
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

#[cfg(test)]
mod tests;
