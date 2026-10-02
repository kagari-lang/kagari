use kagari_abi::{
    scalar::BuiltinType,
    types::{AbiType, ConcreteFunctionIdentity, ConstraintAbi, PublicAbiItem},
};
use kagari_bytecode::program::verify_program as Kagaribytecodeverify_program;
use kagari_mir::{
    codec::{decode_program, encode_program},
    instruction::{CallTarget, Instruction},
    program::{ProgramErrorKind, verify_program},
    verify::{MirVerificationError, MirVerificationErrorKind},
};

use kagari_common::{
    cancellation::CancellationToken,
    diagnostic::DiagnosticKind,
    identity::{FileId, ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{
    bytecode::{BytecodeLoweringError, lower_program_to_bytecode, lower_to_bytecode},
    source::{
        lower::{MirLoweringError, instances::MirLoweringOptions},
        program::{SourceProgramError, lower_program_to_mir},
    },
};
use kagari_hir::{
    analysis::AnalysisDatabase,
    program::{CheckedProgram, ProgramCheckError},
};

fn insert(db: &mut SourceDatabase, name: &str, text: &str) -> FileId {
    let path = format!("mem://{name}");
    db.bind_module(
        &path,
        ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec![name.into()],
        },
    )
    .unwrap();
    db.set(&path, text.into(), SourceLayer::Base).unwrap()
}
fn checked(db: &SourceDatabase, root: FileId) -> CheckedProgram {
    AnalysisDatabase::default()
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap()
}

#[test]
fn imported_generic_methods_have_distinct_program_instances_and_share_the_limit() {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "api",
        include_str!("../../../examples/imported-traits/api.kgr"),
    );
    insert(
        &mut db,
        "model",
        include_str!("../../../examples/imported-traits/generic-model.kgr"),
    );
    let root = insert(
        &mut db,
        "root",
        include_str!("../../../examples/imported-traits/generic-consumer.kgr"),
    );
    let checked = checked(&db, root);
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let wire = encode_program(&ir, &Default::default()).unwrap();
    let decoded = decode_program(&wire, &Default::default()).unwrap();
    assert_eq!(wire, encode_program(&decoded, &Default::default()).unwrap());
    assert_eq!(
        bincode::serialize(&lower_program_to_bytecode(&ir).unwrap()).unwrap(),
        bincode::serialize(&lower_program_to_bytecode(&decoded).unwrap()).unwrap(),
    );
    let model = ir
        .modules()
        .iter()
        .find(|module| module.identity.path == ["model"])
        .unwrap();
    let methods = model
        .functions
        .iter()
        .filter(|function| {
            function
                .instance
                .declaration
                .path
                .last()
                .is_some_and(|segment| segment.name == "get")
        })
        .collect::<Vec<_>>();
    assert_eq!(methods.len(), 2);
    assert!(methods.iter().any(|function| {
        function.instance.arguments == [AbiType::Builtin(BuiltinType::Bool)]
            && ir.function(&function.instance).is_some()
    }));
    assert!(methods.iter().any(|function| {
        function.instance.arguments == [AbiType::Builtin(BuiltinType::I32)]
            && ir.function(&function.instance).is_some()
    }));
    let mut forged = ir.clone().into_unverified();
    let contract = forged
        .iter_mut()
        .flat_map(|module| &mut module.functions)
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Call {
                callee: CallTarget::SourceFunction(contract),
                ..
            } if !contract.arguments.is_empty() => Some(contract),
            _ => None,
        })
        .unwrap();
    contract.arguments[0] = AbiType::Builtin(BuiltinType::F32);
    assert!(matches!(
        verify_program(ir.root().clone(), forged, &Default::default())
            .unwrap_err()
            .kind,
        ProgramErrorKind::UnresolvedFunction(_)
    ));
    let instance_count = ir
        .modules()
        .iter()
        .map(|module| {
            module
                .functions
                .iter()
                .filter(|function| !function.instance.arguments.is_empty())
                .count()
                + module
                    .structures
                    .iter()
                    .filter(|layout| !layout.arguments.is_empty())
                    .count()
                + module
                    .enumerations
                    .iter()
                    .filter(|layout| !layout.arguments.is_empty())
                    .count()
        })
        .sum::<usize>();
    let limit = instance_count - 1;
    let error = lower_program_to_mir(
        &checked,
        &MirLoweringOptions {
            max_generic_instances: limit,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SourceProgramError::Lowering { error: MirLoweringError::Diagnostic(diagnostic), .. }
            if matches!(diagnostic.kind, DiagnosticKind::CompileLimitExceeded {
                resource: "generic instances",
                limit: actual,
            } if actual == limit)
    ));
}

