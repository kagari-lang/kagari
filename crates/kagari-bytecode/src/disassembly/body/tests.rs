use crate::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    disassembly::{
        PreparedDisassembly, body::write_body, presentation::DisplayContext, resolution::Resolution,
    },
    instruction::{BytecodeInstruction, JumpTarget, Slot},
    module::{
        BytecodeFunction, BytecodeModule, CapturedBindingDebugInfo, FrameLayout, FunctionMetadata,
        FunctionRecord, InstructionSourceSpan, LineTableEntry, LocalLiveRange, RootSlotLayout,
        SafeDebugPoint, SafeDebugPointKind,
    },
    program::{BytecodeProgram, ModuleRef},
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    identity::{ModuleIdentity, PackageId, map::DefinitionContext},
    span::Span,
};
use kagari_contract::ids::{DebugPointId, FunctionRef};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::io::{self, Write};

fn function() -> BytecodeFunction {
    {
        let parameter_base: usize = 1;
        let fixture_instructions = vec![
            BytecodeInstruction::Move {
                dst: Slot::new(0),
                src: Slot::new(1),
            },
            BytecodeInstruction::Move {
                dst: Slot::new(2),
                src: Slot::new(0),
            },
            BytecodeInstruction::Jump {
                target: JumpTarget::new(4),
            },
            BytecodeInstruction::Return(Some(Slot::new(0))),
            BytecodeInstruction::Move {
                dst: Slot::new(0),
                src: Slot::new(2),
            },
            BytecodeInstruction::Return(Some(Slot::new(0))),
        ];
        let mut fixture_metadata: crate::module::FunctionMetadata = FunctionMetadata {
            params: vec![ValueType::I32],
            return_type: ValueType::I32,
            slots: [vec![ValueType::I32], vec![ValueType::I32; 2]].concat(),
            ..Default::default()
        };
        if fixture_metadata.parameter_slots.is_empty() {
            fixture_metadata.parameter_slots =
                (0..1).map(|i| Slot::new(parameter_base + i)).collect();
        }
        if fixture_metadata.origins.is_empty() {
            fixture_metadata.origins = (0..fixture_instructions.len())
                .map(|instruction| crate::module::InstructionOrigin {
                    span: Default::default(),
                    line: None,
                    column: None,
                    instruction,
                })
                .collect();
        }
        BytecodeFunction {
            name: "echo".into(),
            parameter_count: 1,
            slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
            metadata: fixture_metadata,
            instructions: fixture_instructions,
            ..Default::default()
        }
    }
}

// Private source-free fixtures cross the real envelope/import/verifier boundary.
// Rendering never receives an unchecked test function or a source compiler product.
fn prepare(function: BytecodeFunction) -> PreparedDisassembly {
    let context = DefinitionContext::new().unwrap();
    let module = BytecodeModule {
        identity: ModuleIdentity {
            package: PackageId("fixture".into()),
            path: vec!["body".into()],
        },
        source_name: "missing/disassembly/source.kgr".into(),
        types: vec![
            ValueType::I32,
            ValueType::Bool,
            ValueType::Str,
            ValueType::HeapObject,
        ],
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            identity: function.identity.clone(),
            name: function.name.clone(),
            params: function.metadata.params.clone(),
            return_type: function.metadata.return_type,
            effects: function.metadata.effects,
        }],
        functions: vec![function],
        ..Default::default()
    };
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![module],
            exports: Default::default(),
        },
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    drop(artifact);
    drop(context);
    PreparedDisassembly::from_bytes(&bytes).unwrap()
}

