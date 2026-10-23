use crate::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, Slot},
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InstructionOrigin,
        RootSlotLayout,
    },
    program::{BytecodeProgram, ModuleRef},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::operations::{IterOp, StringIterKind};
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn string_cursor_operations_roundtrip_with_direct_source_and_exact_item_types() {
    let context = DefinitionContext::new().unwrap();
    for kind in [
        StringIterKind::Chars,
        StringIterKind::CharIndices,
        StringIterKind::Bytes,
    ] {
        let instructions = vec![
            BytecodeInstruction::Iter {
                dst: Slot::new(1),
                value: Some(Slot::new(0)),
                ty: kind.source_type(),
                op: IterOp::String(kind),
            },
            BytecodeInstruction::Return(Some(Slot::new(1))),
        ];
        let slots = vec![ValueType::Str, ValueType::HeapObject];
        let mut metadata = FunctionMetadata {
            params: vec![ValueType::Str],
            parameter_slots: vec![Slot::new(0)],
            return_type: ValueType::HeapObject,
            roots: RootSlotLayout::from_types(&slots),
            slots,
            origins: (0..instructions.len())
                .map(|instruction| InstructionOrigin {
                    instruction,
                    span: Default::default(),
                    line: None,
                    column: None,
                })
                .collect(),
            ..Default::default()
        };
        metadata.semantic.params.insert(0, kind.source_type());
        metadata.semantic.result = Some(Ty::Iter(Box::new(kind.item_type())));
        let function = BytecodeFunction {
            name: "cursor".into(),
            parameter_count: 1,
            slot_count: 2,
            metadata,
            instructions,
            ..Default::default()
        };
        let program = BytecodeProgram {
            modules: vec![BytecodeModule {
                types: vec![ValueType::Str, ValueType::HeapObject],
                function_table: vec![FunctionRecord {
                    id: function.id,
                    identity: None,
                    name: function.name.clone(),
                    params: function.metadata.params.clone(),
                    return_type: function.metadata.return_type,
                    effects: function.metadata.effects,
                }],
                functions: vec![function],
                ..Default::default()
            }],
            exports: Default::default(),
            root: ModuleRef::new(0),
        };
        let artifact = KbcArtifact::from_program(
            program.clone(),
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
        let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
        let decoded = KbcArtifact::from_bytes(&bytes)
            .unwrap()
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
        decoded
            .validate_for_loader(&Default::default(), &context, &Default::default())
            .unwrap();
        assert_eq!(
            decoded.program.modules[0].functions[0].instructions,
            program.modules[0].functions[0].instructions
        );

        let mut invalid = program;
        invalid.modules[0].functions[0].metadata.semantic.result = Some(Ty::Iter(Box::new(
            Ty::Builtin(if kind == StringIterKind::Bytes {
                BuiltinType::I64
            } else {
                BuiltinType::String
            }),
        )));
        assert!(
            KbcArtifact::from_program(invalid, Default::default(), &context, &Default::default())
                .is_err()
        );
    }
}
