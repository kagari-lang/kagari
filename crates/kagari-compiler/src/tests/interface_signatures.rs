use crate::{bytecode::lower_program_to_bytecode, tests::common};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget},
    program::verify_program,
};
use kagari_mir::codec::{decode_program, encode_program};
use kagari_types::{scalar::BuiltinType, ty::Ty};

const SOURCE: &str = r#"
trait Producer { type Items; fn items(self) -> Self::Items; }
struct Item { val value: i32 }
struct Holder<T> { val item: T }
impl<T> Producer for Holder<T> {
    type Items = Vec<T>;
    fn items(self) -> Vec<T> { Vec::from([self.item]) }
}
trait Relay {
    fn relay<P: Producer>(self, source: P) -> P::Items { source.items() }
}
impl Relay for i32 {}
pub fn main() -> i32 {
    val relay: Relay = 0;
    relay.relay(Holder { item: Item { value: 42 } })[0].value
}
"#;

#[test]
fn applied_interface_associated_signatures_survive_both_artifact_boundaries() {
    let mir = common::mir_ok(SOURCE);
    let mir = decode_program(
        &encode_program(&mir, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let artifact =
        KbcArtifact::from_program(lower_program_to_bytecode(&mir).unwrap(), Default::default())
            .unwrap();
    let artifact = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    verify_program(&artifact.program).unwrap();
    let facts = artifact
        .program
        .modules
        .iter()
        .flat_map(|module| &module.functions)
        .flat_map(|function| &function.instructions)
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::Call {
                callee: CallTarget::InterfaceMethod { contract, .. },
                ..
            } => Some(&contract.normalizations),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(facts.len(), 1);
    assert!(matches!(&facts[0].source, Ty::Projection { .. }));
    assert!(
        matches!(&facts[0].result, Ty::NativeObject(nominal) if matches!(nominal.arguments.as_slice(), [Ty::Struct(_)]))
    );
    artifact.into_verified(&Default::default()).unwrap();
}

#[test]
fn interface_signature_normalizations_are_evidence_not_trusted_type_overrides() {
    let program = common::bytecode_ok(SOURCE);
    for mutation in 0..5 {
        let mut forged = program.clone();
        let call = forged
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::InterfaceMethod { contract, .. },
                    ..
                } if !contract.normalizations.is_empty() => Some(contract),
                _ => None,
            })
            .unwrap();
        match mutation {
            0 => call.normalizations.clear(),
            1 => call.normalizations.push(call.normalizations[0].clone()),
            2 => call.normalizations[0].source = Ty::Builtin(BuiltinType::I32),
            3 => {
                // Keep the physical HeapObject representation while changing
                // the associated result's semantic element type.
                let Ty::NativeObject(nominal) = &mut call.normalizations[0].result else {
                    panic!("Vec result")
                };
                nominal.arguments[0] = Ty::Builtin(BuiltinType::Bool);
            }
            4 => call.normalizations[0].result = call.normalizations[0].source.clone(),
            _ => unreachable!(),
        }
        assert!(verify_program(&forged).is_err(), "mutation {mutation}");
        assert!(
            KbcArtifact::from_program(forged, Default::default()).is_err(),
            "mutation {mutation}"
        );
    }
}
