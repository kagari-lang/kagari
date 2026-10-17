use crate::{source::lower::lower_to_mir, tests::common};
use kagari_abi::representation::ValueType;
use kagari_mir::instruction::{Constant, Instruction, Terminator};

#[test]
fn ignored_pattern_children_do_not_project_or_skip_initializer_calls() {
    for (body, field_reads, index_reads) in [
        ("val _ = make(); 42", 0, 0),
        ("val () = { make(); }; 42", 0, 0),
        ("val Pair { .. } = make(); 42", 0, 0),
        ("val Pair { right: _, left: (_, _) } = make(); 42", 0, 0),
        (
            "val Pair { right: _, left: (_, kept) } = make(); kept",
            1,
            1,
        ),
        ("val (_, (_, _)) = (make(), (2, 3)); 42", 0, 0),
        (
            "match make() { Pair { left: (_, 0), .. } => 1, _ => 42 }",
            1,
            1,
        ),
    ] {
        let source = format!(
            "struct Pair {{ val left: (i32, i32), val right: i32 }} \
             fn make() -> Pair {{ Pair {{ left: (20, 22), right: 0 }} }} \
             fn main() -> i32 {{ {body} }}"
        );
        let checked = common::program_ok(&source);
        let mir = lower_to_mir(checked.root(), &Default::default()).unwrap();
        let function = mir
            .functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap();
        let instructions = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .collect::<Vec<_>>();
        assert_eq!(
            instructions
                .iter()
                .filter(|instruction| matches!(instruction, Instruction::Call { .. }))
                .count(),
            1,
            "initializer must still execute exactly once: {body}",
        );
        assert_eq!(
            instructions
                .iter()
                .filter(|instruction| matches!(instruction, Instruction::ReadAggregateField { .. }))
                .count(),
            field_reads,
            "ignored struct subtrees must not be read: {body}",
        );
        assert_eq!(
            instructions
                .iter()
                .filter(|instruction| matches!(instruction, Instruction::ReadAggregateIndex { .. }))
                .count(),
            index_reads,
            "ignored tuple children must not be read, but decisions must be retained: {body}",
        );
    }
}

#[test]
fn annotated_bindings_preserve_produced_interface_and_callable_types() {
    let checked = common::program_ok(
        r#"
trait Read { fn get(self) -> i32; }
struct Number { val value: i32 }
impl Read for Number { fn get(self) -> i32 { self.value } }
impl Fn<(i32,)> for Number {
    type Output = i32;
    fn call(self, args: (i32,)) -> i32 { self.value + args[0] }
}
fn main() -> i32 {
    val readable: Read = Number { value: 1 };
    val callback: fn(i32) -> i32 = Number { value: 2 };
    val (member, action): (Read, fn(i32) -> i32) =
        (Number { value: 20 }, Number { value: 19 });
    readable.get() + callback(0) + member.get() + action(0)
}
"#,
    );
    let mir = lower_to_mir(checked.root(), &Default::default()).unwrap();
    let function = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    for name in ["readable", "callback", "member", "action"] {
        let local = function
            .debug
            .locals
            .iter()
            .find(|local| local.name == name)
            .unwrap()
            .local;
        let expected = &function.semantic.locals[&local.index()];
        let value = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find_map(|instruction| match instruction {
                Instruction::StoreLocal { local: target, src } if *target == local => Some(*src),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            function.semantic.registers.get(&value.temp.index()),
            Some(expected),
            "{name} must receive the checked produced type, not the pre-coercion receiver type",
        );
    }
}

#[test]
fn declaration_extraction_precedes_cells_and_atomic_debug_publication() {
    let source = r#"
struct Pair { val left: (i32, i32), val right: i32 }
fn make() -> Pair { Pair { left: (20, 0), right: 22 } }
fn main() -> i32 {
    var Pair { right: second, left: (first, _) } = make();
    val next = || { first = first + 1; second = second + first; second };
    next()
}
"#;
    let checked = common::program_ok(source);
    let mir = lower_to_mir(checked.root(), &Default::default()).unwrap();
    let function = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let block = &function.blocks[function.entry.index()];
    let projections = block
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::ReadAggregateField { field, .. } => Some(format!("field:{}", field.slot)),
            Instruction::ReadAggregateIndex { .. } => Some("index".to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(projections, ["field:1", "field:0", "index"]);
    let extraction_end = block
        .instructions
        .iter()
        .rposition(|instruction| {
            matches!(
                instruction,
                Instruction::ReadAggregateField { .. } | Instruction::ReadAggregateIndex { .. }
            )
        })
        .unwrap();
    let cells = block
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(instruction, Instruction::MakeCell { .. }).then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(cells.len(), 2);
    assert!(cells.iter().all(|index| *index > extraction_end));

    let locals = function
        .debug
        .locals
        .iter()
        .filter(|local| matches!(local.name.as_str(), "first" | "second"))
        .collect::<Vec<_>>();
    assert_eq!(locals.len(), 2);
    for local in &locals {
        assert_eq!(&source[local.span.start..local.span.end], local.name);
        assert_eq!(local.ty, ValueType::HeapObject);
    }
    let publication_end = block.instructions.iter().rposition(|instruction| {
        matches!(instruction, Instruction::StoreLocal { local, .. } if locals.iter().any(|binding| binding.local == *local))
    }).unwrap();
    assert!(
        block.instruction_scopes[..=publication_end]
            .iter()
            .all(|scope| *scope == 0)
    );
    let mut scope = Some(block.instruction_scopes[publication_end + 1]);
    let mut visible = Vec::new();
    while let Some(index) = scope {
        let lexical = &function.debug.lexical_scopes[index];
        if let Some(local) = lexical.local {
            visible.push(function.locals[local.index()].name.as_str());
        }
        scope = lexical.parent;
    }
    visible.sort_unstable();
    assert_eq!(visible, ["first", "second"]);
}

#[test]
fn nested_irrefutable_alternatives_merge_chosen_values_before_capture_allocation() {
    let checked = common::program_ok(
        "fn main() -> i32 { \
         var (((0, picked) | (picked, _)), _) = ((4, 5), 0); \
         val next = || { picked = picked + 1; picked }; next() }",
    );
    let mir = lower_to_mir(checked.root(), &Default::default()).unwrap();
    let function = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    assert!(
        function.blocks.iter().all(|block| matches!(
            block.terminator,
            Some(Terminator::Jump(_) | Terminator::Branch { .. } | Terminator::Return(_))
        )),
        "an irrefutable declaration must not introduce a runtime failure path"
    );
    let instructions = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .collect::<Vec<_>>();
    let cells = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::MakeCell { value, .. } => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        cells.len(),
        1,
        "alternatives must share one captured-var cell"
    );
    let values = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::Move { dst, src } if *dst == cells[0] => Some(*src),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 2);
    let mut indices = values
        .iter()
        .map(|value| {
            let index = instructions
                .iter()
                .find_map(|instruction| match instruction {
                    Instruction::ReadAggregateIndex { dst, index, .. } if dst == value => {
                        Some(*index)
                    }
                    _ => None,
                })
                .unwrap();
            instructions
                .iter()
                .find_map(|instruction| match instruction {
                    Instruction::LoadConst {
                        dst,
                        constant: Constant::I32(value),
                    } if *dst == index => Some(*value),
                    _ => None,
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    // Block storage order is not execution order; both projection paths must survive.
    indices.sort_unstable();
    assert_eq!(
        indices,
        [0, 1],
        "each alternative must retain its own binding value path"
    );
}
