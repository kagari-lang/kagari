use crate::{
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostInterfaceError,
        type_declaration::HostTypeDeclaration, value_type::HostValueType,
    },
    language::binding,
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};

#[test]
fn scoped_host_signatures_reject_foreign_and_wrong_constructor_identities() {
    let context = DefinitionContext::new().unwrap();
    let owner = HostTypeDeclaration::new(&context, "demo.Value").unwrap();
    let source = HostFunctionDeclaration::new(
        &context,
        "demo.read",
        vec![],
        HostValueType::Option(
            context.intern(&binding::option_declaration()).unwrap(),
            Box::new(HostValueType::Opaque(owner.id)),
        ),
    )
    .unwrap();
    let scoped =
        DefinitionMetadata::checked(context.snapshot(), source.clone(), &Default::default())
            .unwrap();
    assert_eq!(scoped.records().validate(scoped.definitions()), Ok(()));
    let foreign = scoped
        .import_into(&DefinitionContext::new().unwrap(), &Default::default())
        .unwrap();
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
    *id = wrong_source.id;
    assert_eq!(
        wrong_source.fingerprint(&context.snapshot()),
        Err(HostInterfaceError::InvalidDeclaration)
    );
    let interface = HostInterface {
        types: vec![owner],
        functions: vec![wrong_source],
        ..Default::default()
    };
    assert_eq!(
        interface.to_bytes(&context.snapshot()),
        Err(HostInterfaceError::InvalidDeclaration)
    );
}
