use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_runtime::{
    Runtime,
    host::{HostFunction, HostTypeRegistration},
    value::Value,
};
use kagari_types::host_interface::{
    HostInterface,
    type_declaration::{
        HostMethodDeclaration, HostTraitImplementationDeclaration, HostTraitMethodBinding,
        HostTypeDeclaration,
    },
    value_type::HostValueType,
};

fn main() {
    let context = DefinitionContext::new().unwrap();
    let mut counter = HostTypeDeclaration::new(&context, "demo.Counter").unwrap();
    let method =
        HostMethodDeclaration::new(&context, counter.id, "read", vec![], HostValueType::I32)
            .unwrap();
    counter.methods.push(method.clone());

    let module = context
        .intern_root(&ModuleIdentity {
            package: PackageId("demo".into()),
            path: vec!["api".into()],
        })
        .unwrap();
    let trait_id = context
        .intern_child(module, DefinitionKind::Trait, "Readable", 0)
        .unwrap();
    let trait_method = context
        .intern_child(trait_id, DefinitionKind::Method, "get", 0)
        .unwrap();
    counter
        .trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id,
            }],
        ));

    let offline = HostInterface {
        paths: vec![],
        types: vec![counter.clone()],
        functions: vec![],
    };
    let offline =
        HostInterface::from_bytes(&offline.to_bytes(&context.snapshot()).unwrap()).unwrap();
    let counter =
        DefinitionMetadata::checked(context.snapshot(), counter, &Default::default()).unwrap();
    let mut runtime = Runtime::default();
    runtime
        .register_host_type(HostTypeRegistration::new(counter.clone(), "Counter"))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(&counter, method.id, |_, _| Ok(Value::I32(42))).unwrap(),
        )
        .unwrap();
    runtime
        .host()
        .link_interface(offline.records(), offline.definitions())
        .unwrap();
    println!(
        "linked {} trait implementation with {} method",
        offline.records().types[0].trait_implementations.len(),
        offline.records().types[0].trait_implementations[0]
            .methods
            .len()
    );
}