#[test]
fn public_abi_distinguishes_same_named_imported_types_and_constraints() {
    use kagari_bytecode::artifact::{ArtifactFingerprint, KbcArtifact};
    let mut db = SourceDatabase::default();
    for name in ["left", "right"] {
        insert(&mut db, name, "pub struct Item { val value: i32 }");
    }
    let mut artifacts = Vec::new();
    for side in ["Left", "Right"] {
        let source = format!(
            "use pkg::left::Item as Left; use pkg::right::Item as Right; trait LeftMarker {{}} trait RightMarker {{}} pub struct Wrap {{ val value: {side} }} pub fn expose(value: {side}) -> {side} {{ value }} pub trait Api {{ fn accept<T: {side}Marker>(self, value: T) -> T; }}"
        );
        let root = insert(&mut db, "root", &source);
        let ir = lower_program_to_mir(&checked(&db, root), &Default::default()).unwrap();
        let artifact =
            KbcArtifact::from_program(lower_program_to_bytecode(&ir).unwrap(), Default::default())
                .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        decoded.validate_for_loader(&Default::default()).unwrap();
        artifacts.push(decoded);
    }
    let roots = artifacts
        .iter()
        .map(|artifact| &artifact.program.modules[artifact.program.root.index()])
        .collect::<Vec<_>>();
    assert_eq!(roots[0].identity, roots[1].identity);
    assert_eq!(roots[0].dependencies, roots[1].dependencies);
    for name in ["Wrap", "expose", "Api"] {
        let a = roots[0]
            .public_items
            .iter()
            .find(|item| item.name() == name)
            .unwrap();
        let b = roots[1]
            .public_items
            .iter()
            .find(|item| item.name() == name)
            .unwrap();
        assert_ne!(
            ArtifactFingerprint::of_serialized(a),
            ArtifactFingerprint::of_serialized(b),
            "{name}"
        );
    }
    let PublicAbiItem::Trait(interface) = roots[0]
        .public_items
        .iter()
        .find(|item| item.name() == "Api")
        .unwrap()
    else {
        panic!("trait")
    };
    assert!(
        matches!(&interface.methods[0].bounds[0].constraints[0], ConstraintAbi::Trait(id) if id.declaration.module.path == ["root"] && id.declaration.path.last().unwrap().name == "LeftMarker")
    );
}

#[test]
fn public_generic_abi_ignores_binder_spelling_and_constraint_source_order() {
    let mut db = SourceDatabase::default();
    let mut items = Vec::new();
    for source in [
        "pub struct Box<T> { val value: T } pub trait Factory { fn id<T: Eq + Hash + PartialEq>(self, value: T) -> T; }",
        "pub struct Box<U> { val value: U } pub trait Factory { fn id<U>(self, value: U) -> U where U: PartialEq + Eq + Hash; }",
    ] {
        let root = insert(&mut db, "generic", source);
        let ir = lower_program_to_mir(&checked(&db, root), &Default::default()).unwrap();
        let program = lower_program_to_bytecode(&ir).unwrap();
        items.push(program.modules[program.root.index()].public_items.clone());
    }
    assert_eq!(items[0], items[1]);
}
fn fixture() -> CheckedProgram {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "shared",
        "fn id<T>(x: T) -> T { x } pub fn answer(x: i32) -> i32 { id(x) } pub fn flag(x: bool) -> i32 { if x { 1 } else { 0 } } pub struct Data { pub var x: i32 }",
    );
    insert(
        &mut db,
        "left",
        "pub use pkg::shared::answer; pub use pkg::shared::Data;",
    );
    insert(
        &mut db,
        "right",
        "pub use pkg::shared::answer; pub use pkg::shared::Data;",
    );
    insert(&mut db, "unrelated", "fn broken() -> i32 { false }");
    let root = insert(
        &mut db,
        "root",
        "use pkg::left::answer as a; use pkg::right::answer as b; use pkg::left::Data; fn answer() -> bool { true } fn main() -> i32 { val p = Data { x: a(20) }; p.x += b(22); p.x }",
    );
    checked(&db, root)
}

