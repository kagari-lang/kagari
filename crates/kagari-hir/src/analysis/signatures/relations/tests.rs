use super::*;
use crate::{
    analysis::AnalysisDatabase, declarations::Declaration, tests::test_analysis, types::NominalType,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

const TEXT: &str = include_str!("../../../../tests/fixtures/analysis/implementation-relations.kgr");

fn setup() -> (AnalysisDatabase, SourceDatabase, FileId, SignatureSnapshot) {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("relations.kgr", TEXT.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let signatures = database
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    (database, sources, id, signatures)
}

fn named(signatures: &SignatureSnapshot, file: FileId, offset: usize) -> &Declaration {
    signatures
        .file(file)
        .unwrap()
        .definition_at(offset)
        .unwrap()
}

fn identity(declaration: &Declaration) -> DefinitionId {
    let DeclarationId::Definition(id) = declaration.id else {
        panic!("definition")
    };
    id
}

fn query(
    relations: &ImplementationRelations,
    declaration: Option<DefinitionId>,
    receiver: Option<TypeId>,
) -> Vec<ImplementationLocation> {
    relations
        .query(
            &ImplementationTarget {
                declaration,
                receiver,
                assumptions: Default::default(),
            },
            &Default::default(),
        )
        .unwrap()
}

#[test]
fn reverse_relations_preserve_templates_defaults_and_checked_generic_applicability() {
    let (database, _sources, file, signatures) = setup();
    let relations = signatures
        .implementation_relations(Default::default(), &Default::default())
        .unwrap();
    assert!(
        database.files.is_empty(),
        "reverse declarations do not check unrelated bodies"
    );
    let read = identity(named(&signatures, file, TEXT.find("Read {").unwrap()));
    let blocks = query(&relations, Some(read), None);
    assert_eq!(blocks.len(), 3);
    assert!(blocks.iter().all(|location| {
        TEXT[location.extent.range.start..location.extent.range.end].starts_with("impl")
    }));
    let inherited = identity(named(
        &signatures,
        file,
        TEXT.find("inherited(self)").unwrap(),
    ));
    let defaults = query(&relations, Some(inherited), None);
    assert_eq!(
        defaults.len(),
        2,
        "shared inherited body is deduplicated after semantic selection"
    );
    let item = identity(named(&signatures, file, TEXT.find("Item;").unwrap()));
    let associated = query(&relations, Some(item), None);
    assert_eq!(associated.len(), 3);
    let constrained = TypeId::Trait(NominalType {
        declaration: read,
        arguments: vec![],
        associated_types: [(item, TypeId::from_name("i32").unwrap())].into(),
    });
    assert_eq!(
        query(&relations, None, Some(constrained)).len(),
        2,
        "a known interface output excludes implementations with another associated type"
    );
    assert!(associated.iter().all(|location| &TEXT
        [location.selection.range.start..location.selection.range.end]
        == "Item"));
    assert_eq!(
        defaults
            .iter()
            .map(|location| location.selection.range.start)
            .collect::<HashSet<_>>(),
        [
            TEXT.find("inherited(self)").unwrap(),
            TEXT.rfind("inherited(self)").unwrap()
        ]
        .into()
    );
    let first = identity(named(&signatures, file, TEXT.find("First {}").unwrap()));
    let first_type = TypeId::Struct(NominalType {
        declaration: first,
        arguments: vec![],
        associated_types: Default::default(),
    });
    let boxed = identity(named(&signatures, file, TEXT.find("Box<T>").unwrap()));
    let receiver = TypeId::Struct(NominalType {
        declaration: boxed,
        arguments: vec![first_type],
        associated_types: Default::default(),
    });
    assert_eq!(
        query(&relations, None, Some(receiver)).len(),
        2,
        "trait and empty inherent generic blocks are included when bounds hold"
    );
    let receiver = TypeId::Struct(NominalType {
        declaration: boxed,
        arguments: vec![TypeId::Tuple(vec![])],
        associated_types: Default::default(),
    });
    assert!(
        query(&relations, None, Some(receiver)).is_empty(),
        "header unification alone does not prove Read"
    );
    let concrete = identity(named(
        &signatures,
        file,
        TEXT.find("read(self) -> i32 { 11").unwrap(),
    ));
    let selected = query(&relations, Some(concrete), None);
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].selection.range.start,
        TEXT.find("read(self) -> i32 { 11").unwrap()
    );
}

