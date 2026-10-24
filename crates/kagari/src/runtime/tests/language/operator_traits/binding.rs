use crate::engine::KagariEngine;
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::identity::DefinitionKind;
use kagari_source::source::SourceFile;
use kagari_stdlib::{catalog, identity as library, namespaces};
use kagari_types::{callable::CallableImplementation, scalar::BuiltinType, ty::Ty};

#[test]
fn arithmetic_stays_direct_and_indexing_uses_its_checked_native_binding() {
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "fast-ops.kgr",
                r#"
fn main()->i32 {val a=[40];val b=a[0]+4-2;if b>=42 && !false {-(-b)}else{0}}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let module = &artifact.records().program.modules[artifact.records().program.root.index()];
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let calls = main
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::Call { callee, .. } => Some(callee),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1);
    let CallTarget::Native(index) = calls[0] else {
        panic!("native Index binding")
    };
    let providers = catalog::declarations().unwrap();
    let provider = providers
        .iter()
        .find(|provider| provider.records().identity == namespaces::module("core", "array"))
        .unwrap();
    let definitions = provider.definitions();
    let implementations = provider
        .records()
        .implementations
        .iter()
        .filter(|implementation| {
            let [parameter] = implementation.generic_params.as_slice() else {
                return false;
            };
            let Some(interface) = &implementation.trait_type else {
                return false;
            };
            implementation.for_type == Ty::Array(Box::new(parameter.as_type()))
                && definitions
                    .resolve(interface.declaration)
                    .unwrap()
                    .to_path()
                    == library::trait_id("Index")
                && interface.arguments == [Ty::Builtin(BuiltinType::U64)]
        })
        .collect::<Vec<_>>();
    assert_eq!(
        implementations.len(),
        1,
        "unique [T] as core::ops::Index<u64> implementation"
    );
    let implementation = implementations[0];
    let interface = implementation.trait_type.as_ref().unwrap();
    let output = definitions
        .lookup_child(
            interface.declaration,
            DefinitionKind::AssociatedType,
            "Output",
            0,
        )
        .unwrap();
    let item = implementation.generic_params[0].as_type();
    assert_eq!(interface.associated_types.len(), 1);
    assert_eq!(interface.associated_types[&output], item);
    let methods = implementation
        .methods
        .iter()
        .filter(|method| method.name == "index")
        .collect::<Vec<_>>();
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].return_type, item);
    let CallableImplementation::Native(binding) = methods[0].implementation else {
        panic!("native array Index binding")
    };
    let expected = definitions.resolve(binding).unwrap().to_path();
    assert_eq!(expected.module, namespaces::module("core", "array"));
    assert_eq!(
        artifact
            .definitions()
            .resolve(module.native_imports[index.index()].binding)
            .unwrap()
            .to_path(),
        expected,
    );
}
