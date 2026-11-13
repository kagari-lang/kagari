use super::*;
use crate::{execution::fixtures::FixtureExecution, interpreter_tests::common};
use crate::{
    module::entry::ProgramEntry,
    native::conversion::context::ConversionContext,
    value::{aggregates::tuples::TupleObject, scalar, strings::StringObject},
};
use kagari_bytecode::instruction::ConstantId;
use kagari_contract::ids::FunctionRef;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn executes_runtime_reflect_type_of_helper() {
    let (runtime, loaded) = load_bytecode_module(
        "reflect_type.kbc",
        test_function_module(
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::Call {
                    dst: Some(Slot::new(1)),
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
                    args: vec![Slot::new(0)],
                },
                BytecodeInstruction::Return(Some(Slot::new(1))),
            ],
            ValueType::Str,
            vec![ValueType::I32, ValueType::Str],
            vec![ConstantOperand::I32(7)],
        ),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();

    let mut vm = runtime;
    let report = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .expect("vm should execute");

    let actual = vm
        .memory
        .scope(&vm.resources, |cx, _| {
            let value = report.return_value.get(cx)?;
            StringObject::read(cx, value, str::to_owned)
        })
        .unwrap();
    assert_eq!(actual, "i32");
}

#[test]
fn executes_runtime_reflect_get_and_set_field_helpers() {
    let module = common::point_function_module(
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Slot::new(0),
                constant: ConstantId::new(0),
            },
            BytecodeInstruction::MakeStruct {
                dst: Slot::new(1),
                structure: StructId::new(0),
                arguments: vec![],
                fields: vec![Slot::new(0)],
            },
            BytecodeInstruction::Call {
                dst: Some(Slot::new(5)),
                callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField("x".to_owned())),
                args: vec![Slot::new(1)],
            },
            BytecodeInstruction::LoadConst {
                dst: Slot::new(2),
                constant: ConstantId::new(1),
            },
            BytecodeInstruction::Call {
                dst: Some(Slot::new(3)),
                callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField("x".to_owned())),
                args: vec![Slot::new(1), Slot::new(2)],
            },
            BytecodeInstruction::Call {
                dst: Some(Slot::new(4)),
                callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField("x".to_owned())),
                args: vec![Slot::new(3)],
            },
            BytecodeInstruction::MakeTuple {
                dst: Slot::new(6),
                elements: vec![Slot::new(5), Slot::new(4)],
            },
            BytecodeInstruction::Return(Some(Slot::new(6))),
        ],
        ValueType::HeapObject,
        vec![
            ValueType::I32,
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::I32,
            ValueType::HeapObject,
        ],
        vec![ConstantOperand::I32(1), ConstantOperand::I32(9)],
    );
    let (definitions, mut module) = module.into_parts();
    let result = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); 2]);
    module.functions[0].metadata.semantic.result = Some(result.clone());
    module.functions[0]
        .metadata
        .semantic
        .slots
        .insert(6, result);
    let (runtime, loaded) = load_bytecode_module(
        "reflect_field.kbc",
        DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap(),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();

    let mut vm = runtime;
    let report = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .expect("vm should execute");

    vm.memory
        .scope(&vm.resources, |cx, _| {
            let value = report.return_value.get(cx)?;
            let tuple = cx.downcast::<TupleObject>(value.object().unwrap())?;
            assert_eq!(
                scalar::read_integer(BuiltinType::I32, TupleObject::get(cx, tuple, 0)?).unwrap(),
                1
            );
            assert_eq!(
                scalar::read_integer(BuiltinType::I32, TupleObject::get(cx, tuple, 1)?).unwrap(),
                9
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn executes_runtime_reflect_set_index_helper() {
    let module = test_function_module(
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Slot::new(0),
                constant: ConstantId::new(0),
            },
            BytecodeInstruction::LoadConst {
                dst: Slot::new(1),
                constant: ConstantId::new(1),
            },
            BytecodeInstruction::MakeArray {
                element: Ty::Builtin(BuiltinType::I32),
                dst: Slot::new(2),
                elements: vec![Slot::new(0), Slot::new(1)],
            },
            BytecodeInstruction::LoadConst {
                dst: Slot::new(3),
                constant: ConstantId::new(2),
            },
            BytecodeInstruction::Call {
                dst: Some(Slot::new(4)),
                callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
                args: vec![Slot::new(2), Slot::new(3), Slot::new(1)],
            },
            BytecodeInstruction::Return(Some(Slot::new(4))),
        ],
        ValueType::HeapObject,
        vec![
            ValueType::I32,
            ValueType::I32,
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::HeapObject,
        ],
        vec![
            ConstantOperand::I32(1),
            ConstantOperand::I32(2),
            ConstantOperand::I32(0),
        ],
    );
    let (definitions, mut module) = module.into_parts();
    let array = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
    module.functions[0].metadata.semantic.result = Some(array.clone());
    for slot in [2, 4] {
        module.functions[0]
            .metadata
            .semantic
            .slots
            .insert(slot, array.clone());
    }
    let (runtime, loaded) = load_bytecode_module(
        "reflect_index.kbc",
        DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap(),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();

    let mut vm = runtime;
    let report = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .expect("vm should execute");

    let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
    let array = cx.import(&report.return_value).unwrap();
    assert_eq!(
        cx.array_element(&array, 0usize).unwrap().unwrap().as_i32(),
        Some(2)
    );
    assert_eq!(
        cx.array_element(&array, 1usize).unwrap().unwrap().as_i32(),
        Some(2)
    );
}

#[test]
fn executes_source_lowered_reflection_field_helpers() {
    let (runtime, loaded) = load_bytecode_program(
        "test.kgr",
        compile_test_bytecode(include_str!(
            "../../../tests/fixtures/reflection/field-helper.kgr"
        )),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_i32(), Some(9));
}

#[test]
fn executes_source_lowered_set_index_helper() {
    let (runtime, loaded) = load_bytecode_program(
        "test.kgr",
        compile_test_bytecode(include_str!(
            "../../../tests/fixtures/reflection/index-helper.kgr"
        )),
    );
    let program = loaded;
    let loaded = runtime.select_program(&program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
    let array = cx.import(&report.return_value).unwrap();
    assert_eq!(
        cx.array_element(&array, 0usize).unwrap().unwrap().as_i32(),
        Some(9)
    );
    assert_eq!(
        cx.array_element(&array, 1usize).unwrap().unwrap().as_i32(),
        Some(2)
    );
}

#[test]
fn executes_source_lowered_place_assignments() {
    let (runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/reflection/place-assignment.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_i32(), Some(12));
}
