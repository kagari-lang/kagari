//! Exact instruction provenance survives selection, encoding and stripped debugging.
use super::loaded_function;
use crate::{
    Runtime, error::RuntimeErrorKind, frame::cursor::kernel::RegionError, module::VerifiedProgram,
    value::Value,
};
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    instruction::{BytecodeInstruction, JumpTarget},
    module::{BytecodeFunction, FunctionMetadata, InstructionOrigin, LineTableEntry},
};
use kagari_common::{identity::map::DefinitionContext, span::Span};
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{lower::instances::MirLoweringOptions, program::lower_program_to_mir},
};
use kagari_contract::ids::FunctionRef;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

#[test]
fn selected_fault_origin_survives_stripped_source_free_execution() {
    let text = "pub fn probe(x: i32, y: i32) -> i32 { x + {-y}}";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("selected-origin.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let options = MirLoweringOptions {
        optimization: None,
        ..Default::default()
    };
    let mir = lower_program_to_mir(&checked, &options).unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let mut code = code
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let function = code.modules[code.root.index()]
        .functions
        .iter_mut()
        .find(|function| function.name == "probe")
        .unwrap();
    let expected = Span::new(text.find("-y").unwrap(), text.find("-y").unwrap() + 2);
    let (pc, _) = function
        .instructions
        .iter()
        .enumerate()
        .find(|(_, instruction)| matches!(instruction, BytecodeInstruction::Unary { .. }))
        .unwrap();
    let (id, origin) = function.metadata.instruction_origin(pc).unwrap();
    assert_eq!(origin.span, expected);
    assert_ne!(
        function
            .metadata
            .origins
            .iter()
            .position(|origin| origin.instruction == pc),
        Some(id),
        "the removed outer load must not supply the fault origin"
    );
    let entry = function.id;
    function.metadata.debug = Default::default();
    let artifact =
        KbcArtifact::from_program(code, ArtifactBuildOptions::default(), &context).unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
    let mut runtime = Runtime::default();
    let verified = decoded
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records()
        .into_verified(&Default::default(), &context)
        .unwrap();
    let loaded = runtime
        .load_verified_program(
            "selected-origin",
            VerifiedProgram::from_bytecode(verified.into_bytecode()),
        )
        .unwrap();
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push(
            &runtime,
            loaded.slot(),
            entry,
            &[Value::I32(0), Value::I32(i32::MIN)],
            None,
        )
        .unwrap();
    let Err(error) = stack.execute_region(&runtime, &mut None, |_, _, _, _| unreachable!()) else {
        panic!("the selected negation must overflow");
    };
    assert!(
        matches!(error, RegionError::Runtime(ref error) if error.kind() == RuntimeErrorKind::ScriptTrap)
    );
    let trace = runtime.capture_error_trace();
    assert_eq!(trace.frames[0].source_span, Some(expected));
    assert_eq!(trace.frames[0].semantic_origin, Some(id));
    assert_eq!(trace.frames[0].instruction_offset, pc);
    drop(stack);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn native_origin_does_not_borrow_missing_coordinates_from_another_origin() {
    let mut runtime = Runtime::default();
    let module = loaded_function(
        &mut runtime,
        BytecodeFunction {
            name: "main".into(),
            metadata: FunctionMetadata {
                origins: vec![
                    InstructionOrigin {
                        instruction: 0,
                        span: Span::new(10, 12),
                        line: Some(2),
                        column: Some(3),
                    },
                    InstructionOrigin {
                        instruction: 0,
                        span: Span::new(40, 42),
                        line: None,
                        column: None,
                    },
                    InstructionOrigin {
                        instruction: 1,
                        span: Span::new(50, 52),
                        line: None,
                        column: None,
                    },
                ],
                instruction_origins: vec![1],
                debug: kagari_bytecode::module::BytecodeDebugMetadata {
                    line_table: vec![LineTableEntry {
                        instruction_offset: 0,
                        source_offset: 10,
                        line: Some(2),
                        column: Some(3),
                    }],
                    ..Default::default()
                },
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::Jump {
                    target: JumpTarget::new(1),
                },
                BytecodeInstruction::Return(None),
            ],
            ..Default::default()
        },
    );
    let stack = runtime.enter_execution_stack(&module).unwrap();
    stack
        .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    stack
        .current_mut()
        .unwrap()
        .set_native_instruction(&runtime, 1)
        .unwrap();
    let trace = runtime.capture_error_trace();
    assert_eq!(trace.frames[0].semantic_origin, Some(1));
    assert_eq!(trace.frames[0].source_span, Some(Span::new(40, 42)));
    assert_eq!((trace.frames[0].line, trace.frames[0].column), (None, None));
}
