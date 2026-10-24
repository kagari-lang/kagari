//! Exact-version ownership and bounded R13 retention controls.
use super::*;

#[cfg(feature = "execution-diagnostics")]
mod probes;

fn scoped_program(name: &str) -> DefinitionMetadata<BytecodeProgram> {
    let context = DefinitionContext::new().unwrap();
    let mut raw = program().into_records();
    let root = context.intern_root(&raw.modules[0].identity).unwrap();
    raw.modules[0].structures.push(StructLayout {
        declaration: context
            .intern_child(root, DefinitionKind::Struct, name, 0)
            .unwrap(),
        arguments: vec![],
        fields: vec![],
    });
    DefinitionMetadata::checked(context.snapshot(), raw, &Default::default()).unwrap()
}

#[test]
fn referenced_normalization_releases_original_and_intermediate_graphs() {
    let context = DefinitionContext::new().unwrap();
    let source = VerifiedProgram::new(scoped_program("Record"), &context).unwrap();
    let same_context = source.normalized(&context, &Default::default()).unwrap();
    assert!(source.same_version(&same_context));
    assert!(Arc::ptr_eq(
        &source.modules()[0],
        &same_context.modules()[0]
    ));
    drop(same_context);
    let source_id = source.modules()[0].structures[0].declaration;
    let path = source.definitions().resolve(source_id).unwrap().to_path();
    let original = source
        .modules()
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    let mut first = runtime();
    let a_program = first.load_verified_program("a", source.clone()).unwrap();
    let a = first.select_program(&a_program).unwrap();
    let a_id = a.bytecode.structures[0].declaration;
    assert_ne!(source_id, a_id);
    assert!(source.same_version(a.verified_program()));
    let later = first
        .definition_context()
        .intern_child(a_id, DefinitionKind::Field, "later", 0)
        .unwrap();
    assert!(a.definitions().resolve(later).is_err());
    let intermediate = a
        .verified_program()
        .modules()
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    drop((source, context));
    assert!(original.iter().all(|weak| weak.upgrade().is_none()));
    let mut second = runtime();
    let b_program = second
        .load_verified_program("b", a.verified_program().clone())
        .unwrap();
    let b = second.select_program(&b_program).unwrap();
    let b_id = b.bytecode.structures[0].declaration;
    assert_ne!(a_id, b_id);
    assert_eq!(b.definitions().resolve(b_id).unwrap().to_path(), path);
    assert!(b.definitions().resolve(a_id).is_err());
    assert!(a.verified_program().same_version(b.verified_program()));
    drop((a, first));
    assert!(intermediate.iter().all(|weak| weak.upgrade().is_none()));
    assert!(original.iter().all(|weak| weak.upgrade().is_none()));
    // SAFETY: the fixture implements the verified two-instruction Unit function.
    let installed =
        unsafe { second.install_native_function(&b, product(execute, Arc::default())) }.unwrap();
    assert_eq!(
        second.invoke_native_function(&installed).unwrap(),
        Value::Unit
    );
    drop((installed, b, second));
    assert!(original.iter().all(|weak| weak.upgrade().is_none()));
    println!(
        "RL03_TOKEN original_after_source_drop=0 original_after_a_drop=0 intermediate_after_a_drop=0 original_after_b_drop=0"
    );
}
