//! Lower owned registration records directly; retain complete supplied tool source when present.
mod correspondence;
pub(crate) mod preparation;

use crate::{
    collections::HashSet,
    hir::{
        ids::FunctionId,
        item::{
            Item,
            adt::HostContract,
            function::{FunctionImplementation, NativeContract},
            module::ImportLeaf,
        },
        ty::TypeKind,
    },
    lower::{LoweredModule, lower_module_controlled},
    native::{
        api::preparation::PreparedRegistration,
        paths::LibraryPaths,
        render::DeclarationSource,
        structured::{self, RegistrationMap},
    },
};
use kagari_common::identity::ModuleIdentity;
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
};
use std::collections::BTreeSet;
use std::sync::Arc;

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
        structured::lower(metadata, &prepared.paths, prepared.source.clone(), cancel)?
    };
    if prepared.parsed().is_some() {
        for (name, target) in &definition.exports {
            let target = definitions
                .resolve(*target)
                .map_err(|error| DeclarationError(error.to_string()))?;
            let path = prepared.paths.definition_path(target);
            let mut matching = lowered.module.imports.iter_mut().filter(|import| import.segments.iter().map(String::as_str).eq(path.split("::")) && matches!(&import.kind, ImportLeaf::Named { alias: Some(alias) } if alias == &name.name));
            let import = matching
                .next()
                .ok_or_else(|| DeclarationError(format!("missing export {}", name.name)))?;
            if matching.next().is_some() {
                return Err(DeclarationError(format!("ambiguous export {}", name.name)));
            }
            import.namespace = Some(name.namespace);
        }
    }
    attach_types(
        definition,
        definitions,
        providers,
        &correspondence,
        &mut lowered,
    )?;
    attach_functions(definition, definitions, &correspondence, &mut lowered)?;
    lowered.trait_references = definition
        .trait_references
        .iter()
        .map(|(id, abi)| (*id, abi.clone()))
        .collect();
    lowered.trait_references.sort_by(|(left, _), (right, _)| {
        definitions
            .compare(*left, *right)
            .expect("adopted trait identities")
    });
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
                        && !definition
                            .definition(
                                DefinitionKind::Function,
                                &registered.name,
                                &expected.definitions,
                            )
                            .is_ok_and(|id| definition.script_bodies.contains_key(&id))
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
                    let owner = definition.definition(
                        DefinitionKind::Trait,
                        &registered.name,
                        &expected.definitions,
                    )?;
                    for method in contract.methods() {
                        if registered.methods.iter().any(|registered| {
                            registered.name == method.name_text().unwrap_or_default()
                                && registered.implementation == CallableImplementation::Script
                                && !ModuleDecl::method_id(
                                    owner,
                                    &registered.name,
                                    &expected.definitions,
                                )
                                .is_ok_and(|id| definition.script_bodies.contains_key(&id))
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
                    let owner = definition
                        .implementation_id(implementation_index, &expected.definitions)?;
                    for method in implementation.methods() {
                        if registered.methods.iter().any(|registered| {
                            registered.name == method.name_text().unwrap_or_default()
                                && registered.implementation == CallableImplementation::Script
                                && !ModuleDecl::method_id(
                                    owner,
                                    &registered.name,
                                    &expected.definitions,
                                )
                                .is_ok_and(|id| definition.script_bodies.contains_key(&id))
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
            let item = &mut lowered.module.enums[id.index()];
            if item.variants.len() != ty.variants.len() {
                return Err(DeclarationError("registered enum inventory differs".into()));
            }
            for (member, variant) in item.variants.iter_mut().zip(&ty.variants) {
                member.reports_failure = variant.reports_failure;
            }
            continue;
        }
        let host = matches!(ty.kind, TypeDefKind::Host(_)).then(|| HostContract {
            declaration,
            definition: Arc::new(ty.clone()),
            paths: definition
                .paths
                .iter()
                .filter(|path| path.root == declaration)
                .cloned()
                .collect::<Vec<_>>()
                .into(),
            schemas: schemas.clone(),
        });
        let Item::NativeType(id) = item else {
            return Err(DeclarationError(
                "registered native type kind differs".into(),
            ));
        };
        lowered.module.native_types[id.index()].host = host;
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
            lowered.module.traits[id.index()].conversion_adapter = Some(adapter.clone());
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
        attach_contract(definition, lowered, id, declaration, function);
    }
    Ok(())
}

fn attach_contract(
    definition: &ModuleDecl,
    lowered: &mut LoweredModule,
    id: FunctionId,
    declaration: DefinitionId,
    function: &FnDecl,
) {
    if let CallableImplementation::Native(_) = &function.implementation {
        lowered.module.functions[id.index()].implementation =
            FunctionImplementation::Native(NativeContract {
                host_policy: function.host_policy.clone(),
                receiver_access: function.method_policy.receiver_access,
                callable_requirements: definition
                    .callable_requirements
                    .get(&declaration)
                    .cloned()
                    .unwrap_or_default(),
                concrete_result: definition.concrete_results.get(&declaration).cloned(),
            });
    }
    for item in &mut lowered.module.traits {
        if let Some(method) = item.methods.iter_mut().find(|method| method.function == id) {
            method.override_allowed = function.method_policy.override_allowed;
        }
    }
}

pub(crate) fn dependencies(
    metadata: &DefinitionMetadata<ModuleDecl>,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
    paths: &LibraryPaths,
    lowered: &LoweredModule,
    cancel: &CancellationToken,
) -> Result<BTreeSet<ModuleIdentity>, DeclarationError> {
    let definition = metadata.records();
    let definitions = metadata.definitions();
    let mut imports = HashSet::default();
    let referenced: HashSet<_> = lowered
        .module
        .nodes
        .types
        .iter()
        .filter_map(|(_, ty)| match &ty.kind {
            TypeKind::Named(name)
            | TypeKind::Generic { name, .. }
            | TypeKind::Definition { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let string = NativeTypeConstructor::String.declaration().module;
    if referenced.contains(format!("{}::String", paths.module_path(&string)).as_str()) {
        imports.insert(string);
    }
    for provider in providers
        .iter()
        .map(|provider| provider.records())
        .filter(|provider| provider.identity != definition.identity)
    {
        for ty in &provider.types {
            let alias = format!("{}::{}", paths.module_path(&provider.identity), ty.name);
            if !referenced.contains(alias.as_str()) {
                continue;
            }
            // Providers may have independent definition scopes. Reachability needs
            // the provider's portable module identity, not a cross-scope type lookup.
            imports.insert(provider.identity.clone());
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
    Ok(definition
        .dependencies
        .iter()
        .cloned()
        .chain(imports)
        .collect())
}

#[cfg(test)]
mod tests;
