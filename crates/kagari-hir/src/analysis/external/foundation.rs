//! Actual library exports, not a hand-written approximation of the foundation API.

use super::tests::assert_clean;
use crate::tests::standard::standard_modules;
use crate::{
    analysis::{
        AnalysisDatabase, FileAnalysis,
        core_source::{change_core, packaged_modules},
        declarations::DeclarationSnapshot,
    },
    compile_input::program::CheckedCompilation,
    resolver::resolved::ResolvedName,
    typeck::table::CallTarget,
    types::{TypeId, semantic::lower_type},
};
use kagari_common::identity::DefinitionKind;
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{CapturedSource, SourceDatabase, SourceLayer},
};
use kagari_types::{
    language::{Protocol, role::LangRole},
    ty::identity::PortableTypes,
};

/// Analyze only published text under the scopes selected for the structured query.
pub(super) fn publish(
    declarations: &DeclarationSnapshot,
    sources: &mut SourceDatabase,
) -> AnalysisDatabase {
    let cancel = Default::default();
    let documents = declarations
        .native_modules()
        .iter()
        .map(|provider| {
            let doc = declarations
                .render_native_document(provider, &cancel)
                .unwrap();
            CapturedSource {
                key: doc.uri.clone(),
                display_name: doc.uri,
                module: provider.records().identity.clone(),
                text: doc.text,
            }
        })
        .collect();
    sources.replace_generated(documents, &cancel).unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_import_environment(declarations.import_environment().clone());
    analysis
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
    let convert = packaged_modules()
        .iter()
        .find(|input| *input.module_identity() == Protocol::From.module())
        .unwrap()
        .clone();
    let mut files = SourceDatabase::new("/project").unwrap();
    let root = files
        .set(
            "main.kgr",
            include_str!("../../../tests/fixtures/analysis/external/foundation_client.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut direct = AnalysisDatabase::new("/project").unwrap();
    direct.set_native_modules(providers);
    let direct = direct
        .snapshot(files.snapshot(), &Default::default())
        .unwrap();
    let mut offline = publish(direct.declaration_snapshot(), &mut files);
    let sources = files.snapshot();
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
            .find(|file| file.source().module_identity() == convert.module_identity())
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

    let ops = packaged_modules()
        .iter()
        .find(|m| *m.module_identity() == Protocol::Add.module())
        .unwrap();
    let mut altered = ops.clone();
    change_core(&mut altered, |text| {
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
    let facts = snapshot.file(convert.id()).unwrap().result().facts();
    let implementation = facts
        .lowered
        .module
        .impls
        .iter()
        .find(|item| item.methods.iter().any(|method| method.name == "try_into"))
        .unwrap();
    let span = implementation.origin.diagnostic_span();
    change_core(&mut missing, |text| {
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

    // Canonical conversion recognition must reject additional trait or method obligations.
    let text = convert.text();
    let start = text.find("fn try_from(value: S)").unwrap();
    let method = &text[start..start + text[start..].find(';').unwrap()];
    let bounded_method = format!("{method} where Self: Marker");
    for (from, to) in [
        ("pub trait TryFrom<S> {", "pub trait TryFrom<S>: Marker {"),
        ("pub trait TryFrom<S> {", "pub trait TryFrom<S: Marker> {"),
        (method, bounded_method.as_str()),
    ] {
        let mut constrained = convert.clone();
        change_core(&mut constrained, |text| {
            assert!(text.contains(from), "missing conversion witness {from}");
            format!("pub trait Marker {{}}\n{}", text.replace(from, to))
        });
        let id = constrained.id();
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

    offline.replace_core_fixture(convert);
    let mut incomplete = ops.clone();
    change_core(&mut incomplete, |text| {
        text.replace("#[lang = \"add\"]", "#[meta = \"add\"]")
    });
    offline.replace_core_fixture(incomplete);
    let rejected = offline.snapshot(sources, &Default::default()).unwrap();
    assert!(rejected.files().flat_map(|file| file.result().diagnostics()).any(|diagnostic| matches!(&diagnostic.kind, kagari_source::diagnostic::DiagnosticKind::InvalidLanguageRole { role, reason } if role == "add" && reason.contains("missing required"))));
    let program = snapshot.check_program(root, &Default::default()).unwrap();
    CheckedCompilation::from_program(&program, &Default::default()).unwrap();
}
