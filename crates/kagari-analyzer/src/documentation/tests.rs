use crate::query_tests::native_fixture::function_module;
use crate::query_tests::{publish_native_documents, standard_modules};
use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, metadata::DefinitionMetadata,
};
use kagari_hir::analysis::{AnalysisDatabase, core_source::packaged_modules};
use kagari_hir::{
    declarations::{DeclarationId, DeclarationOrigin},
    semantic::signatures::FunctionImplementation,
};
use kagari_source::source_database::{CapturedSource, SourceDatabase, SourceLayer};
use kagari_types::{language as standard_traits, language::Protocol, scalar::BuiltinType, ty::Ty};
use std::{collections::HashSet, sync::Arc};

#[test]
fn installed_declaration_inventory_preserves_every_named_source_site() {
    let mut sources = SourceDatabase::default();
    let installed_modules = standard_modules();
    let (mut database, views) = publish_native_documents(&mut sources, &installed_modules);
    let snapshot = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let mut independent = AnalysisDatabase::default();
    independent.set_import_environment(snapshot.import_environment().clone());
    let independent = independent
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    drop(database);
    assert_eq!(
        snapshot.files().count(),
        installed_modules.len() + packaged_modules().len()
    );
    let mut identities = HashSet::new();
    let mut count = 0;
    for (installed, generated) in installed_modules.iter().zip(&views) {
        let file = snapshot
            .files()
            .find(|file| file.source().name() == generated.uri)
            .unwrap();
        let module_doc = snapshot
            .queries()
            .module_documentation(&installed.records().identity)
            .unwrap();
        assert_eq!(module_doc.location.unwrap().file, file.source().id());
        assert_eq!(
            module_doc.documentation,
            installed.records().module_documentation
        );
        for (id, site) in &generated.sites {
            let range = site.name_span;
            // Impl header spans name a receiver type, not a named declaration.
            if range.start == range.end
                || generated
                    .definitions
                    .resolve(*id)
                    .unwrap()
                    .last()
                    .is_some_and(|part| part.kind == kagari_common::identity::DefinitionKind::Impl)
            {
                continue;
            }
            // Generated sites belong to the installed provider scope. Resolve the
            // exact external query separately in each independent snapshot.
            let query = generated.definitions.resolve(*id).unwrap().to_path();
            let scoped = file.declarations().definitions().lookup(&query).unwrap();
            let declaration = snapshot
                .declaration(&DeclarationId::Definition(scoped))
                .unwrap_or_else(|| panic!("missing generated declaration: {id:?}"));
            assert_eq!(
                file.source()
                    .local_range(declaration.source_location().unwrap()),
                Some(range)
            );
            assert_eq!(&generated.text[range.start..range.end], declaration.name);
            assert!(identities.insert(declaration.id.clone()));
            let metadata = snapshot.queries().documentation(&declaration.id).unwrap();
            assert_eq!(metadata.declaration, *declaration);
            assert_eq!(
                declaration.source_location().unwrap().file,
                file.source().id()
            );
            assert_eq!(
                metadata.documentation,
                installed
                    .records()
                    .documentation
                    .get(id)
                    .cloned()
                    .unwrap_or_default()
            );
            assert!(
                metadata
                    .written_signature
                    .as_deref()
                    .unwrap()
                    .contains(&declaration.name)
            );
            assert!(
                independent
                    .queries()
                    .documentation(&declaration.id)
                    .is_none()
            );
            let other_file = independent
                .files()
                .find(|file| file.source().name() == generated.uri)
                .unwrap();
            let other_id = other_file
                .declarations()
                .definitions()
                .lookup(&query)
                .unwrap();
            let other = independent
                .queries()
                .documentation(&DeclarationId::Definition(other_id))
                .unwrap();
            assert_ne!(other.declaration.id, declaration.id);
            assert_eq!(other.declaration.name, declaration.name);
            assert_eq!(
                other.declaration.source_location().unwrap().range,
                declaration.source_location().unwrap().range
            );
            assert_eq!(other.documentation, metadata.documentation);
            assert_eq!(other.written_signature, metadata.written_signature);
            assert_eq!(
                file.source().span(range),
                Some(declaration.source_location().unwrap())
            );
            count += 1;
        }
    }
    assert!(count > 0);
    for kind in Protocol::ALL {
        let path = standard_traits::identity(kind);
        let owner = snapshot
            .files()
            .find(|file| file.source().module_identity() == &path.module)
            .unwrap();
        let id =
            DeclarationId::Definition(owner.declarations().definitions().lookup(&path).unwrap());
        if let Some(authored) = packaged_modules()
            .iter()
            .find(|module| module.module_identity() == &path.module)
        {
            let doc = snapshot.queries().documentation(&id).unwrap();
            assert_eq!(
                doc.declaration.source_location().unwrap().file,
                authored.id()
            );
            assert!(!doc.documentation.is_empty());
            assert!(!identities.contains(&id));
        } else {
            assert!(identities.contains(&id));
        }
    }
}

