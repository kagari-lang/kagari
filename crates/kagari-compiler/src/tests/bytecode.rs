use crate::tests::common;
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactSectionId,
        ArtifactValidationError, DebugMetadata, DependencyFingerprint, KBC_MAGIC, KbcArtifact,
    },
    instruction::{
        BinaryOp, BytecodeInstruction, CallTarget, FieldRef, JumpTarget, LocalSlot, PathId,
        Register, RuntimeHelper, StructId, UnaryOp,
    },
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, PathRecord, SafeDebugPointKind},
    program::{BytecodeProgram, verify_program},
    verifier::{BytecodeVerificationError, verify_module},
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_types::declaration::TypeDefKind;

fn host_trait_test_program(source: &str) -> BytecodeProgram {
    let mut module = common::bytecode_ok(source);
    let root = module.root.index();
    add_readable_host(&mut module.modules[root]);
    module
}

fn add_readable_host(module: &mut BytecodeModule) {
    use {
        kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment},
        kagari_types::host_interface::{
            type_declaration::{
                HostMethodDeclaration, HostTraitImplementationDeclaration, HostTraitMethodBinding,
                HostTypeDeclaration,
            },
            value_type::HostValueType,
        },
    };

    let trait_id = DefinitionPath {
        module: module.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Readable".into(),
            occurrence: 0,
        }],
    };
    let mut trait_method = trait_id.clone();
    trait_method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "get".into(),
        occurrence: 0,
    });
    let mut host = HostTypeDeclaration::new("demo.Counter");
    let method = HostMethodDeclaration::new(&host.id, "read", vec![], HostValueType::I32);
    host.methods.push(method.clone());
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![HostValueType::I32],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id.clone(),
            }],
        ));
    module
        .host_interface
        .functions
        .push(host.method_contract(&method.id).unwrap());
    module.host_interface.types.push(host);
}

mod artifacts;
mod host_contracts;
mod identities;
mod interfaces;
mod lowering;
mod validation;
