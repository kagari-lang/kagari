use super::*;
use crate::{
    callable::CallableSignature, native::NativeBinding, resolver::resolved::ResolvedName,
    typeck::FunctionImplementation,
};
use kagari_types::{
    declaration::{host::HostPassingStyle, names::NameNamespace},
    host_interface::{
        HostParameter,
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration},
    },
};

#[test]
fn offline_type_queries_preserve_member_contracts_and_reject_stale_ids() {
    let context = DefinitionContext::new().unwrap();
    let mut declaration = HostTypeDeclaration::new(&context, "model.Player").unwrap();
    declaration.fields.push(
        HostFieldDeclaration::new(&context, declaration.id, "score", HostValueType::I32).unwrap(),
    );
    declaration.fields[0].documentation = "Current score".into();
    let interface = metadata(
        &context,
        HostInterface {
            paths: vec![],
            types: vec![declaration.clone()],
            functions: vec![],
        },
    );
    let old = HostDeclarations::new(
        HostInterface::from_bytes(
            &interface
                .records()
                .to_bytes(interface.definitions())
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let id = old.resolve_type("model::Player").unwrap();
    let query = context
        .snapshot()
        .resolve(declaration.id)
        .unwrap()
        .to_path();
    let decoded_id = old.definitions().lookup(&query).unwrap();
    assert_eq!(old.nominal_type(&decoded_id), Some(id));
    assert_eq!(
        old.type_declaration(id).unwrap().fields[0].documentation,
        "Current score"
    );
    assert!(old.module("model").is_some());
    declaration.fields[0].ty = HostValueType::I64;
    let new = HostDeclarations::new(metadata(
        &context,
        HostInterface {
            paths: vec![],
            types: vec![declaration],
            functions: vec![],
        },
    ))
    .unwrap();
    assert!(new.type_declaration(id).is_none());
    assert_eq!(
        new.type_declaration(new.resolve_type("model::Player").unwrap())
            .unwrap()
            .fields[0]
            .ty,
        HostValueType::I64
    );
    assert_eq!(
        old.type_declaration(id).unwrap().fields[0].ty,
        HostValueType::I32
    );
}

pub(super) fn declaration(context: &DefinitionContext) -> HostFunctionDeclaration {
    let mut declaration = HostFunctionDeclaration::new(
        context,
        "demo.echo",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    )
    .unwrap();
    declaration.documentation = "Echo an integer".into();
    declaration
}

#[test]
fn checked_host_callables_retain_provider_contracts_and_reject_other_inputs() {
    let context = DefinitionContext::new().unwrap();
    let mut declaration = declaration(&context);
    declaration.params[0].ty = HostValueType::String;
    declaration.params[0].passing = HostPassingStyle::SharedBorrow;

    declaration.effects.may_call_host_services = true;
    declaration.effects.may_trap = true;
    let interface = metadata(
        &context,
        HostInterface {
            functions: vec![declaration.clone()],
            ..Default::default()
        },
    );
    let original = HostDeclarations::new(interface.clone()).unwrap();
    let other = HostDeclarations::new(interface).unwrap();
    let id = original.resolve("demo::echo").unwrap();
    let callable = original.callable(id).unwrap();
    assert_eq!(callable.contract(), &declaration);
    assert_eq!(callable.name(), "demo.echo");
    assert_eq!(
        callable.implementation(),
        FunctionImplementation::Native(NativeBinding::Host(id))
    );
    assert_eq!(
        callable.parameters().collect::<Vec<_>>(),
        [("value", &TypeId::Builtin(BuiltinType::String))]
    );
    assert_eq!(callable.return_type(), &TypeId::Builtin(BuiltinType::I32));
    assert!(callable.generic_params().is_empty());
    assert!(callable.bounds().is_none());
    assert!(other.callable(id).is_none());
    let other_id = other.resolve("demo::echo").unwrap();
    assert_ne!(id, other_id);
    assert_eq!(other.callable(other_id).unwrap().contract(), &declaration);
    assert_eq!(original.callable(id).unwrap().contract(), &declaration);
}

#[test]
fn host_catalog_rejects_ambiguous_or_unspellable_paths() {
    let context = DefinitionContext::new().unwrap();
    for name in ["std.echo", "demo::echo", "demo.2bad"] {
        assert!(
            HostDeclarations::new(metadata(
                &context,
                HostInterface {
                    paths: vec![],
                    types: Vec::new(),
                    functions: vec![
                        HostFunctionDeclaration::new(&context, name, vec![], HostValueType::Unit)
                            .unwrap()
                    ]
                }
            ))
            .is_err()
        );
    }
    let interface = metadata(
        &context,
        HostInterface {
            paths: vec![],
            types: vec![],
            functions: vec![
                declaration(&context),
                HostFunctionDeclaration::new(
                    &context,
                    "demo.echo.child",
                    vec![],
                    HostValueType::Unit,
                )
                .unwrap(),
            ],
        },
    );
    let declarations = HostDeclarations::new(interface).unwrap();
    assert!(matches!(
        declarations.resolve_name("demo::echo", NameNamespace::Value),
        Some(ResolvedName::HostFunction(_))
    ));
    assert!(matches!(
        declarations.resolve_name("demo::echo", NameNamespace::Type),
        Some(ResolvedName::HostModule(_))
    ));
    // ABI symbols retain their independent uniqueness rule.
    assert!(
        HostDeclarations::new(metadata(
            &context,
            HostInterface {
                paths: vec![],
                types: vec![],
                functions: vec![declaration(&context), declaration(&context)]
            }
        ))
        .is_err()
    );
}

/// Retains the identity table for an editable Host test input. Semantic validation
/// remains with HostDeclarations, including fixtures intentionally made invalid.
pub(super) fn metadata(
    context: &DefinitionContext,
    interface: HostInterface,
) -> DefinitionMetadata<HostInterface> {
    DefinitionMetadata::checked(context.snapshot(), interface, &Default::default()).unwrap()
}
