use crate::{
    BytecodeArtifact, engine::EngineConfig, error::EmbeddingError, test_support::execution,
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BinaryOp, BytecodeInstruction, CallTarget, ScalarOperand},
    program::verify_program,
    verifier::BytecodeVerificationError,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::types::{InterfaceBridge, PublicItem};
use kagari_source::source::SourceFile;
use kagari_types::ty::Ty;

#[test]
fn portable_comparisons_cannot_bypass_static_object_or_protocol_rules() {
    let engine = execution::engine(EngineConfig::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "comparison-proof.kgr",
                r#"
trait Read { fn read(self) -> i32; }
struct Item { val value: i32 }
impl Read for Item { fn read(self) -> i32 { self.value } }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { false } }
enum E { Value }
impl PartialEq for E { fn eq(self, other: Self) -> bool { false } }
enum Plain { Value }
enum Other { Value }
fn enum_probe(a: E, b: E, flag: bool) -> bool { if flag { true } else { false } }
fn tuple_probe(a: (i32, i32), b: Item, flag: bool) -> bool { if flag { true } else { false } }
fn interface_probe(a: Read, b: Item, flag: bool) -> bool { if flag { true } else { false } }
fn distinct_probe(a: Plain, b: Other, flag: bool) -> bool { if flag { true } else { false } }
fn main() -> bool { val a: Read = Item { value: 1 }; a.read() == a.read() }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap()
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
    for (identity, concrete) in [(true, false), (false, false), (false, true)] {
        let mut program = decoded.program.clone();
        let function = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .find(|function| function.name == "main")
            .unwrap();
        let (boxed, object) = function
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                BytecodeInstruction::MakeInterface { dst, value, .. } => Some((*dst, *value)),
                _ => None,
            })
            .unwrap();
        let operand = if concrete { object } else { boxed };
        let instruction = function
            .instructions
            .iter_mut()
            .find(|instruction| {
                matches!(
                    instruction,
                    BytecodeInstruction::Binary {
                        op: BinaryOp::Eq,
                        ..
                    }
                )
            })
            .unwrap();
        let BytecodeInstruction::Binary { op, lhs, rhs, .. } = instruction else {
            unreachable!()
        };
        *op = if identity {
            BinaryOp::IdentityEq
        } else {
            BinaryOp::Eq
        };
        *lhs = operand;
        *rhs = operand;
        assert!(
            KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
                .is_err(),
            "identity={identity}, concrete={concrete}"
        );
    }
    let mut accepted = Vec::new();
    for (probe, op, reverse) in [
        ("enum_probe", BinaryOp::Eq, false),
        ("enum_probe", BinaryOp::NotEq, false),
        ("enum_probe", BinaryOp::IdentityEq, false),
        ("enum_probe", BinaryOp::IdentityNotEq, false),
        ("tuple_probe", BinaryOp::IdentityEq, false),
        ("tuple_probe", BinaryOp::IdentityNotEq, true),
        ("interface_probe", BinaryOp::IdentityEq, false),
        ("interface_probe", BinaryOp::IdentityNotEq, true),
        ("distinct_probe", BinaryOp::Eq, false),
        ("distinct_probe", BinaryOp::NotEq, true),
    ] {
        let mut program = decoded.program.clone();
        let function = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .find(|function| function.name == probe)
            .unwrap();
        let args = &function.metadata.parameter_slots;
        let (lhs, rhs) = if reverse {
            (args[1], args[0])
        } else {
            (args[0], args[1])
        };
        let instruction = function
            .instructions
            .iter_mut()
            .find(|instruction| matches!(instruction, BytecodeInstruction::Branch { .. }))
            .unwrap();
        let BytecodeInstruction::Branch {
            then_target,
            else_target,
            ..
        } = *instruction
        else {
            unreachable!()
        };
        *instruction = BytecodeInstruction::PredicateBranch {
            op,
            lhs: ScalarOperand::Slot(lhs),
            rhs: ScalarOperand::Slot(rhs),
            negated: false,
            then_target,
            else_target,
        };
        let expected = if matches!(op, BinaryOp::IdentityEq | BinaryOp::IdentityNotEq) {
            BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "identity requires statically known object types",
            }
        } else {
            BytecodeVerificationError::InvalidHostInterface(
                "trait output or host bound has no unique valid implementation".into(),
            )
        };
        let mut forged = decoded.clone();
        forged.program = program;
        let wire = KbcArtifact::from_bytes(&forged.to_bytes(&context.snapshot()).unwrap())
            .unwrap()
            .import_into(&context, &Default::default())
            .unwrap();
        for (route, result) in [
            (
                "direct",
                verify_program(&forged.program, &context.snapshot(), &Default::default()),
            ),
            (
                "decoded",
                verify_program(
                    &wire.records().program,
                    wire.definitions(),
                    &Default::default(),
                ),
            ),
        ] {
            match result {
                Ok(()) => accepted.push(format!("{probe} {op:?} reverse={reverse} ({route})")),
                Err(error) => assert_eq!(error, expected, "{probe} {op:?} ({route})"),
            }
        }
    }
    assert!(
        accepted.is_empty(),
        "forged predicates admitted: {accepted:?}"
    );
}

