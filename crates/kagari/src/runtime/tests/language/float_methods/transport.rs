use super::{FLOATS, Float, assert_checks, context, engine, programs};
use crate::{
    BytecodeArtifact, program::PreparedProgram,
    runtime::tests::language::numeric_checks::NumericChecks,
};
use kagari_bytecode::instruction::ConstantOperand;
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;
use rstest::rstest;

#[test]
fn exact_bits_survive_locals_gc_fields_native_methods_and_typed_host_entries() {
    let mut checks = Vec::new();
    for float in FLOATS {
        let ty = float.ty;
        let uint = float.uint;
        for bits in float.patterns() {
            checks.push(format!(
                "{{ val local: {ty} = {}; val stored = Store{ty} {{ value: local }}; \
                 val pressure = Vec::from([1, 2, 3]); \
                 pressure.len() == 3u64 && local.to_bits() == {bits}{uint} \
                 && stored.value.to_bits() == {bits}{uint} }}",
                float.value(bits)
            ));
        }
    }
    let checks = NumericChecks::new(checks);
    let source = format!(
        "struct Storef32 {{ val value: f32 }}\n\
         struct Storef64 {{ val value: f64 }}\n\
         pub fn from32(bits: u32) -> f32 {{ f32::from_bits(bits) }}\n\
         pub fn to32(value: f32) -> u32 {{ value.to_bits() }}\n\
         pub fn from64(bits: u64) -> f64 {{ f64::from_bits(bits) }}\n\
         pub fn to64(value: f64) -> u64 {{ value.to_bits() }}\n\
         {}",
        checks.source()
    );
    for (route, program) in programs(&source) {
        let consumer = engine();
        let context = context();
        let mut runtime = consumer.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let result: i32 = runtime
            .execute_typed(&loaded, ProgramEntry::Main, (), &context)
            .unwrap();
        checks.assert_result(result, route, &source);
        assert!(runtime.runtime().gc().stats().collections > 0, "{route}");
        for bits in FLOATS[0].patterns() {
            let value: f32 = runtime
                .execute_typed(&loaded, "from32", (bits as u32,), &context)
                .unwrap();
            assert_eq!(u64::from(value.to_bits()), bits, "{route}: {bits:#x}");
            let returned: u32 = runtime
                .execute_typed(&loaded, "to32", (f32::from_bits(bits as u32),), &context)
                .unwrap();
            assert_eq!(u64::from(returned), bits, "{route}: {bits:#x}");
        }
        for bits in FLOATS[1].patterns() {
            let value: f64 = runtime
                .execute_typed(&loaded, "from64", (bits,), &context)
                .unwrap();
            assert_eq!(value.to_bits(), bits, "{route}: {bits:#x}");
            let returned: u64 = runtime
                .execute_typed(&loaded, "to64", (f64::from_bits(bits),), &context)
                .unwrap();
            assert_eq!(returned, bits, "{route}: {bits:#x}");
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "{route}");
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        assert!(!runtime.runtime().is_quarantined());
    }
}

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn serialized_float_constants_reach_installed_to_bits_without_payload_changes(
    #[case] float: Float,
) {
    let ty = float.ty;
    let uint = float.uint;
    let patterns = float.patterns();
    let mut source = String::new();
    let mut checks = Vec::new();
    let mut replacements = Vec::new();
    for (index, bits) in patterns.iter().enumerate() {
        let placeholder = 1000 + index;
        source.push_str(&format!(
            "const C{index}: {ty} = {placeholder}.0{ty};\n\
             fn constant{index}() -> {uint} {{ val local: {ty} = C{index}; local.to_bits() }}\n"
        ));
        checks.push(format!("constant{index}() == {bits}{uint}"));
        replacements.push((float.bits(placeholder as f64), *bits));
    }
    let checks = NumericChecks::new(checks);
    source.push_str(&checks.source());
    let compiler = engine();
    let context = DefinitionContext::new().unwrap();
    let mut artifact = compiler
        .compile_to_artifact(
            SourceFile::new("float-constants.kgr", &source),
            Default::default(),
        )
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let root = &mut artifact.program.modules[artifact.program.root.index()];
    let mut replaced = 0;
    // Source cannot spell NaN payloads. Patch only this program's placeholders,
    // then rebuild all integrity/verification metadata before serializing.
    for constant in &mut root.constants {
        let old = match constant {
            ConstantOperand::F32(value) => u64::from(value.to_bits()),
            ConstantOperand::F64(value) => value.to_bits(),
            _ => continue,
        };
        let bits = replacements.iter().find(|(key, _)| *key == old).unwrap().1;
        *constant = if ty == "f32" {
            ConstantOperand::F32(f32::from_bits(bits as u32))
        } else {
            ConstantOperand::F64(f64::from_bits(bits))
        };
        replaced += 1;
    }
    assert_eq!(
        replaced,
        patterns.len(),
        "{ty}: every placeholder must be used"
    );
    let artifact = BytecodeArtifact::from_program(
        artifact.program,
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let artifact =
        DefinitionMetadata::checked(context.snapshot(), artifact, &Default::default()).unwrap();
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let root = &decoded.records().program.modules[decoded.records().program.root.index()];
    let mut actual = root
        .constants
        .iter()
        .filter_map(|constant| match constant {
            ConstantOperand::F32(value) => Some(u64::from(value.to_bits())),
            ConstantOperand::F64(value) => Some(value.to_bits()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut expected = patterns;
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected, "{ty}: decoded constant-pool payloads");
    for (route, artifact) in [
        ("patched constants", artifact),
        ("encoded constants", decoded),
    ] {
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        assert_checks(&program, route, &source, &checks);
    }
}
