//! Actual library exports, not a hand-written approximation of the foundation API.

use super::{
    ExternalFunction, ExternalModule, ExternalType, evidence::ExternalHost, tests::assert_clean,
};
use crate::tests::standard::standard_modules;
use crate::{
    analysis::{AnalysisDatabase, FileAnalysis, core_source::packaged_modules},
    compile_input::program::CheckedCompilation,
    hir::item::function::NativeContract,
    native::{adoption, render::declaration_source_with_external},
    resolver::resolved::ResolvedName,
    typeck::table::CallTarget,
    types::{TypeId, semantic::lower_type},
};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer, SourceSnapshot},
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{FnDecl, TypeDeclaration, TypeDefKind, module::ModuleDecl},
    language::{Protocol, role::LangRole},
    ty::identity::PortableTypes,
};
use std::sync::Arc;

/// Test-only producer: retain only syntax and explicitly scoped static facts.
/// The database under test never receives these registration descriptors.
pub(crate) fn exported(providers: &[Arc<DefinitionMetadata<ModuleDecl>>]) -> Vec<ExternalModule> {
    let context = DefinitionContext::new().unwrap();
    let adopted = adoption::adopt(providers, &context, &Default::default()).unwrap();
    let providers = adopted.as_slice();
    let mut sources = SourceDatabase::new("/exports").unwrap();
    providers
        .iter()
        .map(|provider| {
            let declaration = provider.records();
            let generated =
                declaration_source_with_external(provider, providers, packaged_modules()).unwrap();
            sources
                .bind_module(&generated.uri, declaration.identity.clone())
                .unwrap();
            let id = sources
                .set(&generated.uri, generated.text, SourceLayer::Base)
                .unwrap();
            let mut input = ExternalModule::new(sources.snapshot().file(id).unwrap().clone());
            input.package_alias = declaration.package_alias.clone();
            input.dependencies = declaration.dependencies.clone();
            input.prelude = declaration.prelude;
            for ty in &declaration.types {
                for variant in &ty.variants {
                    if variant.reports_failure {
                        input
                            .enum_failures
                            .insert((ty.name.clone(), variant.name.clone()));
                    }
                }
                let kind = match ty.kind {
                    TypeDefKind::Native(constructor) => ExternalType::Constructor(constructor),
                    TypeDefKind::NativeStorage(_) => continue,
                    TypeDefKind::Host(_) => {
                        let root = declaration
                            .definition(DefinitionKind::Struct, &ty.name, provider.definitions())
                            .unwrap();
                        let schemas = providers
                            .iter()
                            .flat_map(|provider| {
                                provider.records().types.iter().map(|ty| TypeDeclaration {
                                    declaration: provider
                                        .records()
                                        .definition(
                                            ty.kind.definition_kind(),
                                            &ty.name,
                                            provider.definitions(),
                                        )
                                        .unwrap(),
                                    definition: ty.clone(),
                                })
                            })
                            .collect();
                        input.hosts.insert(
                            ty.name.clone(),
                            Arc::new(
                                DefinitionMetadata::checked(
                                    provider.definitions().clone(),
                                    ExternalHost {
                                        root: TypeDeclaration {
                                            declaration: root,
                                            definition: ty.clone(),
                                        },
                                        schemas,
                                        paths: declaration
                                            .paths
                                            .iter()
                                            .filter(|path| path.root == root)
                                            .cloned()
                                            .collect(),
                                    },
                                    &Default::default(),
                                )
                                .unwrap(),
                            ),
                        );
                        ExternalType::Host
                    }
                    TypeDefKind::Struct | TypeDefKind::Enum => continue,
                };
                input.types.insert(ty.name.clone(), kind);
            }
            for trait_ in &declaration.traits {
                if let Some(relationship) = &trait_.conversion_adapter {
                    input.conversions.insert(
                        trait_.name.clone(),
                        Arc::new(
                            DefinitionMetadata::checked(
                                provider.definitions().clone(),
                                relationship.clone(),
                                &Default::default(),
                            )
                            .unwrap(),
                        ),
                    );
                }
            }
            for trait_ in &declaration.traits {
                let owner = declaration
                    .definition(DefinitionKind::Trait, &trait_.name, provider.definitions())
                    .unwrap();
                for method in &trait_.methods {
                    member(
                        &mut input,
                        provider,
                        ExternalFunction::TraitMethod {
                            owner: trait_.name.clone(),
                            name: method.name.clone(),
                        },
                        ModuleDecl::method_id(owner, &method.name, provider.definitions()).unwrap(),
                        method,
                    );
                }
            }
            for function in &declaration.functions {
                member(
                    &mut input,
                    provider,
                    ExternalFunction::Function(function.name.clone()),
                    declaration
                        .definition(
                            DefinitionKind::Function,
                            &function.name,
                            provider.definitions(),
                        )
                        .unwrap(),
                    function,
                );
            }
            for (index, implementation) in declaration.implementations.iter().enumerate() {
                let owner = declaration
                    .implementation_id(index, provider.definitions())
                    .unwrap();
                for method in &implementation.methods {
                    member(
                        &mut input,
                        provider,
                        ExternalFunction::ImplMethod {
                            owner: index,
                            name: method.name.clone(),
                        },
                        ModuleDecl::method_id(owner, &method.name, provider.definitions()).unwrap(),
                        method,
                    );
                }
            }
            input
        })
        .collect()
}

