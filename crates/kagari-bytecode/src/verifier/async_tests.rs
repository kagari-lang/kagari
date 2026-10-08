use crate::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction as I, CallTarget, LocalSlot, Register},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::{BytecodeProgram, ModuleRef, verify_program},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity};
use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{TypeDef, TypeDefKind, module::ModuleDecl, native::NativeStorageLayout},
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};

fn program() -> BytecodeProgram {
    let owner = ModuleDecl::new(ModuleIdentity::single_file("await-contract"));
    let id = owner.definition(DefinitionKind::AssociatedType, "Future");
    let output = Ty::Array(
        Box::new(Ty::Builtin(BuiltinType::I32)),
        CollectionAccess::Mutable,
    );
    let future = Ty::NativeObject(NominalTy {
        declaration: id.clone(),
        arguments: vec![output.clone()],
        associated_types: Default::default(),
    });
    let mut metadata = FunctionMetadata {
        params: vec![ValueType::HeapObject],
        locals: vec![ValueType::HeapObject],
        registers: vec![ValueType::HeapObject; 2],
        return_type: ValueType::HeapObject,
        roots: RootSlotLayout::from_types(&[ValueType::HeapObject], &[ValueType::HeapObject; 2]),
        ..Default::default()
    };
    metadata.effects.may_suspend = true;
    metadata.semantic.params.insert(0, future.clone());
    metadata.semantic.locals.insert(0, future.clone());
    metadata.semantic.registers.insert(0, future.clone());
    metadata.semantic.registers.insert(1, output.clone());
    metadata.semantic.result = Some(output);
    let record = FunctionRecord {
        id: FunctionRef::new(0),
        identity: None,
        name: "wait".into(),
        params: metadata.params.clone(),
        return_type: metadata.return_type,
        effects: metadata.effects,
    };
    BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            identity: owner.identity,
            types: vec![ValueType::Unit, ValueType::HeapObject],
            public_items: vec![PublicItem::Type(TypeDef {
                name: "Future".into(),
                kind: TypeDefKind::NativeStorage(NativeStorageLayout::Future),
                generic_params: vec![GenericParam {
                    owner: id,
                    position: 0,
                }],
                bounds: vec![],
                fields: vec![],
                variants: vec![],
            })],
            function_table: vec![record],
            functions: vec![BytecodeFunction {
                id: FunctionRef::new(0),
                identity: None,
                name: "wait".into(),
                parameter_count: 1,
                register_count: 2,
                local_count: 1,
                metadata,
                instructions: vec![
                    I::LoadLocal {
                        dst: Register::new(0),
                        local: LocalSlot::new(0),
                    },
                    I::Await {
                        dst: Register::new(1),
                        value: Register::new(0),
                        future,
                    },
                    I::Return(Some(Register::new(1))),
                ],
            }],
            ..Default::default()
        }],
    }
}

#[test]
fn async_artifact_validation_contract() {
    let valid = program();
    let artifact = KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    // Every case keeps the physical heap-object representation unchanged.
    for mutation in 0..6 {
        let mut invalid = valid.clone();
        let module = &mut invalid.modules[0];
        let function = &mut module.functions[0];
        match mutation {
            0 => {
                let PublicItem::Type(ty) = &mut module.public_items[0] else {
                    unreachable!()
                };
                ty.kind = TypeDefKind::NativeStorage(NativeStorageLayout::Opaque);
            }
            1 => {
                function.metadata.effects.may_suspend = false;
                module.function_table[0].effects.may_suspend = false;
            }
            2 => {
                function.metadata.semantic.registers.remove(&0);
            }
            3 => {
                function.metadata.semantic.registers.insert(
                    1,
                    Ty::Array(
                        Box::new(Ty::Builtin(BuiltinType::I32)),
                        CollectionAccess::ReadOnly,
                    ),
                );
            }
            4 => {
                let I::Await { value, .. } = &mut function.instructions[1] else {
                    unreachable!()
                };
                *value = Register::new(1);
            }
            5 => {
                function.instructions[1] = I::Call {
                    dst: Some(Register::new(1)),
                    callee: CallTarget::Function(FunctionRef::new(0)),
                    args: vec![Register::new(0)],
                };
            }
            _ => unreachable!(),
        }
        assert!(
            verify_program(&invalid).is_err(),
            "invalid await contract {mutation}"
        );
    }
}
