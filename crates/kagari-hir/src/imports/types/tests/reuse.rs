use crate::{imports::tests::insert, tests::test_analysis};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

#[test]
fn foreign_inherited_trait_revision_invalidates_nominal_only_reuse() {
    let mut sources = SourceDatabase::default();
    // The descendant facade can name Parent, but root can reach it only through inheritance.
    let parent_text =
        "pub mod facade; trait Parent { type Item; fn read(self) -> i32; } fn edit() -> i32 { 1 }";
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
    assert!(!old_parent.trait_methods.is_empty());
    assert!(!old_parent.associated_types.is_empty());

    sources
        .set(
            "mem://parent",
            parent_text.replace("{ 1 }", "{ val n = 2; n }"),
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

    // Only the inherited nominal surface changes, not explicit import inputs.
    assert_eq!(old_types.resolutions, current_types.resolutions);
    assert_eq!(old_types.aliases, current_types.aliases);
    assert_eq!(old_types.variants, current_types.variants);
    assert_ne!(old_parent.id.unit.revision, current_parent.id.unit.revision);
    assert_ne!(old_parent.id.unit.arena, current_parent.id.unit.arena);
    assert!(old_parent.same_local_contract(
        current_parent,
        &old_parent.id.unit,
        &current_parent.id.unit,
    ));

    let old_local = old_root.declarations().names.unit.as_ref().unwrap();
    let current_local = current_root.declarations().names.unit.as_ref().unwrap();
    assert_eq!(old_local, current_local);
    assert_ne!(old_parent.id.unit.module, old_local.module);
    assert!(!old_types.same_reuse_inputs(current_types, Some(old_local), Some(current_local),));
    assert!(!current_root.reused());
}