fn render(prepared: &PreparedDisassembly) -> String {
    let mut output = Vec::new();
    write_body(
        &mut output,
        &DisplayContext {
            definitions: prepared.program().definitions(),
        },
        &Resolution::new(
            prepared.program().program(),
            prepared.program().definitions(),
        ),
        0,
        0,
    )
    .unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn no_debug_body_preserves_stored_order_unreachable_code_and_execution_facts() {
    let mut body = function();
    body.metadata
        .semantic
        .slots
        .insert(1, Ty::Builtin(BuiltinType::I32));
    body.metadata
        .semantic
        .slots
        .insert(0, Ty::Builtin(BuiltinType::I32));
    let prepared = prepare(body);
    let output = render(&prepared);
    assert_eq!(
        output,
        include_str!("../../../tests/fixtures/disassembly/no-debug-body.txt")
    );
    assert_eq!(output, render(&prepared));
}

#[test]
fn span_only_and_source_offset_locations_do_not_require_a_uri_or_source_reads() {
    let mut body = function();
    body.metadata.debug.source_module = Some(ModuleRef::new(0));
    body.metadata.debug.source_spans = vec![
        InstructionSourceSpan {
            instruction_offset: 4,
            span: Span::new(20, 28),
        },
        InstructionSourceSpan {
            instruction_offset: 0,
            span: Span::default(),
        },
    ];
    body.metadata.debug.line_table = vec![LineTableEntry {
        instruction_offset: 1,
        source_offset: 11,
        line: None,
        column: None,
    }];
    let output = render(&prepare(body));
    assert!(output.contains(
        "debug (nonauthoritative): uri=unknown source_module=@0 function_span=unknown\n"
    ));
    assert!(output.contains("; debug span#1: instruction=0 source_span=unknown\n  0000  move"));
    assert!(output.contains("; debug line#0: instruction=1 source_offset=11\n  0001  move"));
    assert!(
        output
            .contains("L4:\n    ; debug span#0: instruction=4 source_span=[20, 28)\n  0004  move")
    );
    assert!(!output.contains("line="));
    assert!(!output.contains("missing/disassembly"));
    assert!(!output.contains("ambiguous"));
}

#[test]
fn duplicate_unsorted_locations_and_debug_points_keep_original_record_order() {
    let mut body = function();
    let debug = &mut body.metadata.debug;
    debug.source_uri = Some("https://invalid.example/no-source\n\u{1b}\u{202e}".into());
    debug.function_span = Span::new(3, 30);
    debug.line_table = vec![
        LineTableEntry {
            instruction_offset: 4,
            source_offset: 20,
            line: Some(9),
            column: Some(7),
        },
        LineTableEntry {
            instruction_offset: 0,
            source_offset: 3,
            line: Some(1),
            column: None,
        },
        LineTableEntry {
            instruction_offset: 4,
            source_offset: 24,
            line: Some(8),
            column: Some(2),
        },
        LineTableEntry {
            instruction_offset: 4,
            source_offset: 20,
            line: Some(9),
            column: Some(7),
        },
    ];
    debug.source_spans = vec![
        InstructionSourceSpan {
            instruction_offset: 4,
            span: Span::new(20, 28),
        },
        InstructionSourceSpan {
            instruction_offset: 0,
            span: Span::new(3, 5),
        },
        InstructionSourceSpan {
            instruction_offset: 4,
            span: Span::new(24, 26),
        },
    ];
    debug.safe_debug_points = vec![
        SafeDebugPoint {
            id: DebugPointId::new(8),
            instruction_offset: 4,
            span: Span::new(20, 28),
            kind: SafeDebugPointKind::BranchTarget,
        },
        SafeDebugPoint {
            id: DebugPointId::new(2),
            instruction_offset: 0,
            span: Span::new(3, 5),
            kind: SafeDebugPointKind::FunctionEntry,
        },
        SafeDebugPoint {
            id: DebugPointId::new(8),
            instruction_offset: 4,
            span: Span::new(24, 26),
            kind: SafeDebugPointKind::Statement,
        },
    ];
    debug.captured_bindings.push(CapturedBindingDebugInfo {
        name: "captured\n\u{202e}".into(),
        span: Span::new(6, 8),
        ty: ValueType::Str,
    });
    let prepared = prepare(body);
    let output = render(&prepared);
    assert!(output.contains(
        "uri=\"https://invalid.example/no-source\\n\\u{1b}\\u{202e}\" function_span=[3, 30)"
    ));
    assert!(!output.contains(['\u{1b}', '\u{202e}']));
    assert!(output.contains("capture#0: name=\"captured\\n\\u{202e}\" debug_ty=str span=[6, 8) (nonauthoritative; no slot mapping)"));
    assert!(output.contains("debug location ambiguous at 4"));
    assert!(!output.contains("debug location ambiguous at 0"));
    let lines: Vec<_> = output
        .lines()
        .filter(|line| line.contains("; debug line#"))
        .collect();
    assert_eq!(
        lines,
        vec![
            "    ; debug line#1: instruction=0 line=1 source_offset=3",
            "    ; debug line#0: instruction=4 line=9 column=7 source_offset=20",
            "    ; debug line#2: instruction=4 line=8 column=2 source_offset=24",
            "    ; debug line#3: instruction=4 line=9 column=7 source_offset=20",
        ]
    );
    let spans: Vec<_> = output
        .lines()
        .filter(|line| line.contains("; debug span#"))
        .collect();
    assert_eq!(
        spans,
        vec![
            "    ; debug span#1: instruction=0 source_span=[3, 5)",
            "    ; debug span#0: instruction=4 source_span=[20, 28)",
            "    ; debug span#2: instruction=4 source_span=[24, 26)",
        ]
    );
    let points: Vec<_> = output
        .lines()
        .filter(|line| line.contains("; debug point#"))
        .collect();
    assert_eq!(
        points,
        vec![
            "    ; debug point#1: id=2 instruction=0 kind=function_entry span=[3, 5)",
            "    ; debug point#0: id=8 instruction=4 kind=branch_target span=[20, 28) (multiple matches; ambiguous)",
            "    ; debug point#2: id=8 instruction=4 kind=statement span=[24, 26) (multiple matches; ambiguous)",
        ]
    );
    assert_eq!(output, render(&prepared));
}

fn range(local: usize, name: &str, start: usize, end: usize) -> LocalLiveRange {
    LocalLiveRange {
        slot: Slot::new(local + 1),
        name: name.into(),
        span: Span::default(),
        ty: ValueType::Bool,
        is_parameter: local != 0,
        binding: 0,
        start,
        end,
    }
}

#[test]
fn overlapping_reused_locals_keep_half_open_names_and_nonauthoritative_claims() {
    let mut body = function();
    body.metadata.debug.local_live_ranges = vec![
        range(0, "late", 4, 6),
        range(0, "early\n\u{202e}", 0, 4),
        range(0, "overlap", 2, 5),
        range(0, "overlap", 2, 5),
        range(1, "not_an_argument", 2, 6),
        range(0, "empty", 4, 4),
    ];
    body.metadata.debug.frame_layout = FrameLayout {
        params: vec![ValueType::Str; 3],
        slots: [vec![ValueType::Str], vec![ValueType::Bool]].concat(),
    };
    let output = render(&prepare(body));
    assert!(output.contains("  entry: arg0 -> s1\n"));
    assert!(!output.contains("arg1"));
    assert!(output.contains("  s1: physical=i32 semantic=unknown\n"));
    assert!(output.contains("  s2: physical=i32 semantic=unknown\n"));
    assert!(output.contains("  s0: physical=i32 semantic=unknown\n"));
    assert!(output.contains("local ranges (nonauthoritative debug_ty/is_parameter; half-open):"));
    assert!(output.contains("range#1: s1 name=\"early\\n\\u{202e}\" instructions=[0, 4) debug_ty=bool is_parameter=false span=unknown (overlap; ambiguous)"));
    assert!(output.contains("range#4: s2 name=\"not_an_argument\" instructions=[2, 6) debug_ty=bool is_parameter=true span=unknown\n"));
    assert!(output.contains("range#5: s1 name=\"empty\" instructions=[4, 4) debug_ty=bool is_parameter=false span=unknown\n"));
    assert!(output.contains("debug frame_layout params (nonauthoritative): [str, str, str]"));
    assert!(output.contains("debug frame_layout slots (nonauthoritative): [str, bool]"));
    let names: Vec<_> = output
        .lines()
        .filter(|line| line.contains("; debug local s1"))
        .collect();
    assert_eq!(
        names,
        vec![
            "    ; debug local s1 at 0: names=[range#1 \"early\\n\\u{202e}\"]",
            "    ; debug local s1 at 1: names=[range#1 \"early\\n\\u{202e}\"]",
            "    ; debug local s1 at 2: names=[range#1 \"early\\n\\u{202e}\", range#2 \"overlap\", range#3 \"overlap\"] (ambiguous)",
            "    ; debug local s1 at 3: names=[range#1 \"early\\n\\u{202e}\", range#2 \"overlap\", range#3 \"overlap\"] (ambiguous)",
            "    ; debug local s1 at 4: names=[range#0 \"late\", range#2 \"overlap\", range#3 \"overlap\"] (ambiguous)",
            "    ; debug local s1 at 5: names=[range#0 \"late\"]",
        ]
    );
    assert!(!output.contains("debug local s0"));
    assert!(!output.contains("names=[range#5"));
    assert!(output.contains("  0004  move s0, s2\n"));
}

#[test]
fn touching_ranges_do_not_overlap_or_leak_names_past_their_end() {
    let mut body = function();
    body.metadata.debug.local_live_ranges = vec![range(0, "second", 2, 4), range(0, "first", 0, 2)];
    let output = render(&prepare(body));
    assert!(!output.contains("ambiguous"));
    assert!(output.contains("debug local s1 at 1: names=[range#1 \"first\"]\n"));
    assert!(output.contains("debug local s1 at 2: names=[range#0 \"second\"]\n"));
    assert!(!output.contains("debug local s1 at 4"));
    assert!(!output.contains("debug local s1 at 5"));
}

#[test]
fn branch_and_jump_targets_define_each_numeric_label_once() {
    let mut body = function();
    body.slot_count = 2;
    body.metadata.params = vec![ValueType::Bool];
    body.metadata.slots = vec![ValueType::Bool; 2];
    body.metadata.slots = vec![ValueType::Bool; 2];
    body.metadata.return_type = ValueType::Bool;
    body.instructions = vec![
        BytecodeInstruction::Move {
            dst: Slot::new(0),
            src: Slot::new(1),
        },
        BytecodeInstruction::Branch {
            cond: Slot::new(0),
            then_target: JumpTarget::new(3),
            else_target: JumpTarget::new(2),
        },
        BytecodeInstruction::Jump {
            target: JumpTarget::new(4),
        },
        BytecodeInstruction::Jump {
            target: JumpTarget::new(4),
        },
        BytecodeInstruction::Return(Some(Slot::new(0))),
    ];
    body.metadata.origins = (0..body.instructions.len())
        .map(|instruction| crate::module::InstructionOrigin {
            span: Default::default(),
            line: None,
            column: None,
            instruction,
        })
        .collect();
    // Labels follow actual jump/branch operands, not this optional metadata list.
    body.metadata.control_flow_targets = vec![JumpTarget::new(0)];
    let output = render(&prepare(body));
    assert!(output.ends_with(include_str!(
        "../../../tests/fixtures/disassembly/branch-label-tail.txt"
    )));
    assert!(!output.contains("L0:"));
    assert_eq!(output.matches("L4:").count(), 1);
}

#[test]
fn physical_gc_roots_are_not_derived_from_debug_layouts() {
    let mut body = function();
    // Str is not a GC-reference representation; use actual traced heap slots.
    body.metadata.params = vec![ValueType::HeapObject];
    body.metadata.slots = vec![ValueType::HeapObject; 3];
    body.metadata.return_type = ValueType::HeapObject;
    body.metadata.roots = RootSlotLayout::from_types(&body.metadata.slots);
    body.metadata.debug.frame_layout.slots = vec![ValueType::I32; 2];
    let output = render(&prepare(body));
    assert!(output.contains("roots: slots=[s0, s1, s2]\n"));
    assert!(output.contains("s1: physical=heap_object semantic=unknown\n"));
    assert!(output.contains("s2: physical=heap_object semantic=unknown\n"));
    assert!(output.contains("s0: physical=heap_object semantic=unknown\n"));
    assert!(output.contains("debug frame_layout slots (nonauthoritative): [i32, i32]\n"));
}

struct FailingWriter {
    remaining: usize,
    failed: bool,
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        assert!(!self.failed, "write after the first IO error");
        if self.remaining == 0 {
            self.failed = true;
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        let count = bytes.len().min(self.remaining);
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("the body renderer must not flush its caller's writer")
    }
}

#[test]
fn every_body_write_propagates_the_first_io_error_without_flushing() {
    let mut body = function();
    body.metadata.debug.local_live_ranges = vec![range(0, "value", 0, 6)];
    body.metadata.debug.line_table.push(LineTableEntry {
        instruction_offset: 4,
        source_offset: 12,
        line: Some(2),
        column: Some(3),
    });
    let prepared = prepare(body);
    let context = DisplayContext {
        definitions: prepared.program().definitions(),
    };
    // Every byte boundary includes header, debug table, annotations, labels,
    // instruction operands and the final newline, not only an initial write.
    for remaining in 0..render(&prepared).len() {
        let mut writer = FailingWriter {
            remaining,
            failed: false,
        };
        let error = write_body(
            &mut writer,
            &context,
            &Resolution::new(prepared.program().program(), context.definitions),
            0,
            0,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(writer.failed);
    }
}
