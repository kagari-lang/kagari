use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding, language::Protocol, native_import::NativeWitnessImplementation,
    scalar::BuiltinType, standard::bindings::NativeProtocolMethod, types::AbiType,
};
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};

#[test]
fn numeric_providers_reject_forged_conversion_and_traversal_contracts() {
    for (binding, method) in [
        (NativeProtocolMethod::NumericSum, "sum"),
        (NativeProtocolMethod::NumericProduct, "product"),
    ] {
        for source in [
            "val source = [20,22];",
            "val source = Wrap{values:[20,22]};",
            "val source:Iterable<Item=i32,Iter=Counter<i32>> = Wrap{values:[20,22]};",
        ] {
            let program = common::bytecode_ok(&format!(
                r#"
struct Counter<T> {{val items:ArrayList<T>,var index:usize}}
impl<T> Iterator for Counter<T> {{type Item=T;fn next(self)->Option<T>{{if self.index>=self.items.len(){{None}}else{{val item=self.items[self.index];self.index+=1;Some(item)}}}}}}
struct Wrap<T> {{val values:ArrayList<T>}}
impl<T> Iterable for Wrap<T> {{type Item=T;type Iter=Counter<T>;fn iter(self)->Counter<T>{{Counter{{items:self.values,index:0}}}}}}
fn main()->i32 {{{source}i32::{method}(source)}}
"#
            ));
            let root = program.root.index();
            let import = program.modules[root]
                .native_imports
                .iter()
                .position(|import| import.binding == EngineNativeBinding::Protocol(binding))
                .unwrap();
            let contract = &program.modules[root].native_imports[import];
            let traversal = contract
                .witnesses
                .iter()
                .position(|witness| {
                    Protocol::from_id(&witness.interface.declaration) == Some(Protocol::Iterator)
                })
                .unwrap();
            let conversion = contract
                .witnesses
                .iter()
                .position(|witness| {
                    Protocol::from_id(&witness.interface.declaration) == Some(Protocol::Iterable)
                })
                .unwrap();
            verify_program(&program).unwrap();
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            for mutation in 0..12 {
                let mut forged = artifact.clone();
                let contract = &mut forged.program.modules[root].native_imports[import];
                match mutation {
                    0 => {
                        contract.witnesses.remove(traversal);
                    }
                    1 => {
                        contract.witnesses.remove(conversion);
                    }
                    2 => {
                        contract
                            .witnesses
                            .push(contract.witnesses[traversal].clone());
                    }
                    3 => {
                        contract.witnesses[traversal].receiver = AbiType::Builtin(BuiltinType::I32)
                    }
                    4 => contract.witnesses[traversal]
                        .interface
                        .associated_types
                        .values_mut()
                        .for_each(|ty| *ty = AbiType::Builtin(BuiltinType::U32)),
                    5 => {
                        contract.witnesses[conversion].implementation =
                            NativeWitnessImplementation::Primitive
                    }
                    6 => contract.witnesses[conversion]
                        .methods
                        .push(contract.instance.clone()),
                    7 => contract.requirements.clear(),
                    8 => contract.signature.result = AbiType::Builtin(BuiltinType::U32),
                    9 => contract.instance.arguments.clear(),
                    10 => {
                        contract.binding = EngineNativeBinding::Protocol(
                            if binding == NativeProtocolMethod::NumericSum {
                                NativeProtocolMethod::NumericProduct
                            } else {
                                NativeProtocolMethod::NumericSum
                            },
                        )
                    }
                    _ => {
                        contract.witnesses[traversal]
                            .methods
                            .push(contract.instance.clone());
                    }
                }
                assert!(
                    verify_program(&forged.program).is_err(),
                    "{binding:?} {source} mutation {mutation}"
                );
                let bytes = DefaultOptions::new()
                    .with_fixint_encoding()
                    .with_little_endian()
                    .serialize(&forged)
                    .unwrap();
                assert!(
                    !KbcArtifact::from_bytes(&bytes).is_ok_and(|decoded| decoded
                        .validate_for_loader(&Default::default())
                        .is_ok()),
                    "encoded {binding:?} {source} mutation {mutation}"
                );
            }
        }
    }
}