#[test]
fn generic_layouts_keep_arguments_across_facades_and_share_program_limits() {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "types",
        "pub struct Cell<T> { pub var value: T } pub enum Packet<T> { Data(T) } fn seed() { val c = Cell { value: true }; }",
    );
    insert(
        &mut db,
        "facade",
        "pub use pkg::types::Cell; pub use pkg::types::Packet;",
    );
    let root = insert(
        &mut db,
        "root",
        "use pkg::facade::Cell; use pkg::facade::Packet; fn main() -> Packet<i32> { val a = Cell { value: 7 }; val b = Cell { value: true }; Packet::Data(a.value) }",
    );
    let checked = checked(&db, root);
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let layouts = &ir
        .modules()
        .iter()
        .find(|module| module.identity.path == ["root"])
        .unwrap()
        .structures;
    assert_eq!(layouts.len(), 2);
    assert_eq!(layouts[0].declaration, layouts[1].declaration);
    assert_ne!(layouts[0].arguments, layouts[1].arguments);
    assert_ne!(layouts[0].fields[0].ty, layouts[1].fields[0].ty);
    let mut program = lower_program_to_bytecode(&ir).unwrap();
    Kagaribytecodeverify_program(&program).unwrap();
    // The owner need not execute an instance for its public template to be checked.
    // Installed standard enum layouts are independent of that user declaration.
    let owner = program
        .modules
        .iter_mut()
        .find(|module| module.identity.path == ["types"])
        .unwrap();
    assert!(
        owner
            .enumerations
            .iter()
            .all(|layout| layout.declaration.module.package == PackageId("kagari-core".into()))
    );
    let PublicAbiItem::Type(template) = owner
        .public_items
        .iter_mut()
        .find(|item| item.name() == "Packet")
        .unwrap()
    else {
        unreachable!()
    };
    template.variants[0].payload[0] = AbiType::Builtin(BuiltinType::Bool);
    assert!(Kagaribytecodeverify_program(&program).is_err());
    let error = lower_program_to_mir(
        &checked,
        &MirLoweringOptions {
            max_generic_instances: 3,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, SourceProgramError::Lowering { error: MirLoweringError::Diagnostic(d), .. } if matches!(d.kind, DiagnosticKind::CompileLimitExceeded { resource: "generic instances", limit: 3 }))
    );
}

