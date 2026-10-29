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
                        initializers: vec![],
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
    let id = unsafe {
        runtime.alloc_array_transport(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
    }
    .unwrap();
    // Fixed arrays permit element replacement without invalidating traversal.
    unsafe { gc.array_set_transport(id, 0, Value::I32(42)) }.unwrap();
    assert_eq!(
        unsafe { gc.array_set_transport(id, 1, Value::I32(9)) }
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::IndexOutOfBounds,
    );
    assert_eq!(
        unsafe { gc.array_get_transport(id, 0) },
        Some(Value::I32(42))
    );
    assert_eq!(unsafe { gc.array_len_transport(id) }, Some(1));
    // Fixed length is an array invariant, independent of any iterator.
    unsafe { gc.array_set_transport(id, 0, Value::I32(7)) }.unwrap();
    assert_eq!(
        unsafe { gc.array_snapshot_transport(id) },
        Some(vec![Value::I32(7)])
    );
}
