//! Actual library exports, not a hand-written approximation of the foundation API.

use super::{
    ExternalFunction, ExternalModule, ExternalType, evidence::ExternalHost, tests::assert_clean,
};
use crate::{
    analysis::{AnalysisDatabase, FileAnalysis},
    compile_input::program::CheckedCompilation,
    hir::item::function::NativeContract,
    native::{adoption, render::declaration_source},
    typeck::table::CallTarget,
    types::{TypeId, semantic::lower_type},
};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_source::{
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
pub(super) fn exported(providers: &[Arc<DefinitionMetadata<ModuleDecl>>]) -> Vec<ExternalModule> {
    let context = DefinitionContext::new().unwrap();
    let adopted = adoption::adopt(providers, &context, &Default::default()).unwrap();
    let providers = adopted.as_slice();
    let mut sources = SourceDatabase::new("/exports").unwrap();
    providers
        .iter()
        .map(|provider| {
            let declaration = provider.records();
            let generated = declaration_source(provider, providers).unwrap();
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

#[test]
fn actual_foundation_exports_admit_roles_arithmetic_iteration_and_conversion_without_authority() {
    let providers = kagari_stdlib::catalog::shared().unwrap();
    let exports = exported(&providers);
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
                            format!("local:{:?}", facts.lowered.source_map.function_span(id))
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
                    file.result().facts().lowered.source_map.expr_span(id),
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
    assert!(matches!(iteration.iterator, TypeId::NativeObject(_)));
    assert_eq!(
        iteration.item,
        TypeId::Builtin(kagari_types::scalar::BuiltinType::I32)
    );

    let rebuilt = exported(&kagari_stdlib::catalog::shared().unwrap());
    offline.set_external_modules(rebuilt);
    assert_eq!(
        fingerprint,
        offline
            .environment_fingerprint(&Default::default())
            .unwrap()
    );

    fn change_export(input: &mut ExternalModule, change: impl FnOnce(&str) -> String) {
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
    let mut altered = exports.clone();
    let owner = altered
        .iter_mut()
        .find(|input| input.source.text().contains("pub trait Add<"))
        .unwrap();
    change_export(owner, |text| {
        let method = text.find("fn add(").unwrap();
        let result = method + text[method..].find(" -> ").unwrap();
        let end = result + text[result..].find(';').unwrap();
        format!("{} -> i64{}", &text[..result], &text[end..])
    });
    offline.set_external_modules(altered);
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
    // Deriving Into from From must not invent receiver or method obligations.
    for (from, to) in [
        ("pub trait Into<T> {", "pub trait Into<T>: Marker {"),
        ("pub trait Into<T> {", "pub trait Into<T: Marker> {"),
        (
            "fn into(self) -> T;",
            "fn into(self) -> T where Self: Marker;",
        ),
    ] {
        let mut constrained = exports.clone();
        let owner = constrained
            .iter_mut()
            .find(|input| input.conversions.contains_key("Into"))
            .unwrap();
        change_export(owner, |text| {
            assert!(text.contains(from), "missing conversion witness {from}");
            format!("pub trait Marker {{}}\n{}", text.replace(from, to))
        });
        let id = owner.source.id();
        offline.set_external_modules(constrained);
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

    // A valid identity-table entry is not a supplied forward trait declaration.
    let mut unbacked = exports.clone();
    let owner = unbacked
        .iter_mut()
        .find(|input| input.conversions.contains_key("Into"))
        .unwrap();
    let metadata = owner.conversions.get_mut("Into").unwrap();
    let context = DefinitionContext::new().unwrap();
    let imported = metadata.import_into(&context, &Default::default()).unwrap();
    let mut relationship = imported.into_records();
    let kagari_types::declaration::conversion::ConversionAdapter::Reverse {
        origin, method, ..
    } = &mut relationship
    else {
        panic!("reverse conversion")
    };
    let forward_root = context
        .intern_root(&kagari_stdlib::namespaces::module("core", "convert"))
        .unwrap();
    *origin = context
        .intern_child(forward_root, DefinitionKind::Trait, "MissingFrom", 0)
        .unwrap();
    *method = context
        .intern_child(*origin, DefinitionKind::Method, "from", 0)
        .unwrap();
    *metadata = Arc::new(
        DefinitionMetadata::checked(context.snapshot(), relationship, &Default::default()).unwrap(),
    );
    offline.set_external_modules(unbacked);
    let rejected = offline
        .snapshot(sources.clone(), &Default::default())
        .unwrap();
    assert!(rejected.files().flat_map(|file| file.result().diagnostics()).any(|diagnostic| matches!(&diagnostic.kind, kagari_source::diagnostic::DiagnosticKind::InvalidLanguageRole { reason, .. } if reason == "invalid static conversion relationship")));

    let mut incomplete = exports;
    let owner = incomplete
        .iter_mut()
        .find(|input| input.source.text().contains("pub trait Add<"))
        .unwrap();
    change_export(owner, |text| {
        text.replace("#[lang = \"add\"]", "#[meta = \"add\"]")
    });
    offline.set_external_modules(incomplete);
    let rejected = offline.snapshot(sources, &Default::default()).unwrap();
    assert!(rejected.files().flat_map(|file| file.result().diagnostics()).any(|diagnostic| matches!(&diagnostic.kind, kagari_source::diagnostic::DiagnosticKind::InvalidLanguageRole { role, reason } if role == "add" && reason.contains("missing required"))));
    assert_clean(&snapshot);
    assert_clean(&direct);
    let program = snapshot.check_program(root, &Default::default()).unwrap();
    CheckedCompilation::from_program(&program, &Default::default()).unwrap();
}
