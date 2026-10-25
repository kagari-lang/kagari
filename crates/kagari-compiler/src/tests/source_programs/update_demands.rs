use super::{checked, insert};
use crate::{
    bytecode::lower_program_to_bytecode,
    source::{
        lower::instances::MirLoweringOptions,
        program::{lower_program_to_mir, lower_program_to_mir_with_demands},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_contract::{
    types::named::{NamedContract, NamedDeclaration, correspondence::DeclarationCorrespondence},
    updates::{BodyClassification, DemandEnvelope, demands::publication_demands, limits::Budget},
};
use kagari_mir::{
    codec::{decode_program, encode_program},
    program::VerifiedMirProgram,
};
use kagari_source::source_database::SourceDatabase;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::collections::HashSet;

pub(super) fn history(
    program: &VerifiedMirProgram,
) -> (DefinitionMetadata<DemandEnvelope>, Vec<NamedDeclaration>) {
    let cancel = CancellationToken::default();
    let mut budget = Budget::new(Default::default(), &cancel).unwrap();
    let demands = publication_demands(program.update_evidence(), &mut budget).unwrap();
    let headers = program
        .modules()
        .iter()
        .flat_map(|module| module.abi.named_declarations.iter().cloned())
        .filter(|record| matches!(record.contract, NamedContract::Implementation(_)))
        .collect::<Vec<_>>();
    // Runtime keeps comparison-scoped metadata, not the previous compiler's IDs.
    let mut correspondence = DeclarationCorrespondence::new().unwrap();
    let demands = correspondence
        .import(
            &demands,
            headers.iter().cloned(),
            program.definitions(),
            &cancel,
        )
        .unwrap();
    let headers = correspondence
        .import(
            &headers,
            headers.iter().cloned(),
            program.definitions(),
            &cancel,
        )
        .unwrap();
    let definitions = headers.definitions().clone();
    let mut seen = HashSet::new();
    let headers = headers
        .into_records()
        .into_iter()
        .filter(|record| seen.insert(record.declaration))
        .collect();
    (
        DefinitionMetadata::checked(definitions, demands.into_records(), &cancel).unwrap(),
        headers,
    )
}

#[test]
fn retained_cross_module_calls_compile_after_source_use_disappears() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "provider",
        "pub fn kept<T>(value: T) -> T { value }",
    );
    let root = insert(
        &mut sources,
        "root",
        "use pkg::provider; fn main() -> i32 { provider::kept(41) }",
    );
    let original = lower_program_to_mir(&checked(&sources, root), &Default::default()).unwrap();
    let (history, headers) = history(&original);
    insert(
        &mut sources,
        "root",
        "use pkg::provider; fn main() -> i32 { 2 }",
    );
    let source = checked(&sources, root);
    let options = MirLoweringOptions {
        optimization: Some(Default::default()),
        ..Default::default()
    };
    let candidate =
        lower_program_to_mir_with_demands(&source, &options, &history, &headers).unwrap();
    let owner = candidate
        .modules()
        .iter()
        .find(|module| module.identity.path == ["provider"])
        .unwrap();
    let kept = owner
        .functions
        .iter()
        .find(|function| {
            candidate
                .definitions()
                .resolve(function.instance.declaration)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "kept")
        })
        .unwrap();
    assert_eq!(kept.instance.arguments, [Ty::Builtin(BuiltinType::I32)]);
    assert!(
        candidate
            .update_evidence()
            .targets
            .iter()
            .any(|target| target.body == kept.instance)
    );
    let bytes = encode_program(&candidate, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&decoded, &Default::default()).unwrap();
    assert_eq!(
        bytecode.program().update_evidence.sources.len(),
        candidate.update_evidence().sources.len()
    );
    assert_eq!(
        bytecode.program().update_evidence.bodies.len(),
        candidate.update_evidence().bodies.len()
    );
    for (before, after) in decoded
        .update_evidence()
        .bodies
        .iter()
        .zip(&bytecode.program().update_evidence.bodies)
    {
        assert_eq!(before.format, after.format);
        assert_eq!(before.instructions, after.instructions);
        assert_eq!(before.classification, after.classification);
    }
    let limited = MirLoweringOptions {
        max_generic_instances: 0,
        ..Default::default()
    };
    assert!(lower_program_to_mir_with_demands(&source, &limited, &history, &headers).is_err());
    insert(&mut sources, "provider", "pub fn other() -> i32 { 0 }");
    assert!(
        lower_program_to_mir_with_demands(&checked(&sources, root), &options, &history, &headers)
            .is_err()
    );
}

#[test]
fn publication_keeps_preoptimization_tokens_and_executable_recipe() {
    let mut sources = SourceDatabase::default();
    let root = insert(
        &mut sources,
        "root",
        include_str!("../../../tests/fixtures/updates/publication.kgr"),
    );
    let source = checked(&sources, root);
    let program = lower_program_to_mir(
        &source,
        &MirLoweringOptions {
            optimization: Some(Default::default()),
            ..Default::default()
        },
    )
    .unwrap();
    let definitions = program.definitions();
    let body = program
        .update_evidence()
        .bodies
        .iter()
        .find(|body| {
            definitions
                .resolve(body.requirement.origin.declaration)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "folded")
        })
        .unwrap();
    assert_ne!(body.instructions, body.actual_instructions);
    let initializer = program
        .update_evidence()
        .sources
        .iter()
        .find(|source| {
            definitions
                .resolve(source.declaration)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "N")
        })
        .unwrap();
    assert!(initializer.tokens.windows(7).any(|tokens| {
        tokens
            .iter()
            .map(|token| token.text.as_str())
            .eq(["(", "(", "1", "+", "2", ")", ")"])
    }));
    let bytecode = lower_program_to_bytecode(&program, &Default::default()).unwrap();
    let projected = bytecode
        .program()
        .update_evidence
        .bodies
        .iter()
        .find(|candidate| candidate.requirement.origin == body.requirement.origin)
        .unwrap();
    assert_eq!(projected.instructions, body.instructions);
    assert_eq!(projected.format, body.format);
    assert_ne!(projected.actual_format, body.actual_format);
    let classifications = bytecode
        .program()
        .update_evidence
        .bodies
        .iter()
        .map(|body| &body.classification)
        .collect::<Vec<_>>();
    for name in ["local", "asynchronous"] {
        assert!(classifications.iter().any(|kind| matches!(kind, BodyClassification::Named(id) if definitions.resolve(*id).unwrap().last().is_some_and(|part| part.name == name))));
    }
    assert!(
        classifications
            .iter()
            .any(|kind| matches!(kind, BodyClassification::Closure(_)))
    );
    assert!(
        classifications
            .iter()
            .any(|kind| matches!(kind, BodyClassification::Async(_)))
    );
    assert!(
        classifications
            .iter()
            .any(|kind| matches!(kind, BodyClassification::Resume(_)))
    );
    assert!(
        classifications
            .iter()
            .any(|kind| matches!(kind, BodyClassification::StaticInitializer(_)))
    );
}
