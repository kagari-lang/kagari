use crate::{
    artifact::{
        ArtifactBuildOptions, ArtifactValidationError, KAGARI_COMPILER_FINGERPRINT,
        KAGARI_LANGUAGE_VERSION, KBC_ARTIFACT_FORMAT_VERSION, KbcArtifact,
    },
    disassembly::{DisassemblyError, PreparedDisassembly},
    instruction::{BytecodeInstruction, LocalSlot, Register},
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
    BytecodeFunction {
        name: name.into(),
        parameter_count: 1,
        local_count: 1,
        register_count: 1,
        metadata: FunctionMetadata {
            params: vec![ValueType::I32],
            return_type: ValueType::I32,
            locals: vec![ValueType::I32],
            registers: vec![ValueType::I32],
            ..Default::default()
        },
        instructions: vec![
            BytecodeInstruction::LoadLocal {
                dst: Register::new(0),
                local: LocalSlot::new(0),
            },
            BytecodeInstruction::Return(Some(Register::new(0))),
        ],
        ..Default::default()
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

#[test]
fn source_free_preparation_retains_scope_and_deterministic_directory() {
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
    let output = directory(&first);
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
    for (m, f) in prepared.select(None).unwrap() {
        assert_eq!(
            prepared.select(Some(&format!("@{m}:{f}"))).unwrap(),
            vec![(m, f)]
        );
    }
    let error = prepared.select(Some("fixture::app::echo")).unwrap_err();
    let DisassemblyError::AmbiguousFunction { candidates, .. } = &error else {
        panic!("expected ambiguity: {error}");
    };
    assert_eq!(candidates.len(), 2);
    assert!(candidates[0].starts_with("fn @1:0 fixture::app::echo<i32>"));
    assert!(candidates[1].starts_with("fn @1:1 fixture::app::echo<i64>"));
    let listing = directory(&prepared);
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
        let error = prepared.select(Some(selector)).unwrap_err();
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
                prepared.select(Some(selector)),
                Err(DisassemblyError::InvalidSelector(_))
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
        BytecodeInstruction::Return(Some(Register::new(99)));
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
    )
    .unwrap();
    let prepared =
        PreparedDisassembly::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap()).unwrap();
    assert!(directory(&prepared).contains("portable_mir: present, not semantically checked\n"));
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
fn custom_native_contracts_prepare_without_registration_or_script_bodies() {
    let context = DefinitionContext::new().unwrap();
    let module_identity = identity("custom_provider");
    let root = context.intern_root(&module_identity).unwrap();
    let binding = context
        .intern_child(root, DefinitionKind::Function, "limit", 0)
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
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            exports: Default::default(),
            modules: vec![BytecodeModule {
                identity: module_identity,
                public_items: vec![PublicItem::Function(required[0].function.clone())],
                native_declarations: required,
                native_imports: vec![import],
                ..Default::default()
            }],
        },
        Default::default(),
        &context,
    )
    .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    drop(artifact);
    drop(context);
    let prepared = PreparedDisassembly::from_bytes(&bytes).unwrap();
    assert_eq!(
        prepared.program().program().modules[0].native_imports.len(),
        1
    );
    assert!(prepared.select(None).unwrap().is_empty());
    assert!(
        prepared
            .select(Some("fixture::custom_provider::limit"))
            .is_err()
    );
    assert!(directory(&prepared).contains("modules: 1, functions: 0\n"));
    assert!(!directory(&prepared).contains("fn @"));
}
