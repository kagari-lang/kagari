use crate::{
    artifact::{
        ArtifactBuildOptions, ArtifactValidationError, KAGARI_COMPILER_FINGERPRINT,
        KAGARI_LANGUAGE_VERSION, KBC_ARTIFACT_FORMAT_VERSION, KbcArtifact,
    },
    disassembly::{DisassemblyError, PreparedDisassembly},
    instruction::{BytecodeInstruction, CallTarget, NativeImportId, Slot},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord},
    native_input::PortableMir,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext};
use kagari_contract::{
    ids::FunctionRef,
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{FnDecl, host::HostCallPolicy, module::ModuleDecl},
    scalar::BuiltinType,
    ty::Ty,
};
use std::io::{self, Write};

fn identity(path: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("fixture".into()),
        path: vec![path.into()],
    }
}

fn function(name: &str) -> BytecodeFunction {
    {
        let parameter_base: usize = 1;
        let fixture_instructions = vec![
            BytecodeInstruction::Move {
                dst: Slot::new(0),
                src: Slot::new(1),
            },
            BytecodeInstruction::Return(Some(Slot::new(0))),
        ];
        let mut fixture_metadata: crate::module::FunctionMetadata = FunctionMetadata {
            params: vec![ValueType::I32],
            return_type: ValueType::I32,
            slots: [vec![ValueType::I32], vec![ValueType::I32]].concat(),
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
            name: name.into(),
            parameter_count: 1,
            slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
            metadata: fixture_metadata,
            instructions: fixture_instructions,
            ..Default::default()
        }
    }
}

fn push_function(module: &mut BytecodeModule, mut function: BytecodeFunction) {
    function.id = FunctionRef::new(module.functions.len());
    module.function_table.push(FunctionRecord {
        id: function.id,
        identity: function.identity.clone(),
        name: function.name.clone(),
        params: function.metadata.params.clone(),
        return_type: function.metadata.return_type,
        effects: function.metadata.effects,
    });
    module.functions.push(function);
}

fn fixture() -> (DefinitionContext, KbcArtifact) {
    let context = DefinitionContext::new().unwrap();
    // Unreferenced earlier IDs must not become the meaning of decoded handles.
    context.intern_root(&identity("unrelated")).unwrap();
    let mut dependency = BytecodeModule {
        identity: identity("dependency"),
        types: vec![ValueType::I32],
        ..Default::default()
    };
    push_function(&mut dependency, function("echo"));
    let mut root = BytecodeModule {
        identity: identity("app"),
        types: vec![ValueType::I32],
        dependencies: vec![ModuleRef::new(0)],
        source_name: "nonexistent/source.kgr".into(),
        ..Default::default()
    };
    let owner = context.intern_root(&root.identity).unwrap();
    let declaration = context
        .intern_child(owner, DefinitionKind::Function, "echo", 0)
        .unwrap();
    for argument in [BuiltinType::I32, BuiltinType::I64] {
        let mut body = function("generated_echo");
        body.identity = Some(ConcreteFunctionIdentity {
            declaration,
            arguments: vec![Ty::Builtin(argument)],
        });
        body.metadata
            .semantic
            .params
            .insert(0, Ty::Builtin(BuiltinType::I32));
        body.metadata.semantic.result = Some(Ty::Builtin(BuiltinType::I32));
        body.metadata.effects.may_suspend = true;
        push_function(&mut root, body);
    }
    push_function(&mut root, function("generated\n\u{202e}name"));
    let mut occurrence = function("untrusted_alias");
    occurrence
        .metadata
        .semantic
        .params
        .insert(0, Ty::Builtin(BuiltinType::I32));
    occurrence.metadata.semantic.result = Some(Ty::Builtin(BuiltinType::I32));
    occurrence.identity = Some(ConcreteFunctionIdentity {
        declaration: context
            .intern_child(owner, DefinitionKind::Function, "echo", 2)
            .unwrap(),
        arguments: vec![],
    });
    push_function(&mut root, occurrence);
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(1),
            modules: vec![dependency, root],
            exports: Default::default(),
        },
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    (context, artifact)
}

fn bytes() -> Vec<u8> {
    let (context, artifact) = fixture();
    artifact.to_bytes(&context.snapshot()).unwrap()
}

fn directory(prepared: &PreparedDisassembly) -> String {
    let mut output = Vec::new();
    prepared.write_directory(&mut output).unwrap();
    String::from_utf8(output).unwrap()
}

fn disassembly(prepared: &PreparedDisassembly, selector: Option<&str>) -> String {
    let mut output = Vec::new();
    prepared.write_disassembly(&mut output, selector).unwrap();
    String::from_utf8(output).unwrap()
}

