use crate::{source::lower::lower_to_mir, tests::common};
use kagari_abi::representation::ValueType;
use kagari_mir::instruction::{CallTarget, Instruction, Terminator};

#[test]
fn struct_update_initializers_use_checked_projections_and_complete_typed_slots() {
    for (expression, expected_events) in [
        (
            "P { ..make() }",
            vec!["make", "read:0", "read:1", "read:2", "read:3", "construct"],
        ),
        (
            "P { items: items(), number: number(), ..make() }",
            vec!["items", "number", "make", "read:1", "read:3", "construct"],
        ),
        (
            "P { count: count(), items: items(), flag: flag(), number: number(), ..make() }",
            vec!["count", "items", "flag", "number", "make", "construct"],
        ),
    ] {
        let checked = common::program_ok(&format!(
            r#"
struct P<T> {{ val number: T, val flag: bool, val items: [i32], var count: i64 }}
fn number() -> i32 {{ 42 }}
fn flag() -> bool {{ true }}
fn items() -> [i32] {{ [42] }}
fn count() -> i64 {{ 99 }}
fn make() -> P<i32> {{ P {{ number: 0, flag: false, items: [0], count: 1 }} }}
fn main() -> P<i32> {{ {expression} }}
"#
        ));
        let module = lower_to_mir(checked.root(), &Default::default()).unwrap();
        let function = module.functions.iter().find(|f| f.name == "main").unwrap();
        let mut instructions = Vec::new();
        let mut next = function.entry;
        for _ in 0..function.blocks.len() {
            let block = &function.blocks[next.index()];
            instructions.extend(&block.instructions);
            match block.terminator {
                Some(Terminator::Jump(target)) => next = target,
                Some(Terminator::Return(_)) => break,
                _ => panic!("expected a straight-line constructor: {expression}"),
            }
        }
        let expected_types = [
            ValueType::I32,
            ValueType::Bool,
            ValueType::HeapObject,
            ValueType::I64,
        ];
        let mut events = Vec::new();
        let mut values = [None; 4];
        let mut base_value = None;
        let mut constructed = None;
        for instruction in instructions {
            match instruction {
                Instruction::Call {
                    dst: Some(dst),
                    callee: CallTarget::Function(id),
                    ..
                } => {
                    let callee = module.functions.iter().find(|f| f.id == *id).unwrap();
                    events.push(callee.name.clone());
                    if callee.name == "make" {
                        assert!(base_value.replace(*dst).is_none());
                    } else {
                        let slot = match callee.name.as_str() {
                            "number" => 0,
                            "flag" => 1,
                            "items" => 2,
                            "count" => 3,
                            _ => panic!("unexpected operand call"),
                        };
                        assert!(values[slot].replace(*dst).is_none());
                    }
                }
                Instruction::ReadAggregateField { dst, base, field } => {
                    assert_eq!(Some(*base), base_value);
                    assert_eq!(field.reference, None);
                    assert_eq!(dst.ty, expected_types[field.slot]);
                    assert_eq!(
                        function.semantic.registers[&dst.temp.index()],
                        module.structure(&field.owner).unwrap().fields[field.slot].ty
                    );
                    assert!(values[field.slot].replace(*dst).is_none());
                    events.push(format!("read:{}", field.slot));
                }
                Instruction::MakeStruct {
                    dst,
                    structure,
                    fields,
                } => {
                    assert!(constructed.replace(*dst).is_none());
                    assert_ne!(Some(*dst), base_value);
                    assert_eq!(fields.len(), values.len());
                    let layout = module.structure(structure).unwrap();
                    let mut slots = Vec::new();
                    for field in fields {
                        assert_eq!(Some(field.value), values[field.slot]);
                        assert_eq!(field.value.ty, expected_types[field.slot]);
                        assert_eq!(
                            function.semantic.registers[&field.value.temp.index()],
                            layout.fields[field.slot].ty
                        );
                        slots.push(field.slot);
                    }
                    slots.sort_unstable();
                    assert_eq!(slots, [0, 1, 2, 3]);
                    events.push("construct".into());
                }
                _ => {}
            }
        }
        assert!(constructed.is_some());
        assert_eq!(events, expected_events, "{expression}");
        let (_, roots) = function.root_slots();
        assert!(roots.contains(&base_value.unwrap().temp));
        assert!(roots.contains(&values[2].unwrap().temp));
        assert!(roots.contains(&constructed.unwrap().temp));
    }
    for (annotation, constructor) in [
        ("Cell<i32>", "Cell<i32>"),
        ("<Tag as Pick>::Item", "Cell<i32>"),
        ("Cell<<Tag as Scalar>::Item>", "Cell<<Tag as Scalar>::Item>"),
    ] {
        common::bytecode_ok(&format!(
            "trait Pick {{ type Item; }} trait Scalar {{ type Item; }} struct Tag {{}} struct Cell<T> {{ val value: T }} impl Pick for Tag {{ type Item = Cell<i32>; }} impl Scalar for Tag {{ type Item = i32; }} fn main() -> i32 {{ val copy = |base: {annotation}| {constructor} {{ ..base }}; copy.call((Cell {{ value: 42 }},)).value }}"
        ));
    }

    common::bytecode_ok(
        "struct Cell<T> { val value: T } fn duplicate<T>(base: Cell<T>) -> Cell<T> { Cell { ..base } } fn main() -> i32 { duplicate(Cell { value: 42 }).value }",
    );
}
