use super::{checked, insert};
use crate::{
    bytecode::lower_program_to_bytecode,
    source::{lower::module_contract, program::lower_program_to_mir},
};
use kagari_common::identity::DefinitionKind;
use kagari_contract::types::PublicItem;
use kagari_mir::{
    codec::{decode_program, encode_program},
    function::MirModule,
    instruction::{Instruction, Terminator},
    program::VerifiedMirProgram,
};
use kagari_source::source_database::SourceDatabase;

fn lower(files: &[(&str, &str)]) -> VerifiedMirProgram {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in files {
        let file = insert(&mut sources, name, text);
        if *name == "root" {
            root = Some(file);
        }
    }
    let program = checked(&sources, root.unwrap());
    let contract = module_contract(program.root(), &Default::default()).unwrap();
    let mir = lower_program_to_mir(&program, &Default::default()).unwrap();
    let public_statics = |items: &[PublicItem]| {
        items
            .iter()
            .filter_map(|item| match item {
                PublicItem::Static(contract) => Some(contract.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        public_statics(&contract.records().public_items),
        public_statics(
            &mir.modules()
                .iter()
                .find(|module| &module.identity == mir.root())
                .unwrap()
                .abi
                .public_items
        ),
    );
    mir
}

fn module<'a>(program: &'a VerifiedMirProgram, path: &[&str]) -> &'a MirModule {
    program
        .modules()
        .iter()
        .find(|module| module.identity.path == path)
        .unwrap()
}

#[test]
fn static_descriptors_preserve_suppliers_hidden_initializers_and_source_free_products() {
    let mir = lower(&[
        (
            "dep",
            "static val PRIVATE: i32 = 7; pub static var COUNT: i32 = 0;",
        ),
        ("facade", "pub use pkg::dep::COUNT as ALIAS;"),
        (
            "root",
            "use pkg::facade::ALIAS; mod nested { static val UNUSED: () = (); } fn run() -> i32 { ALIAS += 1; ALIAS }",
        ),
    ]);
    let supplier = module(&mir, &["dep"]);
    let consumer = module(&mir, &["root"]);
    assert_eq!(supplier.module_slots.len(), 2);
    assert_eq!(consumer.module_slots.len(), 1);
    assert_eq!(consumer.module_slots[0], supplier.module_slots[1]);
    assert_eq!(module(&mir, &["root", "nested"]).module_slots.len(), 1);
    assert!(
        supplier
            .abi
            .public_items
            .iter()
            .any(|item| matches!(item, PublicItem::Static(contract) if contract.name == "COUNT"))
    );
    assert!(
        !supplier
            .abi
            .public_items
            .iter()
            .any(|item| matches!(item, PublicItem::Static(contract) if contract.name == "PRIVATE"))
    );
    for owner in mir.modules() {
        for slot in &owner.module_slots {
            if slot.contract.supplier != owner.identity {
                continue;
            }
            let initializer = &owner.functions[slot.initializer.index()];
            assert!(initializer.params.is_empty());
            assert_eq!(
                initializer.semantic.result.as_ref(),
                Some(&slot.contract.ty)
            );
            assert!(!initializer.effects.may_suspend);
            let identity = mir
                .definitions()
                .resolve(initializer.instance.declaration)
                .unwrap();
            assert_eq!(identity.segments().count(), 1);
            assert_eq!(identity.last().unwrap().kind, DefinitionKind::Function);
            assert!(
                slot.contract.accepts_initializer_identity(
                    initializer.instance.declaration,
                    mir.definitions()
                )
            );
            assert!(!owner.abi.public_items.iter().any(|item| matches!(item, PublicItem::Function(function) if function.name == initializer.name)));
        }
    }
    let bytes = encode_program(&mir, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    assert_eq!(
        bytes,
        encode_program(&decoded, &Default::default()).unwrap()
    );
    for input in [&mir, &decoded] {
        let bytecode = lower_program_to_bytecode(input, &Default::default()).unwrap();
        let supplier = bytecode
            .program()
            .modules
            .iter()
            .find(|module| module.identity.path == ["dep"])
            .unwrap();
        let consumer = bytecode
            .program()
            .modules
            .iter()
            .find(|module| module.identity.path == ["root"])
            .unwrap();
        assert_eq!(consumer.module_slots[0], supplier.module_slots[1]);
        for slot in &supplier.module_slots {
            let initializer = &supplier.functions[slot.initializer.index()];
            assert!(initializer.metadata.params.is_empty());
        }
    }
}

#[test]
fn unused_initializers_retain_closures_generics_coercions_and_static_body_captures() {
    let mir = lower(&[
        ("dep", "pub fn identity<T>(value: T) -> T { value }"),
        (
            "root",
            r#"
            use pkg::dep::identity;
            trait Read { fn read(self) -> i32; }
            struct Box { val value: i32 }
            impl Read for Box { fn read(self) -> i32 { self.value } }
            static val VIEW: Read = Box { value: identity(42) };
            static val CALLBACK: fn() -> i32 = {
                var count = identity(40);
                || { count += 1; count }
            };
        "#,
        ),
    ]);
    let root = module(&mir, &["root"]);
    let callback = root
        .module_slots
        .iter()
        .find(|slot| slot.contract.name == "CALLBACK")
        .unwrap();
    let initializer = &root.functions[callback.initializer.index()];
    let instructions: Vec<_> = initializer
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .collect();
    assert!(
        instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::MakeCell { .. }))
    );
    let closure = instructions
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::MakeClosure {
                function, captures, ..
            } if captures.len() == 1 => Some(*function),
            _ => None,
        })
        .expect("initializer closure retains its mutable local cell");
    let closure = &root.functions[closure.index()];
    assert!(
        closure
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| matches!(instruction, Instruction::WriteCell { .. }))
    );
    assert_eq!(closure.debug.captured_bindings[0].name, "count");
    assert!(
        module(&mir, &["dep"])
            .functions
            .iter()
            .any(|function| !function.instance.arguments.is_empty())
    );
    let view = root
        .module_slots
        .iter()
        .find(|slot| slot.contract.name == "VIEW")
        .unwrap();
    let initializer = &root.functions[view.initializer.index()];
    assert_eq!(
        initializer.semantic.result.as_ref(),
        Some(&view.contract.ty)
    );
    lower_program_to_bytecode(&mir, &Default::default()).unwrap();

    // A module with no authored function still owns real initializer/adapter bodies.
    let only_statics = lower(&[(
        "root",
        "static val COMPARE: bool = (1, 2) == (1, 2); static val EMPTY: () = ();",
    )]);
    lower_program_to_bytecode(&only_statics, &Default::default()).unwrap();
}