fn function_headers(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|line| line.starts_with("fn @"))
        .collect()
}

fn selection_error_before_output(
    prepared: &PreparedDisassembly,
    selector: &str,
) -> DisassemblyError {
    let expected = prepared.select(Some(selector)).unwrap_err();
    let mut writer = FailingWriter {
        remaining: 0,
        failed: false,
    };
    let error = prepared
        .write_disassembly(&mut writer, Some(selector))
        .unwrap_err();
    assert!(
        !writer.failed,
        "invalid selection must not touch the writer"
    );
    assert_eq!(error.to_string(), expected.to_string());
    error
}

#[test]
fn source_free_preparation_retains_scope_and_deterministic_output() {
    let bytes = bytes();
    let first = PreparedDisassembly::from_bytes(&bytes).unwrap();
    let second = PreparedDisassembly::from_bytes(&bytes).unwrap();
    let first_id = first.program().program().modules[1].functions[0]
        .identity
        .as_ref()
        .unwrap()
        .declaration;
    let second_id = second.program().program().modules[1].functions[0]
        .identity
        .as_ref()
        .unwrap()
        .declaration;
    assert_ne!(first_id, second_id);
    assert!(first.program().definitions().resolve(second_id).is_err());
    assert_eq!(
        first
            .program()
            .definitions()
            .resolve(first_id)
            .unwrap()
            .module(),
        &identity("app")
    );
    assert_eq!(directory(&first), directory(&second));
    let full = disassembly(&first, None);
    assert_eq!(full, disassembly(&second, None));
    assert_eq!(full.matches("  0000  move s0, s1").count(), 5);
    assert_eq!(full.matches("  0001  return").count(), 5);
    assert!(!full.contains("nonexistent"));
    assert!(!full.contains('\u{202e}'));
    let output = directory(&first);
    assert_eq!(function_headers(&full), function_headers(&output));
    assert!(output.starts_with(&format!(
        "artifact: format={KBC_ARTIFACT_FORMAT_VERSION} language=\"{KAGARI_LANGUAGE_VERSION}\" validation=envelope+bytecode\ncompiler: \"{KAGARI_COMPILER_FINGERPRINT}\"\n"
    )));
    assert!(output.contains("runtime_abi: \""));
    assert!(output.contains(" helper_abi: \""));
    assert!(output.contains("content_hash: "));
    assert!(
        output.contains("root: @1 fixture::app\nmodules: 2, functions: 5\nportable_mir: absent\n")
    );
    assert!(output.ends_with(concat!(
        "\nmodule @0: fixture::dependency\n",
        "fn @0:0 fixture::dependency::echo(arg0: physical(i32)) -> physical(i32) ; instructions=2 may_suspend=false recorded_effects=[]\n",
        "\nmodule @1: fixture::app\n",
        "fn @1:0 fixture::app::echo<i32>(arg0: i32) -> i32 ; instructions=2 may_suspend=true recorded_effects=[may_suspend]\n",
        "fn @1:1 fixture::app::echo<i64>(arg0: i32) -> i32 ; instructions=2 may_suspend=true recorded_effects=[may_suspend]\n",
        "fn @1:2 fixture::app::\"generated\\n\\u{202e}name\"(arg0: physical(i32)) -> physical(i32) ; instructions=2 may_suspend=false recorded_effects=[]\n",
        "fn @1:3 fixture::app::echo#2(arg0: i32) -> i32 ; instructions=2 may_suspend=false recorded_effects=[]\n",
    )));
    assert!(!output.contains("nonexistent"));
    assert!(!output.contains("load_local"));
}

