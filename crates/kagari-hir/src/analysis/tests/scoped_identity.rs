//! Retained identity prefixes, fixed-ID remapping and analysis-local binding ownership.
use crate::analysis::*;
use crate::tests::standard::standard_modules;
use crate::{declarations::DeclarationId, tests::test_analysis};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn checked_analysis_retains_compact_named_and_generic_metadata() {
    use crate::types::semantic::lower_type;
    use kagari_common::identity::mapping::DefinitionMappingError;
    use kagari_types::ty::Ty;

    let source = SourceFile::new(
        "compact.kgr",
        "struct Player { var hp: i32 } fn identity<T>(value: T) -> T { value }",
    );
    let first = crate::analyze_source(&source, standard_modules())
        .expect("installed declaration analysis")
        .into_codegen()
        .unwrap();
    let second = crate::analyze_source(&source, standard_modules())
        .expect("installed declaration analysis")
        .into_codegen()
        .unwrap();
    let function = first
        .typed
        .signatures
        .functions()
        .iter()
        .find(|function| function.name == "identity")
        .unwrap();
    let other = second
        .typed
        .signatures
        .functions()
        .iter()
        .find(|function| function.name == "identity")
        .unwrap();
    let owner = function.generic_params[0].owner;
    assert_ne!(owner, other.generic_params[0].owner);
    assert!(second.definitions().resolve(owner).is_err());
    assert_eq!(
        first.definitions().resolve(owner).unwrap().to_path(),
        second
            .definitions()
            .resolve(other.generic_params[0].owner)
            .unwrap()
            .to_path()
    );
    assert_eq!(
        lower_type(&function.return_type),
        Ty::Parameter { owner, position: 0 }
    );
    let field = first
        .declarations
        .iter()
        .find(|declaration| declaration.name == "hp")
        .unwrap();
    let DeclarationId::Definition(field) = field.id else {
        panic!("field definition");
    };
    let field_path = first.definitions().resolve(field).unwrap().to_path();
    assert_eq!(
        field_path
            .path
            .iter()
            .map(|segment| segment.name.as_str())
            .collect::<Vec<_>>(),
        ["Player", "hp"]
    );
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        first.to_unverified(&cancelled),
        Err(DefinitionMappingError::Cancelled)
    ));
    let authoring = first.to_unverified(&Default::default()).unwrap();
    assert_eq!(
        authoring
            .records()
            .typed
            .signatures
            .functions()
            .iter()
            .find(|function| function.name == "identity")
            .unwrap()
            .generic_params[0]
            .owner,
        owner
    );
    drop(source);
    drop(second);
    assert_eq!(
        first.definitions().resolve(field).unwrap().to_path(),
        field_path
    );
}

#[test]
fn independently_constructed_hir_types_observe_conversion_bounds() {
    use crate::types::TypeId;
    use kagari_common::identity::{mapping::DefinitionMappingError, scoped::DefinitionReferences};
    let mut ty = TypeId::Builtin(kagari_types::scalar::BuiltinType::I32);
    for _ in 0..65 {
        ty = TypeId::Tuple(vec![ty]);
    }
    assert!(matches!(
        ty.visit_definition_ids(&mut |_| Ok(()), &Default::default()),
        Err(DefinitionMappingError::LimitExceeded)
    ));
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        ty.visit_definition_ids(&mut |_| Ok(()), &cancelled),
        Err(DefinitionMappingError::Cancelled)
    ));
}

#[test]
fn published_cache_scopes_retain_prefixes_and_reject_foreign_queries() {
    use kagari_common::identity::scoped::DefinitionReferences;
    let text = "struct Player { var hp: i32 } fn identity<T>(value: T) -> T { value } fn read(p: Player) -> i32 { p.hp }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache-scopes.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let first = snapshot(&mut database, &sources);
    let analysis = first.file(file).unwrap().clone();
    let signature = first.signature_snapshot().file(file).unwrap().clone();
    let declaration = analysis
        .result()
        .facts()
        .declarations
        .iter()
        .find(|d| d.name == "read")
        .unwrap();
    let DeclarationId::Definition(owner) = declaration.id else {
        panic!("function owner");
    };
    let path = analysis.definitions().resolve(owner).unwrap().to_path();
    let body = database
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    fn validate<T: DefinitionReferences>(records: &T, definitions: &DefinitionTable) {
        records
            .visit_definition_ids(
                &mut |id| {
                    definitions.resolve(*id)?;
                    Ok(())
                },
                &Default::default(),
            )
            .unwrap();
    }
    validate(analysis.result(), analysis.definitions());
    validate(signature.signatures().as_ref(), signature.definitions());
    validate(body.type_table(), body.definitions());
    assert!(Arc::ptr_eq(analysis.signatures(), signature.signatures()));
    let mut independent = test_analysis();
    assert!(
        independent
            .body(sources.snapshot(), &owner, &Default::default())
            .is_err()
    );
    let independent_declarations = independent
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let independent_owner = independent_declarations
        .file(file)
        .unwrap()
        .declarations()
        .definitions()
        .lookup(&path)
        .unwrap();
    let independent_body = independent
        .body(sources.snapshot(), &independent_owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_ne!(*independent_body.owner(), owner);
    sources
        .set(
            "cache-scopes.kgr",
            format!("{text} fn added() {{}}"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = snapshot(&mut database, &sources);
    assert!(second.definitions().resolve(owner).is_ok());
    drop(database);
    drop(first);
    drop(second);
    drop(sources);
    assert_eq!(
        body.definitions().resolve(*body.owner()).unwrap().to_path(),
        path
    );
    assert_eq!(
        analysis.definitions().resolve(owner).unwrap().to_path(),
        path
    );
    validate(signature.signatures().as_ref(), signature.definitions());
}

#[test]
fn definitions_are_module_owned_but_bindings_are_analysis_and_body_owned() {
    let text = "fn same(value: i32) -> i32 { val n = value; n } fn use_it() -> i32 { same(1) }";
    let module = |name: &str| ModuleIdentity {
        package: PackageId("game".into()),
        path: vec![name.into()],
    };
    let mut sources = SourceDatabase::default();
    let a = sources.bind_module("a.kgr", module("a")).unwrap();
    let _b = sources.bind_module("b.kgr", module("b")).unwrap();
    sources
        .set("a.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    sources
        .set("b.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = snapshot(&mut db, &sources);

    let _definitions = first
        .file(a)
        .unwrap()
        .result()
        .facts()
        .declarations
        .definitions();

    let _independent = snapshot(&mut test_analysis(), &sources);

    let _destination = DefinitionContext::new().unwrap();

    let _unchanged = snapshot(&mut db, &sources);

    let edited = text.replace("val n = value", "val n = value + 1");
    sources
        .set("a.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let _second = snapshot(&mut db, &sources);

    // Unchanged source and declarations reuse the same semantic identities.

    let _third = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();

    sources.bind_module("a.kgr", module("renamed")).unwrap();
    let _rebound = snapshot(&mut db, &sources);
}
