//! Run with `cargo run -p kagari-runtime --example array_access`.
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_contract::updates::UpdateEvidence;
use kagari_runtime::module::VerifiedProgram;
use kagari_runtime::{
    Runtime, error::RuntimeErrorKind, native::conversion::context::ConversionContext,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn main() {
    let mut runtime = Runtime::default();
    let context = DefinitionContext::new().unwrap();
    let owner_program = runtime
        .load_verified_program(
            "arrays",
            VerifiedProgram::new(
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
                &context,
            )
            .unwrap(),
        )
        .unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();

    let root = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), &[1i32.into()])
        .unwrap();
    {
        let mut conversion = ConversionContext::new(&mut runtime, &owner).unwrap();
        let value = conversion.import(&root).unwrap();
        // Fixed arrays preserve their declared element type and length. These
        // views borrow the conversion's execution source and create no Host root.
        conversion.array_set(&value, 0, &42i32.into()).unwrap();
        assert_eq!(
            conversion
                .array_set(&value, 1, &9i32.into())
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::IndexOutOfBounds
        );
        assert_eq!(
            conversion
                .array_element(&value, 0)
                .unwrap()
                .unwrap()
                .as_i32(),
            Some(42)
        );
        assert_eq!(conversion.array_len(&value).unwrap(), 1);
    }
    runtime.collect_garbage().unwrap();
    let mut conversion = ConversionContext::new(&mut runtime, &owner).unwrap();
    let value = conversion.import(&root).unwrap();
    conversion.array_set(&value, 0, &7i32.into()).unwrap();
    assert_eq!(
        conversion
            .array_element(&value, 0)
            .unwrap()
            .unwrap()
            .as_i32(),
        Some(7)
    );
}