#[test]
fn exact_selection_uses_raw_names_and_reports_ordered_instance_candidates() {
    let prepared = PreparedDisassembly::from_bytes(&bytes()).unwrap();
    assert_eq!(
        prepared.select(None).unwrap(),
        vec![(0, 0), (1, 0), (1, 1), (1, 2), (1, 3)]
    );
    assert_eq!(
        prepared.select(Some("fixture::dependency::echo")).unwrap(),
        vec![(0, 0)]
    );
    assert_eq!(
        prepared.select(Some("fixture::app::echo#2")).unwrap(),
        vec![(1, 3)]
    );
    assert_eq!(
        prepared
            .select(Some("fixture::app::generated\n\u{202e}name"))
            .unwrap(),
        vec![(1, 2)]
    );
    let listing = directory(&prepared);
    let headers = function_headers(&listing);
    let full = disassembly(&prepared, None);
    assert_eq!(function_headers(&full), headers);
    for ((m, f), header) in prepared.select(None).unwrap().into_iter().zip(&headers) {
        let selector = format!("@{m}:{f}");
        assert_eq!(prepared.select(Some(&selector)).unwrap(), vec![(m, f)]);
        let filtered = disassembly(&prepared, Some(&selector));
        assert_eq!(function_headers(&filtered), vec![*header]);
        let body_start = filtered.find("\nfn @").unwrap() + 1;
        assert!(
            full.contains(&filtered[body_start..]),
            "selected body differs from full output"
        );
        assert_eq!(filtered.matches("  0000  move s0, s1").count(), 1);
        assert_eq!(filtered.matches("  0001  return").count(), 1);
    }
    for (name, slot) in [
        ("fixture::dependency::echo", "@0:0"),
        ("fixture::app::echo#2", "@1:3"),
        ("fixture::app::generated\n\u{202e}name", "@1:2"),
    ] {
        assert_eq!(
            disassembly(&prepared, Some(name)),
            disassembly(&prepared, Some(slot))
        );
    }
    let error = selection_error_before_output(&prepared, "fixture::app::echo");
    let DisassemblyError::AmbiguousFunction { candidates, .. } = &error else {
        panic!("expected ambiguity: {error}");
    };
    assert_eq!(candidates.len(), 2);
    assert!(candidates[0].starts_with("fn @1:0 fixture::app::echo<i32>"));
    assert!(candidates[1].starts_with("fn @1:1 fixture::app::echo<i64>"));
    for candidate in candidates {
        assert!(listing.lines().any(|line| line == candidate));
    }
    for selector in [
        "echo",
        "fixture::app::generated_echo",
        "fixture::app::echo<i32>",
        "fixture::app::generated\\n\\u{202e}name",
        "",
        "missing\n\u{202e}",
    ] {
        let error = selection_error_before_output(&prepared, selector);
        assert!(matches!(error, DisassemblyError::FunctionNotFound(_)));
        assert!(!error.to_string().contains(['\n', '\u{202e}']));
    }
    for selector in [
        "@",
        "@1",
        "@1:",
        "@:0",
        "@-1:0",
        "@+1:0",
        "@1: 0",
        "@1:0:0",
        "@2:0",
        "@1:4",
        "@184467440737095516160:0",
    ] {
        assert!(
            matches!(
                selection_error_before_output(&prepared, selector),
                DisassemblyError::InvalidSelector(_)
            ),
            "{selector}"
        );
    }
}

#[test]
fn preparation_rejects_decode_compatibility_hash_and_unselected_code_failures() {
    let (context, artifact) = fixture();
    let valid = artifact.to_bytes(&context.snapshot()).unwrap();
    for invalid in [&[][..], &valid[..valid.len() - 1]] {
        assert!(matches!(
            PreparedDisassembly::from_bytes(invalid),
            Err(DisassemblyError::Decode(_))
        ));
    }
    let mut trailing = valid;
    trailing.push(0);
    assert!(matches!(
        PreparedDisassembly::from_bytes(&trailing),
        Err(DisassemblyError::Decode(_))
    ));
    let mut incompatible = artifact.clone();
    incompatible.header.runtime_abi_version = "incompatible\n\u{202e}".into();
    let error =
        PreparedDisassembly::from_bytes(&incompatible.to_bytes(&context.snapshot()).unwrap())
            .unwrap_err();
    assert!(
        matches!(&error, DisassemblyError::Validation(error) if matches!(error.as_ref(), ArtifactValidationError::RuntimeAbiMismatch { .. }))
    );
    assert!(!error.to_string().contains(['\n', '\u{202e}']));
    let mut changed = artifact.clone();
    changed.program.modules[0].source_name.push_str("changed");
    assert!(matches!(
        PreparedDisassembly::from_bytes(&changed.to_bytes(&context.snapshot()).unwrap()),
        Err(DisassemblyError::Validation(error)) if matches!(error.as_ref(), ArtifactValidationError::ContentHashMismatch)
    ));
    let mut invalid = artifact;
    invalid.program.modules[1].functions[3].instructions[1] =
        BytecodeInstruction::Return(Some(Slot::new(99)));
    // Decoding still succeeds. No caller can obtain a prepared directory or select
    // an unaffected function without validating this invalid, unselected body.
    let invalid = invalid.to_bytes(&context.snapshot()).unwrap();
    assert!(KbcArtifact::from_bytes(&invalid).is_ok());
    assert!(matches!(
        PreparedDisassembly::from_bytes(&invalid),
        Err(DisassemblyError::Validation(_))
    ));
}