fn member(
    input: &mut ExternalModule,
    provider: &DefinitionMetadata<ModuleDecl>,
    selector: ExternalFunction,
    id: DefinitionId,
    function: &FnDecl,
) {
    if function.method_policy != MethodPolicy::default() {
        input
            .methods
            .insert(selector.clone(), function.method_policy);
    }
    if matches!(function.implementation, CallableImplementation::Native(_)) {
        let declaration = provider.records();
        let contract = NativeContract {
            host_policy: function.host_policy.clone(),
            receiver_access: function.method_policy.receiver_access,
            callable_requirements: declaration
                .callable_requirements
                .get(&id)
                .cloned()
                .unwrap_or_default(),
            concrete_result: declaration.concrete_results.get(&id).cloned(),
        };
        if contract != NativeContract::default() {
            input.contracts.insert(
                selector,
                Arc::new(
                    DefinitionMetadata::checked(
                        provider.definitions().clone(),
                        contract,
                        &Default::default(),
                    )
                    .unwrap(),
                ),
            );
        }
    }
}

pub(crate) fn change_export(input: &mut ExternalModule, change: impl FnOnce(&str) -> String) {
    let mut sources = SourceDatabase::new("/altered").unwrap();
    sources
        .bind_module(input.source.name(), input.source.module_identity().clone())
        .unwrap();
    let id = sources
        .set(
            input.source.name(),
            change(input.source.text()),
            SourceLayer::Base,
        )
        .unwrap();
    input.source = sources.snapshot().file(id).unwrap().clone();
}

/// Check both forwarders against their authored trait member and generic obligation.
fn check_forwarding_declarations(file: &FileAnalysis) {
    let facts = file.result().facts();
    let definitions = file.definitions();
    let root = definitions.lookup_root(&Protocol::From.module()).unwrap();
    for (trait_name, method_name) in [("Into", "into"), ("TryInto", "try_into")] {
        let interface = definitions
            .lookup_child(root, DefinitionKind::Trait, trait_name, 0)
            .unwrap();
        let mut candidates = facts.aggregates.implementations().filter(|i| {
            i.trait_type.declaration == interface
                && definitions.resolve(i.id).unwrap().module() == &Protocol::From.module()
        });
        let signature = candidates.next().unwrap();
        assert!(candidates.next().is_none());
        let member = definitions
            .lookup_child(interface, DefinitionKind::Method, method_name, 0)
            .unwrap();
        let method = definitions
            .lookup_child(signature.id, DefinitionKind::Method, method_name, 0)
            .unwrap();
        assert_eq!(signature.methods[&member], method);
        let actual = facts
            .typed
            .signatures
            .functions()
            .iter()
            .find(|f| facts.declarations.definition(ResolvedName::Function(f.id)) == Some(&method))
            .unwrap();
        assert_eq!(actual.bounds, signature.bounds);
        assert_eq!(actual.bounds.values().map(Vec::len).sum::<usize>(), 1);
    }
}

