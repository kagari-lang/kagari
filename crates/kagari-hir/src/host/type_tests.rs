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
