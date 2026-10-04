use crate::{declarations::DeclarationId, tests::test_analysis};
use kagari_common::identity::DefinitionKind;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

#[test]
fn source_methods_defaults_and_overrides_keep_their_own_declarations() {
    let text = r#"
trait Read {
    /// Inherited body.
    fn inherited(self) -> i32 { 7 }
    fn read(self) -> i32;
}
struct Packet {}
impl Read for Packet {
    /// Concrete body.
    fn read(self) -> i32 { 42 }
}
impl Packet {
    /// User spelling, not the String implementation.
    fn len_bytes(self) -> i32 { 1 }
}
fn inspect(value: Packet) { value. }
fn generic<T: Read>(value: T) { value. }
"#;
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("methods.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(id).unwrap();
    let candidates = file.method_completions(text.find("value. }").unwrap() + 6);
    for (name, owner, expected_docs) in [
        ("inherited", DefinitionKind::Trait, "Inherited body."),
        ("read", DefinitionKind::Impl, "Concrete body."),
        (
            "len_bytes",
            DefinitionKind::Impl,
            "User spelling, not the String implementation.",
        ),
    ] {
        let matching = candidates
            .iter()
            .filter(|candidate| candidate.name == name)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "{name}: {candidates:?}");
        let candidate = matching[0];
        let DeclarationId::Definition(identity) = &candidate.declaration else {
            panic!("source identity")
        };
        assert_eq!(identity.path[0].kind, owner);
        let declaration = snapshot.declaration(&candidate.declaration).unwrap();
        assert_eq!(declaration.location.file, id);
        assert_eq!(
            &text[declaration.location.range.start..declaration.location.range.end],
            name
        );
        assert_eq!(
            snapshot
                .declaration_snapshot()
                .documentation(&candidate.declaration)
                .unwrap()
                .documentation,
            expected_docs
        );
    }
    let generic = file.method_completions(text.rfind("value. }").unwrap() + 6);
    for name in ["inherited", "read"] {
        let method = generic
            .iter()
            .find(|candidate| candidate.name == name)
            .unwrap();
        let DeclarationId::Definition(identity) = &method.declaration else {
            panic!("trait identity")
        };
        assert_eq!(identity.path[0].kind, DefinitionKind::Trait);
    }
    assert!(
        !generic
            .iter()
            .any(|candidate| candidate.name == "len_bytes")
    );
}

#[test]
fn inherent_completion_filters_known_bounds_but_retains_method_generics() {
    let text = r#"
struct Box<T> { val value: T }
impl<T> Box<T> {
    fn ordered(self) -> i32 where T: Ord { 42 }
    fn choose<U>(self, value: U) -> U { value }
}
fn integers(value: Box<i32>) { value. }
fn floats(value: Box<f64>) { value. }
"#;
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("bounds.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot.file(id).unwrap();
    for (offset, ordered) in [
        (text.find("value. }").unwrap() + 6, true),
        (text.rfind("value. }").unwrap() + 6, false),
    ] {
        let methods = file.method_completions(offset);
        assert_eq!(
            methods.iter().any(|method| method.name == "ordered"),
            ordered,
            "{methods:?}"
        );
        assert!(
            methods.iter().any(|method| method.name == "choose"),
            "{methods:?}"
        );
    }
}
