use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("constants.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let mut context = ExecutionContext::default();
        context.capabilities.jit = jit;
        context.language_profile.allow_jit = jit;
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
    }
}

#[test]
fn defaults_overrides_and_generic_static_access_share_the_const_evaluator() {
    execute(
        r#"
const BASE: i32 = 20;
trait Limit { const VALUE: i32 = BASE + 1; fn limit(self) -> i32 { Self::VALUE } }
struct Default {}
struct Override {}
impl Limit for Default {}
impl Limit for Override { const VALUE: i32 = 21; }
fn limit<T: Limit>(x: T) -> i32 { T::VALUE + x.limit() }
fn main() -> i32 { limit(Default {}) + Override::VALUE - Default::VALUE }
"#,
    );
}

#[test]
fn required_constants_self_and_qualified_paths_are_checked() {
    execute(
        r#"
trait Left { const VALUE: i32; }
trait Right { const VALUE: i32 = 20; }
struct Number {}
impl Left for Number { const VALUE: i32 = 22; }
impl Right for Number {}
fn sum<T: Left + Right>(x: T) -> i32 { <T as Left>::VALUE + <T as Right>::VALUE }
fn main() -> i32 { sum(Number {}) }
"#,
    );
    execute(
        r#"
trait Value { const VALUE: i32; fn get(self) -> i32; }
struct Number {}
impl Value for Number { const VALUE: i32 = 42; fn get(self) -> i32 { Self::VALUE } }
fn main() -> i32 { Number {}.get() }
"#,
    );
}

#[test]
fn inherited_constants_are_static_only() {
    execute(
        r#"
trait Parent { const VALUE: i32 = 42; }
trait Child: Parent {}
struct Number {}
impl Parent for Number {}
impl Child for Number {}
fn get<T: Child>(x: T) -> i32 { T::VALUE }
fn main() -> i32 { get(Number {}) }
"#,
    );
}

#[test]
fn invalid_constant_contracts_fail_before_execution() {
    for source in [
        "trait Limit { const VALUE: i32; } struct N {} impl Limit for N {}",
        "trait Limit { const VALUE: i32; } struct N {} impl Limit for N { const VALUE: bool = true; }",
        "trait Limit { const VALUE: i32 = 1; } struct N {} impl Limit for N { const EXTRA: i32 = 1; }",
        "trait Limit { const VALUE: i32; } struct N {} impl Limit for N { const VALUE: i32; }",
        "trait Limit { const VALUE: i32 = 1; const VALUE: i32 = 2; }",
        "trait Limit { const VALUE: Array<i32> = [1]; }",
        "trait Limit { const VALUE: i32 = 2147483647 + 1; }",
        "fn f() -> i32 { 1 } trait Limit { const VALUE: i32 = f(); }",
        "trait Limit { const VALUE: i32 = 1; } fn dynamic(x: Limit) {}",
        "trait Limit { const VALUE: i32 = 1; } trait Child: Limit {} fn dynamic(x: Child) {}",
        "trait Left { const VALUE: i32 = 1; } trait Right { const VALUE: i32 = 2; } fn f<T: Left + Right>(x: T) -> i32 { T::VALUE }",
    ] {
        assert!(
            KagariEngine::default()
                .compile_to_artifact(
                    SourceFile::new("invalid.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn generic_impls_and_all_scalar_constant_types_execute() {
    execute(
        r#"
trait Value { const VALUE: i32 = 42; const OK: bool = true; const FRACTION: f32 = 1.5; const UNIT: () = (); }
struct Holder<T> { val value: T }
impl<T> Value for Holder<T> { const VALUE: i32 = 42; }
fn get<T: Value>(x: T) -> i32 { val unit: () = T::UNIT; if T::OK && T::FRACTION == 1.5 { T::VALUE } else { 0 } }
fn main() -> i32 { get(Holder { value: true }) + Holder<i32>::VALUE - 42 }
"#,
    );
}

#[test]
fn imported_defaults_keep_the_trait_module_constant_resolution() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "const BASE: i32 = 21; pub trait Limit { const VALUE: i32 = BASE; fn limit(self) -> i32 { Self::VALUE } }",
        ),
        (
            "root",
            "use pkg::model::Limit; const BASE: i32 = 0; struct Number {} impl Limit for Number {} fn main() -> i32 { Number::VALUE + Number {}.limit() }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let id = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(id);
        }
    }
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root.unwrap(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn malformed_constant_records_and_dynamic_interface_forgery_are_rejected() {
    use kagari_ir::module::PublicAbiItem;
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("constants.kgr", "pub trait Limit { const VALUE: i32; } struct Number {} impl Limit for Number { const VALUE: i32 = 42; } fn main() -> i32 { Number::VALUE }"), Default::default(), Default::default()).unwrap();
    for mutation in 0..4 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation < 2 {
            let record = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let PublicAbiItem::Trait(record) = item {
                        Some(record)
                    } else {
                        None
                    }
                })
                .unwrap();
            if mutation == 0 {
                record
                    .associated_consts
                    .push(record.associated_consts[0].clone());
            } else {
                record.associated_consts[0].default_value = Some("const-v1:i32:00042".into());
            }
        } else {
            let table = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    if let PublicAbiItem::InterfaceTable(table) = item {
                        Some(table)
                    } else {
                        None
                    }
                })
                .unwrap();
            if mutation == 2 {
                table.associated_consts.clear();
            } else {
                table.associated_consts[0].value = "const-v1:i32:2147483648".into();
            }
        }
        assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
    }
    let dynamic = engine.compile_to_artifact(SourceFile::new("dynamic.kgr", "pub trait Read { fn read(self) -> i32; } struct Number {} impl Read for Number { fn read(self) -> i32 { 42 } } fn main() -> i32 { val x: Read = Number {}; x.read() }"), Default::default(), Default::default()).unwrap();
    let mut program = dynamic.program.clone();
    let module = &mut program.modules[program.root.index()];
    let identity = module
        .public_items
        .iter()
        .find_map(|item| {
            let PublicAbiItem::InterfaceTable(table) = item else {
                return None;
            };
            let kagari_ir::module::abi::AbiType::Trait(interface) = &table.trait_type else {
                return None;
            };
            Some(interface.declaration.clone())
        })
        .unwrap();
    let record = module
        .public_items
        .iter_mut()
        .find_map(|item| {
            if let PublicAbiItem::Trait(record) = item {
                Some(record)
            } else {
                None
            }
        })
        .unwrap();
    record
        .associated_consts
        .push(kagari_ir::module::abi::AssociatedConstAbi {
            declaration: kagari_hir::types::associated_const_id(&identity, "VALUE"),
            ty: kagari_ir::module::abi::AbiType::Builtin(kagari_hir::types::BuiltinType::I32),
            default_value: Some("const-v1:i32:42".into()),
        });
    assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
}