#[test]
fn unresolved_headers_limits_cancellation_and_foreign_scope_do_not_look_like_empty_coverage() {
    let (_database, mut sources, file, signatures) = setup();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        signatures.implementation_relations(Default::default(), &cancel),
        Err(RelationError::Cancelled)
    ));
    assert!(matches!(
        signatures.implementation_relations(
            RelationLimits {
                implementations: 0,
                ..Default::default()
            },
            &Default::default()
        ),
        Err(RelationError::LimitExceeded)
    ));
    let read = identity(named(&signatures, file, TEXT.find("Read {").unwrap()));
    let (_other_database, _other_sources, _other_file, other) = setup();
    let relations = other
        .implementation_relations(Default::default(), &Default::default())
        .unwrap();
    assert_eq!(
        relations.query(
            &ImplementationTarget {
                declaration: Some(read),
                receiver: None,
                assumptions: Default::default()
            },
            &Default::default()
        ),
        Err(RelationError::ForeignTarget)
    );
    sources
        .set(
            "relations.kgr",
            TEXT.replace("impl Read for First", "impl Missing for First"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let changed = test_analysis()
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(matches!(
        changed.implementation_relations(Default::default(), &Default::default()),
        Err(RelationError::Incomplete)
    ));
    sources
        .set(
            "relations.kgr",
            TEXT.replace(
                "fn inspect(value: First) { value. }",
                "fn inspect(value: First",
            ),
            SourceLayer::Overlay,
        )
        .unwrap();
    let header = test_analysis()
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        matches!(
            header.implementation_relations(Default::default(), &Default::default()),
            Err(RelationError::Incomplete)
        ),
        "a recovered declaration header cannot certify exhaustive coverage"
    );
    assert_eq!(
        query(
            &signatures
                .implementation_relations(Default::default(), &Default::default())
                .unwrap(),
            Some(read),
            None
        )
        .len(),
        3,
        "retained old relations stay immutable"
    );
}

#[test]
fn applied_interface_queries_prove_fixed_bounds_and_retain_only_unfixed_template_assumptions() {
    let text =
        include_str!("../../../../tests/fixtures/analysis/applied-implementation-relations.kgr");
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("applied.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let signatures = test_analysis()
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let relations = signatures
        .implementation_relations(Default::default(), &Default::default())
        .unwrap();
    let take = identity(named(&signatures, file, text.find("Take<T>").unwrap()));
    let first = identity(named(&signatures, file, text.find("First {}").unwrap()));
    let first = TypeId::Struct(NominalType {
        declaration: first,
        arguments: vec![],
        associated_types: Default::default(),
    });
    let applied = TypeId::Trait(NominalType {
        declaration: take,
        arguments: vec![first.clone()],
        associated_types: Default::default(),
    });
    assert_eq!(
        query(&relations, None, Some(applied)).len(),
        2,
        "Pair keeps its own unfixed U: Read while Take<First> fixes and proves T: Read"
    );
    let impossible = TypeId::Trait(NominalType {
        declaration: take,
        arguments: vec![TypeId::Tuple(vec![])],
        associated_types: Default::default(),
    });
    assert!(
        query(&relations, None, Some(impossible)).is_empty(),
        "a fixed concrete argument cannot assume its template bound"
    );
    assert_eq!(
        query(&relations, Some(take), None).len(),
        3,
        "a trait declaration still enumerates every declared template"
    );
    let child = identity(named(&signatures, file, text.find("Child<T>").unwrap()));
    let member = identity(named(&signatures, file, text.find("take(self)").unwrap()));
    let receiver = TypeId::Trait(NominalType {
        declaration: child,
        arguments: vec![first],
        associated_types: Default::default(),
    });
    assert_eq!(
        query(&relations, Some(member), Some(receiver)).len(),
        1,
        "an inherited member must prove the receiver's child interface, not every parent implementation"
    );
}
