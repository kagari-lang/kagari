use super::*;
use crate::{LanguageFeatureProfile, analysis::AnalysisDatabase};
use kagari_common::{
    host_interface::{HostParameter, HostPassingStyle},
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

fn interface() -> HostInterface {
    let left = HostTypeDeclaration::new("left.Item");
    let right = HostTypeDeclaration::new("right.Item");
    let make =
        HostFunctionDeclaration::new("left.make", vec![], HostValueType::Opaque(left.id.clone()));
    let take = HostFunctionDeclaration::new(
        "left.take",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::Opaque(left.id.clone()),
            passing: HostPassingStyle::SharedBorrow,
        }],
        HostValueType::I32,
    );
    HostInterface {
        field_paths: vec![],
        types: vec![left, right],
        functions: vec![make, take],
    }
}

#[test]
fn host_methods_keep_checked_receiver_targets_and_offline_documentation() {
    let mut declarations = interface();
    let owner = &mut declarations.types[0];
    let mut method = kagari_common::host_interface::HostMethodDeclaration::new(
        &owner.id,
        "add",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    method.documentation = "Add to the host counter".into();
    let identity = method.id.clone();
    owner.methods.push(method);
    for argument in ["2", "true", "", "2, 3"] {
        let text = format!("use left as api; fn main() -> i32 {{ api::make().add({argument}) }}");
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("mem://method", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = AnalysisDatabase::default();
        db.set_host_declarations(HostDeclarations::new(declarations.clone()).unwrap());
        let snapshot = db
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile {
                    allow_host_calls: true,
                    ..Default::default()
                },
                &Default::default(),
            )
            .unwrap();
        let file = snapshot.file(root).unwrap();
        assert_eq!(
            file.result().diagnostics().is_empty(),
            argument == "2",
            "{:?}",
            file.result().diagnostics()
        );
        let target = file.host_function_at(text.find("add(").unwrap()).unwrap();
        if argument.is_empty() {
            assert!(
                file.result()
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| matches!(
                        diagnostic.kind,
                        kagari_common::DiagnosticKind::CallArityMismatch {
                            expected: 1,
                            found: 0,
                            ..
                        }
                    ))
            );
        }
        assert_eq!(target.id, identity);
        assert_eq!(target.documentation, "Add to the host counter");
        assert_eq!(
            target.params[0].ty,
            HostValueType::Opaque(declarations.types[0].id.clone())
        );
        assert_eq!(
            snapshot.check_program(root, &Default::default()).is_ok(),
            argument == "2"
        );
    }
}

