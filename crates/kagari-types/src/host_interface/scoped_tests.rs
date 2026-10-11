use crate::host_interface::test_support::FingerprintFixture;
use crate::{
    host_interface::{
        HostFunctionDeclaration, HostInterfaceError, host_type_identity, value_type::HostValueType,
    },
    language::binding,
};
use kagari_common::identity::{
    DefinitionKind, mapping::DefinitionMappingError, metadata::scope_record,
};

#[test]
fn scoped_host_signatures_reject_foreign_and_wrong_constructor_identities() {
    let source = HostFunctionDeclaration::new(
        "demo.read",
        vec![],
        HostValueType::Option(
            binding::option_declaration(),
            Box::new(HostValueType::Opaque(host_type_identity("demo.Value"))),
        ),
    );
    let scoped = scope_record(&source, &Default::default()).unwrap();
    assert_eq!(scoped.records().validate(scoped.definitions()), Ok(()));
    let foreign = scope_record(&source, &Default::default()).unwrap();
    assert_eq!(
        foreign.records().validate(scoped.definitions()),
        Err(HostInterfaceError::InvalidDeclaration)
    );

    let HostValueType::Option(option, value) = &scoped.records().return_type else {
        panic!("declared option");
    };
    let HostValueType::Opaque(object) = value.as_ref() else {
        panic!("declared object");
    };
    let mut forged = scoped.records().clone();
    for result in [
        HostValueType::Option(*object, value.clone()),
        HostValueType::Opaque(*option),
        HostValueType::List(*object, Box::new(HostValueType::I32)),
        HostValueType::Vec(*option, Box::new(HostValueType::I32)),
    ] {
        forged.return_type = result;
        assert_eq!(
            forged.validate(scoped.definitions()),
            Err(HostInterfaceError::InvalidDeclaration)
        );
    }
    let mut wrong_source = source;
    let HostValueType::Option(_, value) = &mut wrong_source.return_type else {
        unreachable!();
    };
    let HostValueType::Opaque(id) = value.as_mut() else {
        unreachable!();
    };
    id.path.last_mut().unwrap().kind = DefinitionKind::Function;
    assert_eq!(
        wrong_source.fingerprint_fixture(),
        Err(HostInterfaceError::InvalidDeclaration)
    );
    assert!(matches!(
        scope_record(&wrong_source, &Default::default()),
        Err(DefinitionMappingError::InvalidContract)
    ));
}