#[test]
fn portable_mir_is_reported_but_never_interpreted() {
    let (context, artifact) = fixture();
    let artifact = KbcArtifact::from_program(
        artifact.program,
        ArtifactBuildOptions {
            portable_mir: Some(PortableMir {
                bytes: b"not a MIR program".to_vec(),
            }),
            ..Default::default()
        },
        &context,
        &Default::default(),
    )
    .unwrap();
    let prepared =
        PreparedDisassembly::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap()).unwrap();
    assert!(directory(&prepared).contains("portable_mir: present, not semantically checked\n"));
    let output = disassembly(&prepared, Some("@1:0"));
    assert!(output.contains("portable_mir: present, not semantically checked\n"));
    assert!(output.contains("  0000  move s0, s1"));
}

struct FailingWriter {
    remaining: usize,
    failed: bool,
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        assert!(!self.failed, "writer called again after its first error");
        if self.remaining == 0 {
            self.failed = true;
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        let count = self.remaining.min(bytes.len());
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("caller owns flushing")
    }
}

fn assert_disassembly_io_error(
    prepared: &PreparedDisassembly,
    selector: Option<&str>,
    remaining: usize,
) {
    let mut writer = FailingWriter {
        remaining,
        failed: false,
    };
    let error = prepared
        .write_disassembly(&mut writer, selector)
        .unwrap_err();
    let DisassemblyError::Io(error) = error else {
        panic!("expected wrapped writer failure: {error}");
    };
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(error.to_string(), "closed");
    assert!(writer.failed);
}

