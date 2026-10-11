//! Retained identity prefixes, fixed-ID remapping and analysis-local binding ownership.
use super::*;
use crate::{declarations::DeclarationId, tests::test_analysis};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::catalog as foundation_catalog;

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
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
    let b = sources.bind_module("b.kgr", module("b")).unwrap();
    sources
        .set("a.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    sources
        .set("b.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = snapshot(&mut db, &sources);
    let function_a = first
        .file(a)
        .unwrap()
        .definition_at(text.rfind("same(1)").unwrap())
        .unwrap()
        .clone();
    let function_b = first
        .file(b)
        .unwrap()
        .definition_at(text.rfind("same(1)").unwrap())
        .unwrap();
    assert_ne!(function_a.id, function_b.id);
    let binding_a = first
        .file(a)
        .unwrap()
        .definition_at(text.find("n }").unwrap())
        .unwrap()
        .clone();
    let DeclarationId::Binding(binding) = &binding_a.id else {
        panic!("local binding")
    };
    let DeclarationId::Definition(owner) = &function_a.id else {
        panic!("function definition")
    };
    let definitions = first
        .file(a)
        .unwrap()
        .result()
        .facts()
        .declarations
        .definitions();
    assert_eq!(binding.body, *owner);
    assert!(definitions.resolve(binding.body).is_ok());
    let independent = snapshot(&mut test_analysis(), &sources);
    assert!(independent.definitions().resolve(binding.body).is_err());
    let binding_metadata = DefinitionMetadata::checked(
        definitions.clone(),
        binding_a.id.clone(),
        &Default::default(),
    )
    .unwrap();
    assert!(
        DefinitionMetadata::checked(
            independent.definitions().clone(),
            binding_a.id.clone(),
            &Default::default(),
        )
        .is_err()
    );
    let destination = DefinitionContext::new().unwrap();
    let imported = binding_metadata
        .import_into(&destination, &Default::default())
        .unwrap();
    let DeclarationId::Binding(imported_binding) = imported.records() else {
        panic!("imported local binding");
    };
    assert_ne!(imported_binding.body, binding.body);
    assert_eq!(imported_binding.analysis, binding.analysis);
    assert_eq!(
        imported
            .definitions()
            .resolve(imported_binding.body)
            .unwrap()
            .to_path(),
        definitions.resolve(binding.body).unwrap().to_path()
    );
    let restored = imported
        .import_into(
            first
                .file(a)
                .unwrap()
                .result()
                .facts()
                .declarations
                .context(),
            &Default::default(),
        )
        .unwrap();
    assert_eq!(restored.records(), &binding_a.id);
    assert_eq!(
        first.definitions().resolve(binding.body).unwrap().to_path(),
        definitions.resolve(*owner).unwrap().to_path()
    );
    assert_eq!(first.declaration(&function_b.id).unwrap().location.file, b);
    let unchanged = snapshot(&mut db, &sources);
    assert_eq!(unchanged.declaration(&binding_a.id), Some(&binding_a));

    let edited = text.replace("val n = value", "val n = value + 1");
    sources
        .set("a.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    let new_function = second.declaration(&function_a.id).unwrap();
    assert_ne!(new_function.location.revision, function_a.location.revision);
    assert_eq!(new_function.location.file, function_a.location.file);
    assert!(second.declaration(&binding_a.id).is_none());
    assert_eq!(first.declaration(&binding_a.id), Some(&binding_a));
    let new_binding = second
        .file(a)
        .unwrap()
        .definition_at(edited.find("n }").unwrap())
        .unwrap();
    assert_ne!(new_binding.id, binding_a.id);

    // Unchanged source and declarations reuse the same semantic identities.

    let third = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(third.declaration(&new_binding.id), Some(new_binding));
    assert!(third.declaration(&function_a.id).is_some());
    sources.bind_module("a.kgr", module("renamed")).unwrap();
    let rebound = snapshot(&mut db, &sources);
    assert!(rebound.declaration(&function_a.id).is_none());
    assert!(rebound.declaration(&function_b.id).is_some());
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
    let first = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_codegen()
        .unwrap();
    let second = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis")
        .into_codegen()
        .unwrap();
    let function = first
        .typed
        .functions
        .iter()
        .find(|function| function.name == "identity")
        .unwrap();
    let other = second
        .typed
        .functions
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
            .functions
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
