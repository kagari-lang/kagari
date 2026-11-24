use crate::{imports::tests::insert, tests::test_analysis};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

#[test]
fn inherited_trait_surfaces_follow_edits_without_becoming_explicit_imports() {
    let mut sources = SourceDatabase::default();
    // The descendant facade can name Parent, but root can reach it only through inheritance.
    let parent_text = "pub mod facade; trait Parent { type Item; fn read(self) -> i32; }";
    let parent = insert(&mut sources, "parent", parent_text);
    let facade = insert(
        &mut sources,
        "parent::facade",
        "use pkg::parent::Parent; pub trait Child: Parent {}",
    );
    let root = insert(
        &mut sources,
        "root",
        "use pkg::parent::facade::Child; fn keep<T: Child>(x: T) -> T { x }",
    );
    let mut database = test_analysis();
    let old = database
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    for file in [parent, facade, root] {
        assert!(
            old.file(file).unwrap().diagnostics().is_empty(),
            "{:?}",
            old.file(file).unwrap().diagnostics()
        );
    }
    let old_root = old.file(root).unwrap();
    let old_types = &old_root.declarations().imported_types;
    let (parent_id, old_parent) = old_types
        .nominal_types
        .iter()
        .find(|(_, surface)| surface.declaration.name == "Parent" && surface.id.unit.file == parent)
        .unwrap();
    assert!(
        old_types
            .resolutions
            .values()
            .any(|surface| surface.declaration.name == "Child")
    );
    assert!(
        old_types
            .resolutions
            .values()
            .all(|surface| surface.declaration.id != old_parent.declaration.id)
    );
    assert_eq!(old_parent.trait_methods[0].name, "read");
    assert_eq!(old_parent.associated_types, ["Item"]);

    sources
        .set(
            "mem://parent",
            parent_text.replace("type Item;", "type Item; type Detail;"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let current = database
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    for file in [parent, facade, root] {
        assert!(
            current.file(file).unwrap().diagnostics().is_empty(),
            "{:?}",
            current.file(file).unwrap().diagnostics()
        );
    }
    for file in [facade, root] {
        let before = old.file(file).unwrap().source();
        let after = current.file(file).unwrap().source();
        assert_eq!(before.text(), after.text());
        assert_eq!(before.revision(), after.revision());
    }
    let current_root = current.file(root).unwrap();
    let current_types = &current_root.declarations().imported_types;
    let current_parent = current_types.nominal_types.get(parent_id).unwrap();
    assert!(
        current_types
            .resolutions
            .values()
            .all(|surface| surface.declaration.id != current_parent.declaration.id)
    );

    let child = current_types
        .resolutions
        .values()
        .find(|surface| surface.declaration.name == "Child")
        .unwrap();
    let [supertrait] = child.supertraits.as_slice() else {
        panic!("Child must still inherit Parent");
    };
    assert_eq!(supertrait.declaration, *parent_id);
    assert_eq!(current_parent.associated_types, ["Item", "Detail"]);
    assert_eq!(old_parent.associated_types, ["Item"]);
    assert_eq!(old_parent.trait_methods, current_parent.trait_methods);
    assert_ne!(old_parent.id.unit.arena, current_parent.id.unit.arena);
    for (snapshot, surface) in [(&old, old_parent), (&current, current_parent)] {
        let source = snapshot.file(parent).unwrap().source();
        let location = surface.declaration.source_location().unwrap();
        assert_eq!(location.file, parent);
        assert_eq!(location.revision, source.revision());
        assert_eq!(
            &source.text()[location.range.start..location.range.end],
            "Parent"
        );
    }
}
