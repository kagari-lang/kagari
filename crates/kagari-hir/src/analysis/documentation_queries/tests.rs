use crate::{analysis::AnalysisDatabase, declarations::DeclarationId};
use kagari_common::source_database::{SourceDatabase, SourceLayer};

#[test]
fn documentation_uses_declaration_identity_and_survives_edits() {
    let original = "/// Original 文本.\r\n///\r\n/// ```kgr\r\n/// value()\r\n/// ```\r\nfn value() -> Missing { unresolved }\r\n";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("docs.kgr", original.into(), SourceLayer::Base)
        .unwrap();
    let mut database = AnalysisDatabase::default();
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
    let old = first.documentation(&id).unwrap();
    assert_eq!(old.declaration, *declaration);
    assert_eq!(old.documentation, "Original 文本.\n\n```kgr\nvalue()\n```");
    assert_eq!(
        old.written_signature,
        "fn value() -> Missing { unresolved }"
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
    assert_eq!(first.documentation(&id).unwrap(), old);
    let updated = second.documentation(&id).unwrap();
    assert!(updated.documentation.starts_with("Updated docs."));
    assert_ne!(
        updated.declaration.location.revision,
        old.declaration.location.revision
    );
}

#[test]
fn same_named_declarations_and_inline_modules_keep_their_own_documentation() {
    let text = "/// Outer.\nfn same() {}\nmod nested {\n/// Inner.\nfn same() {}\n}\n";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("nested.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let mut docs = snapshot
        .files
        .values()
        .flat_map(|file| file.declarations().iter())
        .filter(|declaration| declaration.name == "same")
        .map(|declaration| snapshot.documentation(&declaration.id).unwrap())
        .collect::<Vec<_>>();
    docs.sort_by_key(|doc| doc.declaration.location.range.start);
    assert_eq!(docs.len(), 2);
    assert_eq!(docs[0].documentation, "Outer.");
    assert_eq!(docs[1].documentation, "Inner.");
    assert_ne!(docs[0].declaration.id, docs[1].declaration.id);
    assert!(docs.iter().all(|doc| doc.declaration.location.file == file));
}

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
    let mut database = AnalysisDatabase::default();
    let snapshot = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let installed = database.stdlib.get().unwrap().clone();
    drop(database);
    for (uri, name) in [
        ("array", "ArrayList"),
        ("option", "Option"),
        ("option", "Some"),
        ("iter", "next"),
    ] {
        let source = installed
            .package
            .files()
            .iter()
            .find(|file| file.source().name() == format!("kagari://std/{uri}.kgr"))
            .unwrap()
            .source();
        let file = snapshot.file(source.id()).unwrap();
        let declaration = file
            .declarations()
            .iter()
            .find(|declaration| {
                declaration.name == name && matches!(declaration.id, DeclarationId::Definition(_))
            })
            .unwrap();
        let docs = snapshot.documentation(&declaration.id).unwrap();
        assert_eq!(docs.declaration, *declaration);
        assert!(!docs.documentation.is_empty());
        assert!(docs.written_signature.contains(name));
        assert_eq!(
            source.span(declaration.location.range),
            Some(docs.declaration.location)
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
            .documentation(&declaration.id)
            .unwrap()
            .documentation,
        "User Option."
    );
}