#[test]
fn writer_errors_stop_summary_and_directory_without_flushing() {
    let prepared = PreparedDisassembly::from_bytes(&bytes()).unwrap();
    let mut summary = Vec::new();
    prepared.write_summary(&mut summary).unwrap();
    for remaining in [0, 1, summary.len() + 40] {
        let mut writer = FailingWriter {
            remaining,
            failed: false,
        };
        assert_eq!(
            prepared.write_directory(&mut writer).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert!(writer.failed);
    }
    // Full output wraps summary failures in the public disassembly error.
    assert_disassembly_io_error(&prepared, None, 0);
    assert_disassembly_io_error(&prepared, Some("@1:0"), 1);
    let mut writer = FailingWriter {
        remaining: 3,
        failed: false,
    };
    assert_eq!(
        prepared.write_summary(&mut writer).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[test]
fn custom_native_calls_render_verified_offline_contracts_without_providers() {
    let context = DefinitionContext::new().unwrap();
    let module_identity = identity("custom_provider");
    let root = context.intern_root(&module_identity).unwrap();
    let binding = context
        .intern_child(root, DefinitionKind::Function, "rust_limit", 0)
        .unwrap();
    let mut declarations = ModuleDecl::new(module_identity.clone());
    declarations.functions.push(FnDecl {
        name: "limit".into(),
        implementation: CallableImplementation::Native(binding),
        host_policy: Some(HostCallPolicy {
            passing: vec![],
            effects: Default::default(),
        }),
        method_policy: Default::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![],
        return_type: Ty::Builtin(BuiltinType::I32),
    });
    declarations.collect_definitions(&context).unwrap();
    let required = declarations
        .native_declarations(&context.snapshot())
        .unwrap();
    let import = NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: None,
        instance: ConcreteFunctionIdentity {
            declaration: required[0].declaration,
            arguments: vec![],
        },
        binding,
        signature: Signature {
            params: vec![],
            result: Ty::Builtin(BuiltinType::I32),
        },
        requirements: vec![],
        callables: vec![],
    };
    let mut module = BytecodeModule {
        identity: module_identity,
        source_name: "nonexistent/native_caller.kgr".into(),
        types: vec![ValueType::I32],
        public_items: vec![PublicItem::Function(required[0].function.clone())],
        native_declarations: required,
        native_imports: vec![import],
        ..Default::default()
    };
    let mut caller = {
        let fixture_instructions = vec![
            BytecodeInstruction::Call {
                dst: Some(Slot::new(0)),
                callee: CallTarget::Native(NativeImportId::new(0)),
                args: vec![],
            },
            BytecodeInstruction::Call {
                dst: Some(Slot::new(0)),
                callee: CallTarget::Native(NativeImportId::new(0)),
                args: vec![],
            },
            BytecodeInstruction::Return(Some(Slot::new(0))),
        ];
        let mut fixture_metadata: crate::module::FunctionMetadata = FunctionMetadata {
            return_type: ValueType::I32,
            slots: [vec![ValueType::I32], vec![].clone()].concat(),
            ..Default::default()
        };

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
            name: "call_limit".into(),
            slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
            metadata: fixture_metadata,
            instructions: fixture_instructions,
            ..Default::default()
        }
    };
    caller.metadata.semantic.result = Some(Ty::Builtin(BuiltinType::I32));
    caller
        .metadata
        .semantic
        .slots
        .insert(0, Ty::Builtin(BuiltinType::I32));
    caller.metadata.effects.calls = true;
    push_function(&mut module, caller);
    push_function(&mut module, function("echo"));
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            exports: Default::default(),
            modules: vec![module],
        },
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let mut invalid = artifact;
    invalid.program.modules[0].functions[0].instructions[0] = BytecodeInstruction::Call {
        dst: Some(Slot::new(0)),
        callee: CallTarget::Native(NativeImportId::new(99)),
        args: vec![],
    };
    let invalid_bytes = invalid.to_bytes(&context.snapshot()).unwrap();
    drop(invalid);
    drop(declarations);
    drop(context);

    // Only encoded artifacts remain: no compiler, source documents, runtime,
    // provider installation or original definition scope participates in output.
    let prepared = PreparedDisassembly::from_bytes(&bytes).unwrap();
    let fresh = PreparedDisassembly::from_bytes(&bytes).unwrap();
    assert_ne!(
        prepared.program().program().modules[0].native_imports[0].binding,
        fresh.program().program().modules[0].native_imports[0].binding,
    );
    let output = disassembly(&prepared, None);
    assert_eq!(output, disassembly(&fresh, None));
    assert_eq!(prepared.select(None).unwrap(), vec![(0, 0), (0, 1)]);
    assert!(matches!(
        selection_error_before_output(&prepared, "fixture::custom_provider::limit"),
        DisassemblyError::FunctionNotFound(_)
    ));
    let listing = directory(&prepared);
    assert!(listing.contains("modules: 1, functions: 2\n"));
    assert_eq!(function_headers(&output), function_headers(&listing));
    assert!(!listing.contains("native#"));
    assert!(output.contains(concat!(
        "descriptors @0: fixture::custom_provider\n",
        "  @0:native#0 fixture::custom_provider::limit arguments=[] binding=fixture::custom_provider::rust_limit receiver_access=none installation=not_checked\n",
        "    signature params=[] -> i32\n",
    )));
    assert_eq!(output.matches("  @0:native#0 ").count(), 1);
    assert!(output.contains("    declaration fixture::custom_provider::limit\n"));
    assert!(
        output.contains(
            "name=\"limit\" implementation=(native fixture::custom_provider::rust_limit)"
        )
    );
    let calls = concat!(
        "  0000  call dst=s0, target=native @0:native#0, args=[]\n",
        "  0001  call dst=s0, target=native @0:native#0, args=[]\n",
        "  0002  return s0\n",
    );
    assert!(output.contains(calls), "{output}");
    assert_eq!(output.matches("target=native @0:native#0").count(), 2);
    assert!(!output.contains("nonexistent"));

    let selected = disassembly(&prepared, Some("fixture::custom_provider::call_limit"));
    assert_eq!(selected, disassembly(&prepared, Some("@0:0")));
    assert_eq!(selected, disassembly(&fresh, Some("@0:0")));
    assert_eq!(
        function_headers(&selected),
        vec![function_headers(&listing)[0]]
    );
    assert!(selected.contains(calls));
    assert!(selected.contains("installation=not_checked"));
    let unreferenced = disassembly(&prepared, Some("@0:1"));
    assert_eq!(
        function_headers(&unreferenced),
        vec![function_headers(&listing)[1]]
    );
    assert!(!unreferenced.contains("descriptors @"));
    assert!(!unreferenced.contains("native#"));
    assert!(!unreferenced.contains("call_limit"));

    // Distinct facade routes: descriptor output and native-call body output must
    // wrap the original IO error and stop, not retry or flush. Low-level writers
    // already cover every byte boundary in their own pure-renderer fixtures.
    for marker in ["    signature params=[]", "target=native @0:native#0"] {
        let boundary = selected.find(marker).unwrap() + marker.len();
        assert_disassembly_io_error(&prepared, Some("@0:0"), boundary);
    }
    assert!(KbcArtifact::from_bytes(&invalid_bytes).is_ok());
    assert!(matches!(
        PreparedDisassembly::from_bytes(&invalid_bytes),
        Err(DisassemblyError::Validation(error)) if matches!(error.as_ref(), ArtifactValidationError::Bytecode(_))
    ));
}
