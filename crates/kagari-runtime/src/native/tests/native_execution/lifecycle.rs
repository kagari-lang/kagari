//! Exact-version ownership and bounded R13 retention controls.
use super::*;
use crate::module::retention::RetentionOrigin;

#[cfg(feature = "execution-diagnostics")]
mod probes;

fn scoped_program(name: &str) -> DefinitionMetadata<BytecodeProgram> {
    let context = DefinitionContext::new().unwrap();
    let mut raw = program()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
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
fn shared_code_survives_its_preparer_and_each_independent_installation() {
    let context = DefinitionContext::new().unwrap();
    let source = VerifiedProgram::new(scoped_program("Record"), &context).unwrap();
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
    assert_eq!(source_id, a_id);
    assert!(source.same_version(a.verified_program()));
    let later = first
        .definition_context()
        .intern_child(
            a.identity_admission().runtime_id(a_id).unwrap(),
            DefinitionKind::Field,
            "later",
            0,
        )
        .unwrap();
    assert!(a.definitions().resolve(later).is_err());
    let intermediate = a
        .verified_program()
        .modules()
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    drop((source, context));
    assert!(original.iter().all(|weak| weak.upgrade().is_some()));
    let mut second = runtime();
    let b_program = second
        .load_verified_program("b", a.verified_program().clone())
        .unwrap();
    let b = second.select_program(&b_program).unwrap();
    let b_id = b.bytecode.structures[0].declaration;
    assert_eq!(a_id, b_id);
    assert!(Arc::ptr_eq(&a.bytecode, &b.bytecode));
    assert_eq!(b.definitions().resolve(b_id).unwrap().to_path(), path);
    assert!(a.verified_program().same_version(b.verified_program()));
    drop((a, first));
    assert!(intermediate.iter().all(|weak| weak.upgrade().is_some()));
    assert!(original.iter().all(|weak| weak.upgrade().is_some()));
    // SAFETY: the fixture implements the verified two-instruction Unit function.
    let installed = unsafe {
        second.install_native_function(
            &b,
            product(execute, Arc::default()),
            RetentionOrigin::Current,
        )
    }
    .unwrap();
    assert_unit(&mut second, &installed);
    drop((installed, b, second));
    assert!(original.iter().all(|weak| weak.upgrade().is_none()));
}