#[test]
fn associated_type_declarations_keep_owner_identity_and_written_documentation() {
    let text = r#"
trait Items {
    /// Contract item.
    type Item;
}
struct Values {}
impl Items for Values {
    /// Implementation item.
    type Item = i32;
}
"#;
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("items.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(id).unwrap();
    let mut identities = HashSet::new();
    for (needle, documentation, owner) in [
        (
            "Item;",
            "Contract item.",
            kagari_common::identity::DefinitionKind::Trait,
        ),
        (
            "Item =",
            "Implementation item.",
            kagari_common::identity::DefinitionKind::Impl,
        ),
    ] {
        let declaration = file
            .queries()
            .member_at(text.find(needle).unwrap())
            .unwrap();
        let DeclarationId::Definition(identity) = &declaration.id else {
            panic!("associated declaration identity")
        };
        let path = file
            .declarations()
            .definitions()
            .resolve(*identity)
            .unwrap()
            .to_path();
        assert_eq!(path.path[0].kind, owner);
        assert_eq!(
            path.path[1].kind,
            kagari_common::identity::DefinitionKind::AssociatedType
        );
        assert!(identities.insert(*identity));
        assert_eq!(file.declarations().get(&declaration.id), Some(declaration));
        let metadata = snapshot.queries().documentation(&declaration.id).unwrap();
        assert_eq!(metadata.documentation, documentation);
        assert!(
            metadata
                .written_signature
                .as_deref()
                .unwrap()
                .contains(needle)
        );
    }
}

#[test]
fn documentation_uses_declaration_identity_and_survives_edits() {
    let original = "/// Original 文本.\r\n///\r\n/// ```kgr\r\n/// value()\r\n/// ```\r\nfn value() -> Missing { unresolved }\r\n";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("docs.kgr", original.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let first = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let declaration = first
        .file(file)
        .unwrap()
        .declarations()
        .iter()
        .find(|declaration| declaration.name == "value")
        .unwrap();
    let id = declaration.id.clone();
    let old = first.queries().documentation(&id).unwrap();
    assert_eq!(old.declaration, *declaration);
    assert_eq!(old.documentation, "Original 文本.\n\n```kgr\nvalue()\n```");
    assert_eq!(
        old.written_signature.as_deref(),
        Some("fn value() -> Missing { unresolved }")
    );
    assert!(first.file(file).unwrap().diagnostics().is_empty());
    sources
        .set(
            "docs.kgr",
            original.replace("Original 文本.", "Updated docs."),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    drop(database);
    assert_eq!(first.queries().documentation(&id).unwrap(), old);
    let updated = second.queries().documentation(&id).unwrap();
    assert!(updated.documentation.starts_with("Updated docs."));
    assert_ne!(
        updated.declaration.source_location().unwrap().revision,
        old.declaration.source_location().unwrap().revision
    );
}

#[test]
fn same_named_declarations_and_inline_modules_keep_their_own_documentation() {
    let text = "/// Outer.\nfn same() {}\nmod nested {\n/// Inner.\nfn same() {}\n}\n";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("nested.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let mut docs = snapshot
        .files()
        .flat_map(|file| file.declarations().iter())
        .filter(|declaration| declaration.name == "same")
        .map(|declaration| snapshot.queries().documentation(&declaration.id).unwrap())
        .collect::<Vec<_>>();
    docs.sort_by_key(|doc| doc.declaration.source_location().unwrap().range.start);
    assert_eq!(docs.len(), 2);
    assert_eq!(docs[0].documentation, "Outer.");
    assert_eq!(docs[1].documentation, "Inner.");
    assert_ne!(docs[0].declaration.id, docs[1].declaration.id);
    assert!(
        docs.iter()
            .all(|doc| doc.declaration.source_location().unwrap().file == file)
    );
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn installed_native_docs_are_owned_by_the_snapshot() {
    let mut sources = SourceDatabase::default();
    let user = sources
        .set(
            "user.kgr",
            "/// User Option.\nstruct Option {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut database = test_analysis();
    let snapshot = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    drop(database);
    for name in ["Vec", "Option", "Iterator"] {
        let file = snapshot
            .files()
            .find(|file| {
                file.source().module_identity()
                    == &if name == "Iterator" {
                        standard_traits::identity(Protocol::Iterator).module
                    } else {
                        kagari_stdlib::namespaces::type_owner(name)
                    }
            })
            .unwrap();
        let declaration = file
            .declarations()
            .iter()
            .find(|declaration| {
                declaration.name == name && matches!(declaration.id, DeclarationId::Definition(_))
            })
            .unwrap();
        let docs = snapshot.queries().documentation(&declaration.id).unwrap();
        assert_eq!(docs.declaration, *declaration);
        assert!(
            !docs.documentation.is_empty(),
            "missing docs for {name}: {docs:?}"
        );
        assert!(docs.written_signature.as_deref().unwrap().contains(name));
        assert_eq!(
            snapshot
                .source(declaration.source_location().unwrap().file)
                .unwrap()
                .span(declaration.source_location().unwrap().range),
            Some(docs.declaration.source_location().unwrap())
        );
    }
    let declaration = snapshot
        .file(user)
        .unwrap()
        .declarations()
        .iter()
        .find(|declaration| declaration.name == "Option")
        .unwrap();
    assert_eq!(
        snapshot
            .queries().documentation(&declaration.id)
            .unwrap()
            .documentation,
        "User Option."
    );
}
*/

#[test]
fn native_view_locations_are_snapshot_owned_and_do_not_select_binding_identity() {
    let identity = ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec![],
    };
    let module = function_module(
        identity.clone(),
        "echo",
        vec![("value", Ty::Builtin(BuiltinType::I32))],
        Ty::Builtin(BuiltinType::I32),
    );
    let mut records = module.records().clone();
    let echo_id = records
        .definition(DefinitionKind::Function, "echo", module.definitions())
        .unwrap();
    records
        .documentation
        .insert(echo_id, "Echo structured documentation.".into());
    records.module_documentation = "Demo structured module.".into();
    let module = Arc::new(
        DefinitionMetadata::checked(module.definitions().clone(), records, &Default::default())
            .unwrap(),
    );
    let mut sources = SourceDatabase::default();
    let text = "use demo::echo; fn main() -> i32 { echo(1) }";
    let root = sources
        .set("native-location.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let offset = text.rfind("echo(1)").unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(vec![module.clone()]);
    let structured = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let target = structured.queries().definition_at(root, offset).unwrap();
    let DeclarationId::Definition(declaration) = target.id else {
        panic!("registered definition")
    };
    assert_eq!(target.origin, DeclarationOrigin::Structured { declaration });
    assert!(target.source_location().is_none());
    let structured_docs = structured.queries().documentation_at(root, offset).unwrap();
    assert_eq!(structured_docs.declaration, *target);
    assert_eq!(
        structured_docs.documentation,
        "Echo structured documentation."
    );
    assert!(structured_docs.written_signature.is_none());
    let declaration_snapshot = structured.declaration_snapshot();
    let module_docs = declaration_snapshot
        .queries()
        .module_documentation(&identity)
        .unwrap();
    assert_eq!(module_docs.documentation, "Demo structured module.");
    assert!(module_docs.location.is_none());
    let provider = structured
        .files()
        .find(|file| file.source().module_identity() == &identity)
        .unwrap();
    assert!(provider.queries().definition_at(0).is_none());
    assert!(
        structured
            .queries()
            .analysis_at(provider.source().id(), 0)
            .is_none()
    );
    let uris = [
        "file:///not-installed/provider.kgr",
        "kagari://std/math.kgr",
    ];
    let view = declaration_snapshot
        .render_native_document(&module, &Default::default())
        .unwrap();
    let mut editor = AnalysisDatabase::default();
    editor.set_import_environment(declaration_snapshot.import_environment().clone());
    let mut retained = Vec::new();
    for uri in uris {
        let input = sources
            .replace_generated(
                vec![CapturedSource {
                    key: view.uri.clone(),
                    display_name: uri.into(),
                    module: identity.clone(),
                    text: view.text.clone(),
                }],
                &Default::default(),
            )
            .unwrap();
        let snapshot = editor.snapshot(input, &Default::default()).unwrap();
        let file = snapshot.file(root).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        snapshot.check_program(root, &Default::default()).unwrap();
        let target = file.queries().source_function_at(offset).unwrap();
        let FunctionImplementation::Native(_) = target.signature.implementation else {
            panic!("registered native entry")
        };
        let binding = target.declaration;
        assert_eq!(
            file.definitions().resolve(binding).unwrap().module(),
            &identity
        );
        let docs = snapshot.queries().documentation_at(root, offset).unwrap();
        assert_eq!(
            snapshot
                .source(docs.declaration.source_location().unwrap().file)
                .unwrap()
                .name(),
            uri
        );
        assert!(docs.written_signature.as_deref().unwrap().contains("echo"));
        retained.push((snapshot, docs));
    }
    let old = retained[0].1.declaration.source_location().unwrap();
    let new = retained[1].1.declaration.source_location().unwrap();
    assert_eq!(old.file, new.file);
    assert_ne!(old.revision, new.revision);
    drop(database);
    assert_eq!(
        structured.queries().documentation_at(root, offset),
        Some(structured_docs)
    );
    for ((snapshot, docs), uri) in retained.iter().zip(uris) {
        assert_eq!(
            snapshot
                .source(docs.declaration.source_location().unwrap().file)
                .unwrap()
                .name(),
            uri
        );
        assert_eq!(
            snapshot.queries().documentation_at(root, offset),
            Some(docs.clone())
        );
    }
}