#[test]
fn static_assignment_loads_precede_rhs_and_preserve_the_compound_left_operand() {
    let mir = lower(&[(
        "root",
        r#"
        static var COUNT: i32 = 0;
        fn rhs() -> i32 { COUNT = 10; 2 }
        fn plain() { COUNT = rhs(); }
        fn compound() { COUNT += rhs(); }
    "#,
    )]);
    let root = module(&mir, &["root"]);
    for name in ["plain", "compound"] {
        let function = root
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        let instructions: Vec<_> = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .collect();
        let (load_index, loaded) = instructions
            .iter()
            .enumerate()
            .find_map(|(index, instruction)| match instruction {
                Instruction::LoadModule { dst, .. } => Some((index, *dst)),
                _ => None,
            })
            .unwrap();
        let call_index = instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::Call { .. }))
            .unwrap();
        let store_index = instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::StoreModule { .. }))
            .unwrap();
        assert!(load_index < call_index && call_index < store_index);
        assert_eq!(
            instructions
                .iter()
                .filter(|instruction| matches!(instruction, Instruction::LoadModule { .. }))
                .count(),
            1
        );
        if name == "compound" {
            assert!(instructions.iter().any(|instruction| matches!(instruction, Instruction::Binary { lhs, .. } if *lhs == loaded)));
        }
        assert!(
            function
                .blocks
                .iter()
                .any(|block| matches!(block.terminator, Some(Terminator::Return(_))))
        );
    }
    lower_program_to_bytecode(&mir, &Default::default()).unwrap();
}

#[test]
fn static_roots_are_captured_early_but_projections_are_read_after_rhs() {
    let mir = lower(&[(
        "root",
        r#"
        struct Box { var values: [i32] }
        static var BOX: Box = Box { values: [0] };
        fn index() -> u64 { BOX = Box { values: [1] }; 0u64 }
        fn rhs() -> i32 { BOX = Box { values: [2] }; 3 }
        fn update() { BOX.values[index()] = rhs(); }
    "#,
    )]);
    let root = module(&mir, &["root"]);
    let function = root
        .functions
        .iter()
        .find(|function| function.name == "update")
        .unwrap();
    let instructions: Vec<_> = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .collect();
    let load = instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::LoadModule { .. }))
        .unwrap();
    let receiver = instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::ReadAggregateField { .. }))
        .unwrap();
    let calls: Vec<_> = instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(instruction, Instruction::Call { .. }).then_some(index)
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(load < calls[0] && calls[0] < calls[1] && calls[1] < receiver);
    assert_eq!(
        instructions
            .iter()
            .filter(|instruction| matches!(instruction, Instruction::LoadModule { .. }))
            .count(),
        1
    );
    assert_eq!(
        instructions
            .iter()
            .filter(|instruction| matches!(instruction, Instruction::ReadAggregateField { .. }))
            .count(),
        1
    );
    lower_program_to_bytecode(&mir, &Default::default()).unwrap();
}