#[test]
fn portable_unused_signatures_require_dynamic_compatibility() {
    let engine = execution::engine(EngineConfig::default());
    let source = "pub trait Read { fn read(self) -> i32; } pub fn unused(value: Read) -> i32 { 42 } fn main() -> i32 { 42 }";
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("unused-interface.kgr", source),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let mut records = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    // Populate all implicit references before mutating the declared trait surface.
    records = KbcArtifact::from_program(
        records.program,
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let contract = records
        .program
        .modules
        .iter_mut()
        .flat_map(|module| &mut module.public_items)
        .find_map(|item| match item {
            PublicItem::Trait(contract) if contract.name == "Read" => Some(contract),
            _ => None,
        })
        .unwrap();
    let Ty::SelfType(owner) = contract.methods[0].params[0].ty else {
        panic!("trait receiver")
    };
    contract.methods[0].return_type = Ty::SelfType(owner);
    assert!(
        KbcArtifact::from_program(
            records.program.clone(),
            Default::default(),
            &context,
            &Default::default()
        )
        .is_err()
    );
    let bytes = records.to_bytes(&context.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
    assert!(
        verify_program(
            &decoded.records().program,
            decoded.definitions(),
            &Default::default()
        )
        .is_err()
    );
    assert!(matches!(
        engine.compile_to_artifact(
            SourceFile::new(
                "invalid-unused.kgr",
                source.replace("fn read(self) -> i32", "fn read(self) -> Self")
            ),
            Default::default()
        ),
        Err(EmbeddingError::Diagnostics { .. })
    ));
}

#[test]
fn intrinsic_interface_bridges_retain_checked_protocol_targets() {
    let engine = execution::engine(EngineConfig::default());
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "intrinsic-bridge.kgr",
                r#"
use std::fmt::Debug;
fn forged(value: i32) -> String { "forged" }
fn main() -> String { val view: Debug = 42; view.debug() }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let mut program = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records()
        .program;
    let module = program.modules.iter_mut().find(|module| module.public_items.iter().any(|item| matches!(item, PublicItem::InterfaceTable(table) if table.bridge == Some(InterfaceBridge::Intrinsic)))).unwrap();
    let forged = module
        .functions
        .iter()
        .find(|function| function.name == "forged")
        .unwrap()
        .id;
    let bridge = module
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table)
                if table.bridge == Some(InterfaceBridge::Intrinsic) =>
            {
                Some(table.declaration)
            }
            _ => None,
        })
        .unwrap();
    let definitions = context.snapshot();
    let function = module
        .functions
        .iter_mut()
        .find(|function| {
            function.identity.as_ref().is_some_and(|identity| {
                definitions.parent(identity.declaration) == Ok(Some(bridge))
            })
        })
        .unwrap();
    let instruction = function
        .instructions
        .iter_mut()
        .find(|instruction| matches!(instruction, BytecodeInstruction::Call { .. }))
        .unwrap();
    let BytecodeInstruction::Call { callee, .. } = instruction else {
        unreachable!()
    };
    *callee = CallTarget::Function(forged);
    assert!(
        KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
            .is_err()
    );
}