#[test]
fn actual_foundation_exports_admit_roles_arithmetic_iteration_and_conversion_without_authority() {
    let providers = standard_modules();
    let exports = exported(&providers);
    let convert = packaged_modules()
        .iter()
        .find(|input| *input.source.module_identity() == Protocol::From.module())
        .unwrap()
        .clone();
    let source = Arc::new(SourceFile::new(
        "/project/main.kgr",
        include_str!("../../../tests/fixtures/analysis/external/foundation_client.kgr"),
    ));
    let root = source.id();
    let sources = SourceSnapshot::single_file(source);
    let mut direct = AnalysisDatabase::new("/project").unwrap();
    direct.set_native_modules(providers);
    let direct = direct
        .snapshot(sources.clone(), &Default::default())
        .unwrap();
    let mut offline = AnalysisDatabase::new("/project").unwrap();
    offline.set_external_modules(exports.clone());
    let fingerprint = offline
        .environment_fingerprint(&Default::default())
        .unwrap();
    let snapshot = offline
        .snapshot(sources.clone(), &Default::default())
        .unwrap();

    assert_clean(&direct);
    assert_clean(&snapshot);
    for analysis in [&direct, &snapshot] {
        let file = analysis
            .files()
            .find(|file| file.source().module_identity() == convert.source.module_identity())
            .unwrap();
        check_forwarding_declarations(file);
    }
    let roles = snapshot
        .files()
        .map(|file| file.result().facts().declarations.language_items.len())
        .sum::<usize>();
    assert_eq!(roles, LangRole::ALL.len());
    assert!(snapshot.inputs().native_modules().is_empty());
    for file in [direct.file(root).unwrap(), snapshot.file(root).unwrap()] {
        let result = file
            .definitions()
            .lookup(&kagari_types::language::binding::result_declaration())
            .unwrap();
        let result = file
            .result()
            .facts()
            .aggregates
            .enumerations()
            .find(|item| item.id == result)
            .unwrap();
        assert_eq!(
            result
                .variants
                .iter()
                .map(|variant| (variant.name.as_str(), variant.reports_failure))
                .collect::<Vec<_>>(),
            [("Ok", false), ("Err", true)]
        );
    }
    let semantics = |file: &FileAnalysis| {
        let facts = file.result().facts();
        let table = facts.typed.type_table();
        let portable = |types: Vec<TypeId>| {
            format!(
                "{:?}",
                PortableTypes::encode(
                    file.definitions(),
                    &types.iter().map(lower_type).collect::<Vec<_>>(),
                    &Default::default()
                )
                .unwrap()
            )
        };
        let path = |id| file.definitions().resolve(id).unwrap().to_path();
        file.result()
            .facts()
            .lowered
            .module
            .nodes
            .expressions()
            .map(|(id, _)| {
                let call = table.call_resolution(id).map(|call| {
                    let target = match call.target {
                        CallTarget::SourceFunction(id) => format!("source:{:?}", path(id)),
                        CallTarget::TraitMethod { method, interface } => format!(
                            "trait:{:?}:{}",
                            path(method),
                            portable(vec![TypeId::Trait(interface)])
                        ),
                        CallTarget::Function(id) => {
                            format!(
                                "local:{:?}",
                                facts.lowered.module.functions[id.index()]
                                    .origin
                                    .diagnostic_span()
                            )
                        }
                        other => format!("{other:?}"),
                    };
                    let signature = call.signature.map(|signature| {
                        let mut types = signature.params;
                        types.push(signature.return_type);
                        portable(types)
                    });
                    (target, portable(call.type_arguments), signature)
                });
                let iteration = table.iteration(id).map(|iteration| {
                    portable(vec![
                        TypeId::Trait(iteration.into_interface.clone()),
                        iteration.iterator.clone(),
                        TypeId::Trait(iteration.next_interface.clone()),
                        iteration.item.clone(),
                    ])
                });
                (
                    file.result()
                        .facts()
                        .lowered
                        .module
                        .expr(id)
                        .origin
                        .diagnostic_span(),
                    file.result()
                        .facts()
                        .typed
                        .type_table()
                        .expr_type(id)
                        .map(|ty| portable(vec![ty])),
                    call,
                    iteration,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        semantics(direct.file(root).unwrap()),
        semantics(snapshot.file(root).unwrap())
    );
    let file = snapshot.file(root).unwrap();
    let facts = file.result().facts();
    let iteration = facts
        .lowered
        .module
        .nodes
        .expressions()
        .find_map(|(id, _)| facts.typed.type_table().iteration(id))
        .unwrap();
    assert_eq!(
        Some(iteration.into_interface.declaration),
        Protocol::Iterable.definition(file.definitions())
    );
    assert_eq!(
        Some(iteration.next_interface.declaration),
        Protocol::Iterator.definition(file.definitions())
    );
    let TypeId::Struct(iterator) = &iteration.iterator else {
        panic!(
            "map should use its authored adapter, got {:?}",
            iteration.iterator
        );
    };
    let definitions = file.definitions();
    let owner = definitions
        .lookup_root(&Protocol::Iterator.module())
        .unwrap();
    assert_eq!(
        Some(iterator.declaration),
        definitions.lookup_child(owner, DefinitionKind::Struct, "MapIterator", 0)
    );
    let item = TypeId::Builtin(kagari_types::scalar::BuiltinType::I32);
    assert_eq!(iterator.arguments, [item.clone(), item.clone()]);
    assert_eq!(iteration.item, item);

    let rebuilt = exported(&standard_modules());
    offline.set_external_modules(rebuilt);
    assert_eq!(
        fingerprint,
        offline
            .environment_fingerprint(&Default::default())
            .unwrap()
    );

    let ops = packaged_modules()
        .iter()
        .find(|m| *m.source.module_identity() == Protocol::Add.module())
        .unwrap();
    let mut altered = ops.clone();
    change_export(&mut altered, |text| {
        let method = text.find("    fn add(self, rhs: Rhs)").unwrap();
        let result = method + text[method..].find(" -> ").unwrap();
        let end = result + text[result..].find(';').unwrap();
        format!("{} -> i64{}", &text[..result], &text[end..])
    });
    offline.replace_core_fixture(altered);
    assert_ne!(
        fingerprint,
        offline
            .environment_fingerprint(&Default::default())
            .unwrap()
    );
    let rejected = offline
        .snapshot(sources.clone(), &Default::default())
        .unwrap();
    assert!(rejected.files().flat_map(|file| file.result().diagnostics()).any(|diagnostic| matches!(&diagnostic.kind, kagari_source::diagnostic::DiagnosticKind::InvalidLanguageRole { role, reason } if role == "add" && reason.contains("member shapes"))));
    // Removing just the blanket leaves TryFrom and its concrete impls intact.
    offline.replace_core_fixture(ops.clone());
    let mut missing = convert.clone();
    let facts = snapshot.file(convert.source.id()).unwrap().result().facts();
    let implementation = facts
        .lowered
        .module
        .impls
        .iter()
        .find(|item| item.methods.iter().any(|method| method.name == "try_into"))
        .unwrap();
    let span = implementation.origin.diagnostic_span();
    change_export(&mut missing, |text| {
        let mut text = text.to_owned();
        text.replace_range(span.start..span.end, "");
        text
    });
    offline.replace_core_fixture(missing);
    let rejected = offline
        .snapshot(sources.clone(), &Default::default())
        .unwrap();
    assert!(
        rejected
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(&d.kind, DiagnosticKind::UnknownName { name } if name == "try_into"))
    );

    // The forward sidecar must not authorize additional trait or method obligations.
    let text = convert.source.text();
    let start = text.find("fn try_from(value: S)").unwrap();
    let method = &text[start..start + text[start..].find(';').unwrap()];
    let bounded_method = format!("{method} where Self: Marker");
    for (from, to) in [
        ("pub trait TryFrom<S> {", "pub trait TryFrom<S>: Marker {"),
        ("pub trait TryFrom<S> {", "pub trait TryFrom<S: Marker> {"),
        (method, bounded_method.as_str()),
    ] {
        let mut constrained = convert.clone();
        change_export(&mut constrained, |text| {
            assert!(text.contains(from), "missing conversion witness {from}");
            format!("pub trait Marker {{}}\n{}", text.replace(from, to))
        });
        let id = constrained.source.id();
        offline.replace_core_fixture(constrained);
        let rejected = offline
            .snapshot(sources.clone(), &Default::default())
            .unwrap();
        let diagnostics = rejected.file(id).unwrap().result().diagnostics();
        assert!(diagnostics.iter().any(|diagnostic| matches!(&diagnostic.kind, kagari_source::diagnostic::DiagnosticKind::InvalidLanguageRole { reason, .. } if reason == "invalid static conversion relationship")), "{from}: {diagnostics:?}");
        assert!(
            !diagnostics.iter().any(|diagnostic| matches!(
                diagnostic.kind,
                kagari_source::diagnostic::DiagnosticKind::UnknownTrait { .. }
            )),
            "{diagnostics:?}"
        );
        assert!(rejected.check_program(root, &Default::default()).is_err());
    }

    // Interning a result identity does not expose a result declaration.
    let mut unbacked = convert.clone();
    let file = snapshot.file(convert.source.id()).unwrap();
    let interface = file
        .definitions()
        .lookup_child(
            file.definitions()
                .lookup_root(&Protocol::From.module())
                .unwrap(),
            DefinitionKind::Trait,
            "TryFrom",
            0,
        )
        .unwrap();
    let adapter = file
        .result()
        .facts()
        .aggregates
        .trait_(&interface)
        .unwrap()
        .conversion_adapter
        .clone()
        .unwrap();
    let metadata =
        DefinitionMetadata::checked(file.definitions().clone(), adapter, &Default::default())
            .unwrap();
    let context = DefinitionContext::new().unwrap();
    let imported = metadata.import_into(&context, &Default::default()).unwrap();
    let mut relationship = imported.into_records();
    let root_id = context
        .intern_root(&kagari_stdlib::namespaces::module("core", "convert"))
        .unwrap();
    relationship.result = context
        .intern_child(root_id, DefinitionKind::Enum, "MissingResult", 0)
        .unwrap();
    unbacked.conversions.insert(
        "TryFrom".into(),
        Arc::new(
            DefinitionMetadata::checked(context.snapshot(), relationship, &Default::default())
                .unwrap(),
        ),
    );
    offline.replace_core_fixture(unbacked);
    let rejected = offline
        .snapshot(sources.clone(), &Default::default())
        .unwrap();
    assert!(rejected.files().flat_map(|file| file.result().diagnostics()).any(|diagnostic| matches!(&diagnostic.kind, kagari_source::diagnostic::DiagnosticKind::InvalidLanguageRole { reason, .. } if reason == "invalid static conversion relationship")));

    offline.replace_core_fixture(convert);
    let mut incomplete = ops.clone();
    change_export(&mut incomplete, |text| {
        text.replace("#[lang = \"add\"]", "#[meta = \"add\"]")
    });
    offline.replace_core_fixture(incomplete);
    let rejected = offline.snapshot(sources, &Default::default()).unwrap();
    assert!(rejected.files().flat_map(|file| file.result().diagnostics()).any(|diagnostic| matches!(&diagnostic.kind, kagari_source::diagnostic::DiagnosticKind::InvalidLanguageRole { role, reason } if role == "add" && reason.contains("missing required"))));
    let program = snapshot.check_program(root, &Default::default()).unwrap();
    CheckedCompilation::from_program(&program, &Default::default()).unwrap();
}
