//! Run with `cargo run -p kagari-runtime --example collection_iteration`.
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_contract::updates::UpdateEvidence;
use kagari_runtime::{Runtime, error::RuntimeErrorKind, value::Value};
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn main() {
    let mut runtime = Runtime::default();
    let context = DefinitionContext::new().unwrap();
    let owner_program = runtime
        .load_program(
            "arrays",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    // This owner has no declarations or executable bodies.
                    update_evidence: UpdateEvidence {
                        source_manifest: None,
                        sources: vec![],
                        bodies: vec![],
                        roots: vec![],
                        targets: vec![],
                        selections: vec![],
                    },
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();

    let gc = runtime.gc();
    let id = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
    let value = Value::Array(id);
    let revision = gc.collection_revision(&value).unwrap();
    // Fixed arrays permit element replacement without invalidating traversal.
    gc.array_set(id, 0, Value::I32(42)).unwrap();
    assert_eq!(
        gc.array_set(id, 1, Value::I32(9)).unwrap_err().kind(),
        RuntimeErrorKind::IndexOutOfBounds,
    );
    assert_eq!(gc.array_get(id, 0), Some(Value::I32(42)));
    assert!(gc.sequence_push(id, Value::I32(2)).is_err());
    assert_eq!(gc.array_len(id), Some(1));
    assert!(gc.sequence_pop(id).is_err());
    gc.check_collection_revision(&value, revision).unwrap();
    // Fixed length is an array invariant, independent of any iterator.
    assert!(gc.sequence_push(id, Value::I32(2)).is_err());
    assert!(gc.sequence_clear(id).is_err());
    gc.array_set(id, 0, Value::I32(7)).unwrap();
    assert_eq!(gc.array_snapshot(id), Some(vec![Value::I32(7)]));
}
