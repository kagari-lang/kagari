use crate::{analysis::AnalysisDatabase, declarations::DeclarationId};
use kagari_abi::language::{self as standard_traits, Protocol, catalog};
use kagari_common::source_database::{SourceDatabase, SourceLayer};
use std::collections::HashSet;

#[test]
fn installed_declaration_inventory_preserves_every_named_source_site() {
    let sources = SourceDatabase::default();
    let mut database = AnalysisDatabase::default();
    let snapshot = database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let installed = catalog::shared();
    let generated = installed.declaration_source().unwrap();
    let independent = AnalysisDatabase::default()
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    drop(database);
    assert_eq!(snapshot.files().count(), 1);
    let mut identities = HashSet::new();
    let mut count = 0;
    let file = snapshot
        .files()
        .find(|file| file.source().name() == generated.uri)
        .unwrap();
    for (id, site) in &generated.sites {
        let range = site.name_span;
        // Impl header spans name a receiver type, not a named declaration.
        if range.start == range.end
            || id
                .path
                .last()
                .is_some_and(|part| part.kind == kagari_common::identity::DefinitionKind::Impl)
        {
            continue;
        }
        let declaration = snapshot
            .declaration(&DeclarationId::Definition(id.clone()))
            .unwrap_or_else(|| panic!("missing generated declaration: {id:?}"));
        assert_eq!(declaration.location.range, range);
        assert_eq!(&generated.text[range.start..range.end], declaration.name);
        assert!(identities.insert(declaration.id.clone()));
        let metadata = snapshot.documentation(&declaration.id).unwrap();
        assert_eq!(metadata.declaration, *declaration);
        assert_eq!(
            metadata.documentation,
            installed.documentation.get(id).cloned().unwrap_or_default()
        );
        assert!(metadata.written_signature.contains(&declaration.name));
        let other = independent.documentation(&declaration.id).unwrap();
        assert_eq!(other.declaration.id, declaration.id);
        assert_eq!(other.declaration.name, declaration.name);
        assert_eq!(other.declaration.location.range, range);
        assert_eq!(other.documentation, metadata.documentation);
        assert_eq!(other.written_signature, metadata.written_signature);
        assert_eq!(file.source().span(range), Some(declaration.location));
        count += 1;
    }
    assert!(count > 0);
    for kind in Protocol::ALL {
        assert!(identities.contains(&DeclarationId::Definition(standard_traits::identity(kind))));
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
    let snapshot = AnalysisDatabase::default()
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
        let declaration = file.member_at(text.find(needle).unwrap()).unwrap();
        let DeclarationId::Definition(identity) = &declaration.id else {
            panic!("associated declaration identity")
        };
        assert_eq!(identity.path[0].kind, owner);
        assert_eq!(
            identity.path[1].kind,
            kagari_common::identity::DefinitionKind::AssociatedType
        );
        assert!(identities.insert(identity.clone()));
        assert_eq!(file.declarations().get(&declaration.id), Some(declaration));
        let metadata = snapshot.documentation(&declaration.id).unwrap();
        assert_eq!(metadata.documentation, documentation);
        assert!(metadata.written_signature.contains(needle));
    }
}

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
    let uri = catalog::shared().declaration_source().unwrap().uri;
    drop(database);
    let file = snapshot
        .files()
        .find(|file| file.source().name() == uri)
        .unwrap();
    let source = file.source();
    for name in ["ArrayList", "Option", "Iterator"] {
        let declaration = file
            .declarations()
            .iter()
            .find(|declaration| {
                declaration.name == name && matches!(declaration.id, DeclarationId::Definition(_))
            })
            .unwrap();
        let docs = snapshot.documentation(&declaration.id).unwrap();
        assert_eq!(docs.declaration, *declaration);
        assert!(
            !docs.documentation.is_empty(),
            "missing docs for {name}: {docs:?}"
        );
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
