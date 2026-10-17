use crate::tests::native_boundary::{compile_program, support::round_trip_program};
use kagari_bytecode::{instruction::BytecodeInstruction, program::verify_program};
use kagari_types::{reference::ReferenceAccess, scalar::BuiltinType, ty::Ty};

#[test]
fn source_free_reference_facts_reject_target_upgrade_storage_and_suspension_corruption() {
    let program = round_trip_program(compile_program(
        r#"
        pub struct Player { pub var hp: i32 }
        pub fn inspect(value: Ref<Player>) -> i32 { 42 }
        pub fn forward(value: Ref<Player>) -> i32 { inspect(value) }
        pub fn read_mut(value: RefMut<Player>) -> i32 { inspect(value) }
        pub fn tuple(value: Ref<Player>) -> (i32, i32) { inspect(value); (1, 2) }
    "#,
        None,
    ));
    verify_program(program.records(), program.definitions()).unwrap();
    let root = program.records().root.index();

    let mut invalid = program.records().clone();
    let forward = invalid.modules[root]
        .functions
        .iter_mut()
        .find(|f| f.name == "forward")
        .unwrap();
    let (_, ty) = forward
        .metadata
        .semantic
        .registers
        .iter_mut()
        .find(|(_, ty)| matches!(ty, Ty::Reference { .. }))
        .unwrap();
    let Ty::Reference { access, .. } = ty else {
        unreachable!()
    };
    *access = ReferenceAccess::Mutable;
    assert!(verify_program(&invalid, program.definitions()).is_err());

    let mut invalid = program.records().clone();
    let inspect = invalid.modules[root]
        .functions
        .iter_mut()
        .find(|f| f.name == "inspect")
        .unwrap();
    let Ty::Reference { target, .. } = inspect.metadata.semantic.params.get_mut(&0).unwrap() else {
        unreachable!()
    };
    **target = Ty::Builtin(BuiltinType::I32);
    assert!(verify_program(&invalid, program.definitions()).is_err());

    let mut invalid = program.records().clone();
    let tuple = invalid.modules[root]
        .functions
        .iter_mut()
        .find(|f| f.name == "tuple")
        .unwrap();
    let reference = tuple
        .metadata
        .semantic
        .registers
        .iter()
        .find(|(_, ty)| matches!(ty, Ty::Reference { .. }))
        .map(|(i, _)| *i)
        .unwrap();
    let source = tuple
        .instructions
        .iter()
        .find_map(|instruction| match instruction {
            BytecodeInstruction::LoadLocal { dst, .. } if dst.index() == reference => Some(*dst),
            _ => None,
        })
        .unwrap();
    let elements = tuple
        .instructions
        .iter_mut()
        .find_map(|instruction| match instruction {
            BytecodeInstruction::MakeTuple { elements, .. } => Some(elements),
            _ => None,
        })
        .unwrap();
    elements[0] = source;
    assert!(verify_program(&invalid, program.definitions()).is_err());

    let mut invalid = program.records().clone();
    let module = &mut invalid.modules[root];
    let inspect = module
        .functions
        .iter_mut()
        .find(|f| f.name == "inspect")
        .unwrap();
    inspect.metadata.effects.may_suspend = true;
    module.function_table[inspect.id.index()].effects = inspect.metadata.effects;
    assert!(verify_program(&invalid, program.definitions()).is_err());
}
