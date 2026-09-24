use kagari_common::{
    host_interface::{
        HostInterface, HostMethodDeclaration, HostTraitImplementationDeclaration,
        HostTraitMethodBinding, HostTypeDeclaration, HostValueType,
    },
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId},
};
use kagari_runtime::{HostTypeRegistration, Runtime, host::HostFunction, value::Value};

fn main() {
    let mut counter = HostTypeDeclaration::new("demo.Counter");
    let method = HostMethodDeclaration::new(&counter.id, "read", vec![], HostValueType::I32);
    counter.methods.push(method.clone());

    let trait_id = DefinitionId {
        module: ModuleIdentity {
            package: PackageId("demo".into()),
            path: vec!["api".into()],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Readable".into(),
            occurrence: 0,
        }],
    };
    let mut trait_method = trait_id.clone();
    trait_method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "get".into(),
        occurrence: 0,
    });
    counter
        .trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id.clone(),
            }],
        ));

    let offline = HostInterface {
        paths: vec![],
        types: vec![counter.clone()],
        functions: vec![],
    };
    let offline = HostInterface::from_bytes(&offline.to_bytes().unwrap()).unwrap();
    let mut runtime = Runtime::default();
    runtime
        .register_host_type(HostTypeRegistration::new(counter.clone(), "Counter"))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(&counter, &method.id, |_, _| Ok(Value::I32(42))).unwrap(),
        )
        .unwrap();
    runtime.host().link_interface(&offline).unwrap();
    println!(
        "linked {} trait implementation with {} method",
        offline.types[0].trait_implementations.len(),
        offline.types[0].trait_implementations[0].methods.len()
    );
}