#[test]
fn source_program_keeps_module_identity_and_resolves_transitive_call_contracts() {
    let checked = fixture();
    assert_eq!(
        checked
            .modules()
            .iter()
            .filter(
                |module| module.lowered.source.module_identity().package == PackageId("pkg".into())
            )
            .map(|module| module.lowered.source.module_identity().path[0].as_str())
            .collect::<Vec<_>>(),
        ["left", "right", "root", "shared"]
    );
    let program = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let root = program
        .modules()
        .iter()
        .find(|module| module.identity.path == ["root"])
        .unwrap();
    let calls = root
        .functions
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.instructions)
        .filter_map(|i| {
            if let Instruction::Call {
                callee: CallTarget::SourceFunction(contract),
                ..
            } = i
            {
                Some(contract)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    for call in calls {
        assert_eq!(call.declaration.module.path, ["shared"]);
        let binding = program
            .function(&ConcreteFunctionIdentity {
                declaration: call.declaration.clone(),
                arguments: call.arguments.clone(),
            })
            .unwrap();
        let target = &program.modules()[binding.module].functions[binding.function.index()];
        assert_eq!(target.name, "answer");
        assert_eq!(target.instance.declaration, call.declaration);
    }
    assert!(
        program
            .modules()
            .iter()
            .find(|module| module.identity.path == ["shared"])
            .unwrap()
            .functions
            .iter()
            .any(|f| !f.instance.arguments.is_empty())
    );
    assert!(matches!(
        lower_to_bytecode(root),
        Err(BytecodeLoweringError::UnlinkedSourceModules)
    ));
    // Imported bindings still require whole-program linking.
    let shared = program
        .modules()
        .iter()
        .find(|module| module.identity.path == ["shared"])
        .unwrap();
    assert!(matches!(
        lower_to_bytecode(shared),
        Err(BytecodeLoweringError::UnlinkedSourceModules)
    ));
}

#[test]
fn missing_dependencies_and_mismatched_link_signatures_are_rejected() {
    let program = lower_program_to_mir(&fixture(), &Default::default()).unwrap();
    let root = program.root().clone();
    let raw = program.into_unverified();
    let mut schema = raw.clone();
    schema
        .iter_mut()
        .find(|module| module.identity.path == ["shared"])
        .unwrap()
        .structures[0]
        .fields[0]
        .mutable = false;
    assert!(matches!(
        verify_program(root.clone(), schema, &Default::default())
            .unwrap_err()
            .kind,
        ProgramErrorKind::Verification(MirVerificationError {
            kind: MirVerificationErrorKind::InvalidStructLayout,
            ..
        })
    ));
    let mut unresolved = raw.clone();
    for instruction in unresolved
        .iter_mut()
        .find(|module| module.identity.path == ["root"])
        .unwrap()
        .functions
        .iter_mut()
        .flat_map(|f| &mut f.blocks)
        .flat_map(|b| &mut b.instructions)
    {
        if let Instruction::Call {
            callee: CallTarget::SourceFunction(contract),
            ..
        } = instruction
        {
            contract.declaration.path.last_mut().unwrap().name = "absent".into();
        }
    }
    assert!(matches!(
        verify_program(root.clone(), unresolved, &Default::default())
            .unwrap_err()
            .kind,
        ProgramErrorKind::UnresolvedFunction(_)
    ));
    let mut missing = raw.clone();
    missing.remove(0);
    assert!(matches!(
        verify_program(root.clone(), missing, &Default::default())
            .unwrap_err()
            .kind,
        ProgramErrorKind::InvalidGraph
    ));
    let mut cycle = raw.clone();
    cycle
        .iter_mut()
        .find(|module| module.identity.path == ["shared"])
        .unwrap()
        .dependencies
        .push(root.clone());
    assert!(verify_program(root.clone(), cycle, &Default::default()).is_ok());
    let mut wrong = raw.clone();
    let other = wrong
        .iter()
        .find(|module| module.identity.path == ["shared"])
        .unwrap()
        .functions
        .iter()
        .find(|f| f.name == "flag")
        .unwrap()
        .instance
        .declaration
        .clone();
    for instruction in wrong
        .iter_mut()
        .find(|module| module.identity.path == ["root"])
        .unwrap()
        .functions
        .iter_mut()
        .flat_map(|f| &mut f.blocks)
        .flat_map(|b| &mut b.instructions)
    {
        if let Instruction::Call {
            callee: CallTarget::SourceFunction(contract),
            ..
        } = instruction
        {
            contract.declaration = other.clone();
        }
    }
    assert!(
        matches!(verify_program(root.clone(), wrong, &Default::default()).unwrap_err().kind, ProgramErrorKind::FunctionContract(id) if id == other)
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        verify_program(root, raw, &cancel).unwrap_err().kind,
        ProgramErrorKind::Cancelled
    ));
}

#[test]
fn program_limits_are_shared_across_modules() {
    let checked = fixture();
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let count = ir
        .modules()
        .iter()
        .flat_map(|m| &m.functions)
        .flat_map(|f| &f.blocks)
        .map(|b| b.instructions.len() + usize::from(b.terminator.is_some()))
        .sum::<usize>();
    let error = lower_program_to_mir(
        &checked,
        &MirLoweringOptions {
            max_instructions: count - 1,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, SourceProgramError::Lowering { error: MirLoweringError::Diagnostic(d), .. } if matches!(d.kind, DiagnosticKind::CompileLimitExceeded {resource: "generated instructions", ..}))
    );
}

#[test]
fn dependency_diagnostics_and_function_targets_belong_to_the_checked_snapshot() {
    let mut db = SourceDatabase::default();
    let dependency = insert(&mut db, "dependency", "pub fn value() -> i32 { 1 }");
    let root = insert(
        &mut db,
        "root",
        "use pkg::dependency::value; fn main() -> i32 { value() }",
    );
    let mut analysis = AnalysisDatabase::default();
    let old = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let file = old.file(root).unwrap();
    let target = file
        .source_function_at(file.source().text().rfind("value()").unwrap())
        .unwrap()
        .id;
    let old_program = old.check_program(root, &Default::default()).unwrap();
    assert!(old_program.source_function(target).is_some());
    db.set(
        "mem://dependency",
        "pub fn value() -> i32 { 2 }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    assert!(checked(&db, root).source_function(target).is_none());
    assert!(old_program.source_function(target).is_some());
    db.set(
        "mem://dependency",
        "pub fn value() -> i32 { 2 } fn broken() -> i32 { false }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let current = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(
        current
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    let ProgramCheckError::Diagnostics(records) = current
        .check_program(root, &Default::default())
        .unwrap_err()
    else {
        panic!("expected dependency error")
    };
    assert!(records.iter().all(|record| record.file == dependency
        && record.revision == current.file(dependency).unwrap().source().revision()));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        current.check_program(root, &cancel),
        Err(ProgramCheckError::Cancelled)
    ));
}

#[test]
fn portable_default_method_origins_remain_with_the_definition_module() {
    use kagari_common::{line_index::PositionEncoding, source::SourceFile};
    let mut db = SourceDatabase::default();
    let model = "pub trait Read { fn read(self) -> i32;\n fn again(self) -> i32 { self.read() }\n}";
    insert(&mut db, "model", model);
    let root = insert(
        &mut db,
        "root",
        "use pkg::model::Read; struct Holder { val value: i32 } impl Read for Holder { fn read(self) -> i32 { self.value } } fn main() -> i32 { val x: Read = Holder { value: 7 }; x.again() }",
    );
    let checked = checked(&db, root);
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let root_module = ir
        .modules()
        .iter()
        .find(|module| module.identity.path == ["root"])
        .unwrap();
    let inherited = root_module
        .functions
        .iter()
        .find(|function| function.instance.declaration.path.last().unwrap().name == "again")
        .unwrap();
    assert_eq!(inherited.instance.declaration.module.path, ["root"]);
    assert_eq!(
        inherited.debug.source_module.as_ref().unwrap().path,
        ["model"]
    );
    assert_eq!(inherited.debug.source.as_ref().unwrap().uri, "mem://model");
    drop(checked);
    drop(db);
    let bytecode = lower_program_to_bytecode(&ir).unwrap();
    let root_module = bytecode
        .modules
        .iter()
        .find(|module| module.identity.path == ["root"])
        .unwrap();
    let debug = &root_module.functions[inherited.id.index()].metadata.debug;
    assert_eq!(
        bytecode.modules[debug.source_module.unwrap().index()]
            .identity
            .path,
        ["model"]
    );
    assert_eq!(debug.source_uri.as_deref(), Some("mem://model"));
    let source = SourceFile::new("mem://model", model);
    for entry in &debug.line_table {
        let position = source
            .position(entry.source_offset, PositionEncoding::Utf8)
            .unwrap();
        assert_eq!(
            (entry.line, entry.column),
            (
                Some(position.line as u32 + 1),
                Some(position.character as u32 + 1)
            )
        );
    }
}

#[test]
fn portable_inline_module_origins_keep_physical_offsets() {
    use kagari_common::{line_index::PositionEncoding, source::SourceFile};
    let text = "// 中文😀\r\nmod child {\r\n pub fn value() -> i32 { 42 }\r\n}\r\nuse self::child::value; fn main() -> i32 { value() }";
    let mut db = SourceDatabase::default();
    let root = insert(&mut db, "root", text);
    let checked = checked(&db, root);
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let child = ir
        .modules()
        .iter()
        .find(|module| module.identity.path == ["root", "child"])
        .unwrap();
    let function = &child.functions[0];
    let origin = function.debug.source.as_ref().unwrap();
    assert_eq!(origin.uri, "mem://root");
    assert_eq!(
        function.debug.source_module.as_ref().unwrap().path,
        ["root", "child"]
    );
    let source = SourceFile::new("mem://root", text);
    for span in function.source_spans() {
        let expected = source.position(span.start, PositionEncoding::Utf8).unwrap();
        let actual = origin.position(span.start).unwrap();
        assert_eq!(
            (actual.line, actual.column),
            (
                Some(expected.line as u32 + 1),
                Some(expected.character as u32 + 1)
            )
        );
    }
    let value = text.find("42").unwrap();
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| &block.instruction_spans)
            .any(|span| span.start == value)
    );
    drop(checked);
    drop(db);
    let bytecode = lower_program_to_bytecode(&ir).unwrap();
    let child = bytecode
        .modules
        .iter()
        .find(|module| module.identity.path == ["root", "child"])
        .unwrap();
    assert_eq!(
        child.functions[0].metadata.debug.source_uri.as_deref(),
        Some("mem://root")
    );
    assert!(
        child.functions[0]
            .metadata
            .debug
            .line_table
            .iter()
            .any(|entry| entry.source_offset == value && entry.line == Some(3))
    );
}
