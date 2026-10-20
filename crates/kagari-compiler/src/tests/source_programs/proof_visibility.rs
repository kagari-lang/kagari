use super::{bytecode, checked, insert};
use crate::source::program::lower_program_to_mir;
use kagari_bytecode::{
    program::{ModuleRef, verify_program},
    verifier::BytecodeVerificationError,
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext};
use kagari_contract::types::PublicItem;
use kagari_source::source_database::SourceDatabase;

#[test]
fn linked_proofs_use_each_modules_dependency_visibility() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "types",
        "pub trait Tag {} pub struct Token {}",
    );
    insert(
        &mut sources,
        "provider",
        "use pkg::types::{Tag, Token}; impl Tag for Token {}",
    );
    insert(
        &mut sources,
        "consumer",
        "use pkg::types::{Tag, Token}; use pkg::provider; pub trait Read { type Item: Tag; } pub struct Reader {} impl Read for Reader { type Item = Token; } pub fn answer() -> i32 { 42 }",
    );
    let root = insert(
        &mut sources,
        "root",
        "use pkg::consumer; use pkg::provider; fn main() -> i32 { consumer::answer() }",
    );
    let mir = lower_program_to_mir(&checked(&sources, root), &Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let mut program = bytecode(&context, &mir);
    let consumer = program
        .modules
        .iter()
        .position(|module| module.identity.path == ["consumer"])
        .unwrap();
    let provider = program
        .modules
        .iter()
        .position(|module| module.identity.path == ["provider"])
        .unwrap();
    // Source lowering carries a package-wide lookup closure. This executable
    // graph fixture narrows it to the explicit dependency edges under test.
    let identities = program
        .modules
        .iter()
        .map(|module| module.identity.clone())
        .collect::<Vec<_>>();
    for module in &mut program.modules {
        let name = module.identity.path.first().map(String::as_str);
        module.dependencies.retain(|dependency| {
            let identity = &identities[dependency.index()];
            identity.package.0 != "pkg"
                || match name {
                    Some("root") => {
                        ["consumer", "provider", "types"].contains(&identity.path[0].as_str())
                    }
                    Some("consumer") => ["provider", "types"].contains(&identity.path[0].as_str()),
                    Some("provider") => identity.path == ["types"],
                    _ => false,
                }
        });
    }
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();

    // The root can still see the provider, but that must not satisfy the
    // consumer's associated-output bound after its own dependency is removed.
    let mut hidden = program.clone();
    hidden.modules[consumer]
        .dependencies
        .retain(|module| module.index() != provider);
    let result = verify_program(&hidden, &context.snapshot(), &Default::default());
    assert!(
        matches!(
            result,
            Err(BytecodeVerificationError::InvalidHostInterface(_))
        ),
        "{result:?}"
    );

    // Add an independently named, otherwise equivalent implementation in a root
    // sibling. It is well formed but invisible to the consumer and must not make
    // the consumer's single available implementation ambiguous.
    let mut competing = program.modules[provider].clone();
    competing.identity = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["competing".into()],
    };
    let owner = context.intern_root(&competing.identity).unwrap();
    for item in &mut competing.public_items {
        let PublicItem::InterfaceTable(table) = item else {
            continue;
        };
        let original = table.declaration;
        let declaration = context
            .intern_child(owner, DefinitionKind::Impl, "", 0)
            .unwrap();
        table.declaration = declaration;
        for linked in &mut competing.interface_tables {
            if linked.declaration == original {
                linked.declaration = declaration;
            }
        }
    }
    let competing_slot = ModuleRef::new(program.modules.len());
    program.modules[program.root.index()]
        .dependencies
        .push(competing_slot);
    program.modules.push(competing);
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();

    // Making that same competing implementation visible introduces an actual
    // ambiguous proof and must be rejected.
    program.modules[consumer].dependencies.push(competing_slot);
    assert!(matches!(
        verify_program(&program, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));
}
