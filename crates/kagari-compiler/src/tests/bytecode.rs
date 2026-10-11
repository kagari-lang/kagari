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
    program::BytecodeProgram,
    verifier::{BytecodeVerificationError, verify_module},
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext};
use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_types::{
    declaration::TypeDefKind,
    host_interface::{
        type_declaration::{
            HostMethodDeclaration, HostTraitImplementationDeclaration, HostTraitMethodBinding,
            HostTypeDeclaration,
        },
        value_type::HostValueType,
    },
};

fn host_trait_test_program(source: &str, context: &DefinitionContext) -> BytecodeProgram {
    let mut module = common::adopt_bytecode(context, common::bytecode_ok(source));
    let root = module.root.index();
    add_readable_host(&mut module.modules[root], context);
    module
}

fn add_readable_host(module: &mut BytecodeModule, context: &DefinitionContext) {
    let root = context.intern_root(&module.identity).unwrap();
    let trait_id = context
        .intern_child(root, DefinitionKind::Trait, "Readable", 0)
        .unwrap();
    let trait_method = context
        .intern_child(trait_id, DefinitionKind::Method, "get", 0)
        .unwrap();
    let mut host = HostTypeDeclaration::new(context, "demo.Counter").unwrap();
    let method =
        HostMethodDeclaration::new(context, host.id, "read", vec![], HostValueType::I32).unwrap();
    host.methods.push(method.clone());
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![HostValueType::I32],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id,
            }],
        ));
    module.host_interface.functions.push(
        host.method_contract(&method.id, &context.snapshot())
            .unwrap(),
    );
    context
        .intern_child(host.id, DefinitionKind::Impl, "", 0)
        .unwrap();
    module.host_interface.types.push(host);
}

mod artifacts;
mod host_contracts;
mod identities;
mod interfaces;
mod lowering;
mod validation;
