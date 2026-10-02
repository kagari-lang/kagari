use super::*;
use crate::{
    analysis::AnalysisDatabase,
    callable::CallableSignature,
    host::{HostDeclarations, tests::declaration},
    native::NativeBinding,
    typeck::FunctionImplementation,
};
use kagari_common::{
    host_interface::{
        type_declaration::{HostFieldDeclaration, HostMethodDeclaration, HostTypeDeclaration},
        value_type::HostValueType,
    },
    source_database::{SourceDatabase, SourceLayer},
};

fn location(uri: &str) -> HostSourceLocation {
    HostSourceLocation::new(uri, Span::new(3, 7)).unwrap()
}

#[test]
fn origins_are_optional_and_lexically_checked_without_opening_documents() {
    for (uri, range) in [
        ("", Span::new(0, 1)),
        ("relative.rs", Span::new(0, 1)),
        ("file:///bad%xx.rs", Span::new(0, 1)),
        ("virtual://", Span::new(0, 1)),
        ("1virtual://declaration", Span::new(0, 1)),
        ("virtual://bad\0file", Span::new(0, 1)),
        ("virtual://bad file", Span::new(0, 1)),
        ("virtual://good", Span::new(4, 2)),
    ] {
        assert!(HostSourceLocation::new(uri, range).is_err(), "{uri:?}");
    }
    let rust = location("file:///not-installed/provider.rs");
    let declaration_location = location("kagari-host://declarations/demo.kgr");
    let mut origin = HostDeclarationOrigin::default();
    assert_eq!(origin.preferred(), None);
    origin.rust = Some(rust.clone());
    assert_eq!(origin.preferred(), Some(&rust));
    origin.declaration = Some(declaration_location.clone());
    assert_eq!(origin.preferred(), Some(&declaration_location));
    assert_eq!(origin.rust, Some(rust));
    assert_eq!(declaration_location.range(), Span::new(3, 7));

    let declaration = declaration();
    let hosts = HostDeclarations::new(HostInterface {
        functions: vec![declaration.clone()],
        ..Default::default()
    })
    .unwrap();
    assert!(hosts.origin(&declaration.id).is_none());
    assert!(
        hosts
            .callable(hosts.resolve("demo::echo").unwrap())
            .unwrap()
            .origin()
            .is_none()
    );
}

#[test]
fn installation_checks_origin_identities_including_expanded_methods() {
    let mut owner = HostTypeDeclaration::new("demo.Counter");
    owner.fields.push(HostFieldDeclaration::new(
        &owner.id,
        "value",
        HostValueType::I32,
    ));
    owner.methods.push(HostMethodDeclaration::new(
        &owner.id,
        "read",
        vec![],
        HostValueType::I32,
    ));
    let origin = HostDeclarationOrigin {
        declaration: Some(location("kagari-host://declarations/counter.kgr")),
        rust: None,
    };
    let mut input = HostInput::from(HostInterface {
        types: vec![owner.clone()],
        ..Default::default()
    });
    for id in [&owner.id, &owner.fields[0].id, &owner.methods[0].id] {
        input.origins.insert(id.clone(), origin.clone());
    }
    let hosts = HostDeclarations::new(input.clone()).unwrap();
    for id in input.origins.keys() {
        assert_eq!(hosts.origin(id), Some(&origin));
    }
    let method = hosts
        .callable(hosts.method(&owner.id, "read").unwrap())
        .unwrap();
    assert_eq!(method.origin(), Some(&origin));
    assert_eq!(method.contract().id, owner.methods[0].id);

    input.origins.insert(declaration().id, origin);
    assert!(HostDeclarations::new(input).is_err());
    let input = HostInput {
        interface: HostInterface {
            types: vec![owner.clone()],
            ..Default::default()
        },
        origins: HashMap::from([(owner.id, HostDeclarationOrigin::default())]),
    };
    assert!(HostDeclarations::new(input).is_err());
}

#[test]
fn origin_changes_are_snapshot_owned_and_never_change_native_authority() {
    let declaration = declaration();
    let origin = HostDeclarationOrigin {
        declaration: Some(location("kagari://std/math.kgr")),
        rust: Some(location("file:///provider.rs")),
    };
    let mut input = HostInput {
        interface: HostInterface {
            functions: vec![declaration.clone()],
            ..Default::default()
        },
        origins: HashMap::from([(declaration.id.clone(), origin.clone())]),
    };
    let old = HostDeclarations::new(input.clone()).unwrap();
    let id = old.resolve("demo::echo").unwrap();
    let callable = old.callable(id).unwrap();
    assert_eq!(
        callable.implementation(),
        FunctionImplementation::Native(NativeBinding::Host(id))
    );
    assert_eq!(callable.contract(), &declaration);
    assert_eq!(callable.origin(), Some(&origin));
    assert!(old.resolve("std::math::echo").is_none());

    let text = "use demo::echo as invoke; fn main()->i32 { invoke(1) }";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("origin-query.kgr", text.into(), SourceLayer::Base)
        .unwrap();

    let mut database = AnalysisDatabase::default();
    database.set_host_declarations(old.clone());
    let first = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let first_file = first.file(root).unwrap();
    assert!(
        first_file.result().diagnostics().is_empty(),
        "{:?}",
        first_file.result().diagnostics()
    );
    for offset in [text.find("echo").unwrap(), text.rfind("invoke").unwrap()] {
        assert_eq!(first_file.host_origin_at(offset), Some(&origin));
    }
    assert_eq!(
        first_file.host_origin_at(text.find("fn main").unwrap()),
        None
    );

    let changed = HostDeclarationOrigin {
        declaration: Some(location("kagari-host://v2/demo.kgr")),
        rust: None,
    };
    input
        .origins
        .insert(declaration.id.clone(), changed.clone());
    let new = HostDeclarations::new(input).unwrap();
    assert!(new.callable(id).is_none());
    database.set_host_declarations(new);
    let second = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let offset = text.rfind("invoke").unwrap();
    assert_ne!(first.host_revision(), second.host_revision());
    assert_eq!(
        second.file(root).unwrap().host_origin_at(offset),
        Some(&changed)
    );
    assert_eq!(first_file.host_origin_at(offset), Some(&origin));
    assert_eq!(old.callable(id).unwrap().contract(), &declaration);
    assert_eq!(
        first_file.call_signature_at(offset),
        second.file(root).unwrap().call_signature_at(offset)
    );
}