#[test]
fn host_types_resolve_through_facades_and_keep_revision_owned_query_facts() {
    let mut sources = SourceDatabase::default();
    sources
        .bind_module(
            "mem://facade",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["facade".into()],
            },
        )
        .unwrap();
    sources
        .set(
            "mem://facade",
            "pub use left::Item as Object; pub use left as api;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let text = "use pkg::facade::{Object as Item, api}; use pkg::facade as facade; fn pass(value: facade::Object) -> api::Item { value } fn main() -> i32 { val value: Item = left::make(); left::take(pass(value)) }";
    let root = sources
        .set("mem://root", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    db.set_host_declarations(HostDeclarations::new(interface()).unwrap());
    let profile = LanguageFeatureProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let old = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let file = old.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    old.check_program(root, &Default::default()).unwrap();
    let id = interface().types[0].id.clone();
    for spelling in [
        "Object as Item",
        "facade::Object",
        "api::Item",
        "Item =",
        "value))",
    ] {
        assert_eq!(
            file.host_type_at(text.find(spelling).unwrap()).unwrap().id,
            id,
            "{spelling}"
        );
    }
    let mut changed = interface();
    changed.types[0].documentation = "updated".into();
    changed.functions[0].return_type = HostValueType::Opaque(changed.types[1].id.clone());
    db.set_host_declarations(HostDeclarations::new(changed).unwrap());
    let new = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    assert!(!new.file(root).unwrap().result().diagnostics().is_empty());
    let offset = text.find("facade::Object").unwrap();
    assert!(file.host_type_at(offset).unwrap().documentation.is_empty());
    assert_eq!(
        new.file(root)
            .unwrap()
            .host_type_at(offset)
            .unwrap()
            .documentation,
        "updated"
    );
    assert_eq!(
        file.type_at(text.find("left::make()").unwrap()),
        Some(TypeId::Host(id))
    );
}

#[test]
fn host_type_errors_preserve_other_functions_and_do_not_enable_equality_or_constructors() {
    for broken in [
        "use right::Item; fn bad(value: Item) -> i32 { left::take(value) }",
        "use left::Item; fn bad(a: Item, b: Item) -> bool { a == b }",
        "use left::Item; fn bad() { val value = Item {}; }",
        "use left::Item; fn bad(value: Item<i32>) {}",
        "fn left() {} fn bad(value: left::Item) {}",
        "use missing as left; fn bad(value: left::Item) {}",
        "fn bad(value: left::Item) { value. }",
    ] {
        let mut sources = SourceDatabase::default();
        let text = format!("{broken} fn good() -> i32 {{ 7 }}");
        let root = sources
            .set("mem://root", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = AnalysisDatabase::default();
        db.set_host_declarations(HostDeclarations::new(interface()).unwrap());
        let snapshot = db
            .snapshot(
                sources.snapshot(),
                LanguageFeatureProfile {
                    allow_host_calls: true,
                    ..Default::default()
                },
                &Default::default(),
            )
            .unwrap();
        let file = snapshot.file(root).unwrap();
        assert!(!file.result().diagnostics().is_empty(), "{broken}");
        if let Some(offset) = text.find("value.") {
            assert_eq!(
                file.member_receiver_type(offset + "value.".len()),
                Some(TypeId::Host(interface().types[0].id.clone()))
            );
        }
        if let Some(offset) = text.find("Item<i32>") {
            assert_eq!(
                file.host_type_at(offset).unwrap().id,
                interface().types[0].id
            );
        }
        assert_eq!(
            file.type_at(text.rfind('7').unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(snapshot.check_program(root, &Default::default()).is_err());
    }
}

#[test]
fn field_reads_keep_offline_facts_and_remap_root_ids_after_neighbor_edits() {
    use kagari_common::host_interface::{
        HostFieldDeclaration, HostFieldPathDeclaration, HostTypeOwnership, PathAccess,
    };
    let mut declarations = interface();
    let owner = &mut declarations.types[0];
    owner.ownership = HostTypeOwnership::HostRoot;
    owner.path_access = PathAccess::ReadOnly;
    let mut field = HostFieldDeclaration::new(&owner.id, "score", HostValueType::I32);
    field.path_access = PathAccess::ReadOnly;
    field.documentation = "Offline score documentation".into();
    owner.fields.push(field.clone());
    let path = HostFieldPathDeclaration {
        root: owner.id.clone(),
        fields: vec![field.id.clone()],
        access: PathAccess::ReadOnly,
        schema_epoch: 2,
        capabilities: Default::default(),
    };
    declarations.field_paths.push(path.clone());
    let mut sources = SourceDatabase::default();
    let text = "fn neighbor() -> i32 { 1 } fn read() -> i32 { left::make().score }";
    let root = sources
        .set("mem://field", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    db.set_host_declarations(HostDeclarations::new(declarations.clone()).unwrap());
    let profile = LanguageFeatureProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let old = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let old_file = old.file(root).unwrap();
    assert!(old_file.result().diagnostics().is_empty());
    assert_eq!(
        old_file.host_field_at(text.find("score").unwrap()).unwrap(),
        &field
    );
    let changed = text.replace("{ 1 }", "{ 10 + 20 }");
    sources
        .set("mem://field", changed.clone(), SourceLayer::Base)
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let new_file = new.file(root).unwrap();
    assert_eq!(new_file.result().facts().typed.reused_bodies, 1);
    let path_fact = |file: &crate::analysis::FileAnalysis| {
        let facts = file.result().facts();
        facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, _)| facts.typed.type_table.host_path(id).cloned())
            .unwrap()
    };
    let old_path = path_fact(old_file);
    let new_path = path_fact(new_file);
    assert_ne!(old_path.root, new_path.root);
    assert_eq!(old_path.declaration, new_path.declaration);
    new.check_program(root, &Default::default()).unwrap();
    for ambiguous in [false, true] {
        let mut invalid = declarations.clone();
        if ambiguous {
            let mut other = path.clone();
            other.schema_epoch = 3;
            invalid.field_paths.push(other);
        } else {
            invalid.field_paths.clear();
        }
        db.set_host_declarations(HostDeclarations::new(invalid).unwrap());
        let snapshot = db
            .snapshot(sources.snapshot(), profile, &Default::default())
            .unwrap();
        let file = snapshot.file(root).unwrap();
        assert!(file.result().diagnostics().iter().any(|d| matches!(
            d.kind,
            kagari_common::DiagnosticKind::InvalidHostPath { .. }
        )));
        assert_eq!(
            file.host_field_at(changed.find("score").unwrap()).unwrap(),
            &field
        );
        assert!(snapshot.check_program(root, &Default::default()).is_err());
    }
    db.set_host_declarations(HostDeclarations::new(declarations).unwrap());
    let incomplete = "fn bad() { left::make(). } fn good() -> i32 { 7 }";
    sources
        .set("mem://field", incomplete.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let file = snapshot.file(root).unwrap();
    assert!(!file.result().diagnostics().is_empty());
    assert_eq!(
        file.type_at(incomplete.find("left::make()").unwrap()),
        Some(TypeId::Host(path.root))
    );
    assert_eq!(
        file.type_at(incomplete.rfind('7').unwrap()),
        Some(TypeId::Builtin(crate::types::BuiltinType::I32))
    );
}

#[test]
fn field_write_facts_survive_body_reuse_and_readonly_paths_are_diagnostics() {
    use kagari_common::host_interface::{
        HostFieldDeclaration, HostFieldPathDeclaration, HostTypeOwnership, PathAccess,
    };
    let mut declarations = interface();
    let owner = &mut declarations.types[0];
    owner.ownership = HostTypeOwnership::HostRoot;
    owner.path_access = PathAccess::ReadWrite;
    let mut field = HostFieldDeclaration::new(&owner.id, "score", HostValueType::I32);
    field.writable = true;
    field.path_access = PathAccess::ReadWrite;
    owner.fields.push(field.clone());
    declarations.field_paths.push(HostFieldPathDeclaration {
        root: owner.id.clone(),
        fields: vec![field.id.clone()],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
        capabilities: Default::default(),
    });
    let mut sources = SourceDatabase::default();
    let text = "fn neighbor() -> i32 { 1 } fn update(target: left::Item) { target.score += 2; }";
    let root = sources
        .set("mem://write", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    db.set_host_declarations(HostDeclarations::new(declarations.clone()).unwrap());
    let profile = LanguageFeatureProfile {
        allow_path_mutation: true,
        ..Default::default()
    };
    let old = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    old.check_program(root, &Default::default()).unwrap();
    assert_eq!(
        old.file(root)
            .unwrap()
            .host_field_at(text.find("score").unwrap()),
        Some(&field)
    );
    let old_table = &old.file(root).unwrap().result().facts().typed.type_table;
    let old_place = old_table.host_write_places().next().unwrap();
    let old_path = old_table.host_place_path(old_place).unwrap().clone();
    sources
        .set(
            "mem://write",
            text.replace("{ 1 }", "{ 10 + 20 }"),
            SourceLayer::Base,
        )
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let file = new.file(root).unwrap();
    assert_eq!(file.result().facts().typed.reused_bodies, 1);
    let table = &file.result().facts().typed.type_table;
    let place = table.host_write_places().next().unwrap();
    let path = table.host_place_path(place).unwrap();
    assert_ne!(old_path.root, path.root);
    assert_eq!(old_path.declaration, path.declaration);
    let changed = text.replace("{ 1 }", "{ 10 + 20 }");
    assert_eq!(
        file.host_field_at(changed.find("score").unwrap()),
        Some(&field)
    );
    assert_eq!(file.host_field_at(changed.find("neighbor").unwrap()), None);
    new.check_program(root, &Default::default()).unwrap();
    declarations.field_paths[0].access = PathAccess::ReadOnly;
    db.set_host_declarations(HostDeclarations::new(declarations).unwrap());
    let readonly = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    assert!(
        readonly
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::InvalidHostPath { .. }
            ))
    );
    assert!(readonly.check_program(root, &Default::default()).is_err());
    assert_eq!(
        readonly
            .file(root)
            .unwrap()
            .host_field_at(changed.find("score").unwrap()),
        Some(&field)
    );
    assert_eq!(
        old.file(root)
            .unwrap()
            .host_field_at(text.find("score").unwrap()),
        Some(&field)
    );
}
