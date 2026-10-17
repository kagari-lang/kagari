//! Explicit Kagari trait registration can use a Rust trait as its callback body.
//! Run with `cargo run -p kagari-runtime --example host_trait_table`.
use kagari_bytecode::{artifact::KbcArtifact, instruction::NativeImportId, module::CallableTarget};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::types::PublicItem;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    frame::driver::ReturnAction,
    native::{
        builder::ModuleBuilder,
        conversion::context::ConversionContext,
        declarations::MethodDecl,
        module::NativeModule,
        payload::Opaque,
        typed::{NativeContext, loans::LoanContext},
        types::Type,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{declarations::StandardDeclarations, modules};
use kagari_types::{reference::ReferenceAccess, ty::Ty};
use std::{error::Error, sync::Arc};

kagari_runtime::native_data! { struct Counter { value: i32 } }

trait RustReadable {
    fn read(&self) -> i32;
}

impl RustReadable for Counter {
    fn read(&self) -> i32 {
        self.value
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut module = ModuleBuilder::new("demo", &StandardDeclarations::default().catalog()?)?;
    let counter = module.register_type::<Opaque<Counter>>()?;
    // RustReadable alone exposes nothing. Declare the Kagari trait explicitly.
    let mut readable = module.define_trait("Readable")?;
    readable.define_method(MethodDecl::instance("get").returns(Type::i32()))?;
    let readable = readable.finish()?;
    module.implement(counter.clone(), |group| {
        group.trait_impl(readable.apply([])?, |methods| {
            methods.receiver_access("get", ReferenceAccess::Shared)?;
            methods.bind("get", |_: &mut LoanContext<'_>, (counter,): (&Counter,)| {
                Ok(RustReadable::read(counter))
            })
        })
    })?;
    let mut modules = modules()?;
    modules.push(module.finish()?);
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let mut sources = SourceDatabase::default();
    let root = sources.set(
        "trait-table.kgr",
        "use demo::Counter; use demo::Readable; pub fn read(value: Counter) -> i32 { value.get() }"
            .into(),
        SourceLayer::Base,
    )?;
    let snapshot = analysis.snapshot(sources.snapshot(), &Default::default())?;
    let checked = snapshot
        .check_program(root, &Default::default())
        .expect("checked explicit trait impl");
    let mir = lower_program_to_mir(&checked, &Default::default()).expect("checked MIR");
    let bytecode = lower_program_to_bytecode(&mir).expect("checked bytecode");
    // The artifact contains declarations and checked trait tables, never Rust code.
    let definitions = DefinitionContext::new()?;
    let bytecode = bytecode.import_into(&definitions, &Default::default())?;
    let artifact =
        KbcArtifact::from_program(bytecode.into_records(), Default::default(), &definitions)?;
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&definitions.snapshot())?)?;
    let decoded = decoded.import_into(&definitions, &Default::default())?;
    decoded
        .records()
        .validate_for_loader(&Default::default(), &definitions)?;
    let (snapshot, decoded) = decoded.into_parts();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules, &mut runtime)?;
    let loaded = runtime.load_program(
        "trait-table",
        DefinitionMetadata::checked(snapshot, decoded.program, &Default::default())?,
    )?;
    let tables: usize = loaded
        .members()
        .map(|member| {
            member
                .bytecode
                .public_items
                .iter()
                .filter(|item| {
                    let PublicItem::InterfaceTable(table) = item else {
                        return false;
                    };
                    let Ty::Trait(interface) = &table.trait_type else {
                        return false;
                    };
                    member
                        .definitions()
                        .resolve(interface.declaration)
                        .ok()
                        .is_some_and(|id| {
                            id.module().package.0 == "demo"
                                && id.last().is_some_and(|part| part.name == "Readable")
                        })
                })
                .count()
        })
        .sum();
    assert_eq!(tables, 1);
    let Ty::NativeObject(nominal) = counter.abi() else {
        unreachable!("registered Opaque type")
    };
    let descriptor = runtime.bind_native_type_declaration::<Counter>(
        &loaded,
        nominal.declaration,
        counter.definitions().expect("registered schema"),
        &[],
    )?;
    let owned = descriptor.create(
        &mut NativeContext::new(&runtime, &loaded)?,
        Counter { value: 42 },
    )?;
    let encoded = ConversionContext::new(&runtime, &loaded)?.encode(owned.clone())?;
    let value = encoded.value(runtime.gc()).expect("retained Counter");
    let (owner, slot) = loaded
        .members()
        .find_map(|member| {
            member
                .bytecode
                .native_imports
                .iter()
                .position(|import| {
                    member
                        .definitions()
                        .resolve(import.instance.declaration)
                        .ok()
                        .and_then(|id| id.last())
                        .is_some_and(|part| part.name == "get")
                })
                .map(|slot| (member, slot))
        })
        .expect("checked Readable::get application");
    let stack = runtime.enter_execution_stack(&owner)?;
    stack.push_callable(
        &runtime,
        owner.clone(),
        CallableTarget::Native(NativeImportId::new(slot)),
        &[value],
        None,
    )?;
    stack.start_native_entry(&runtime, |_, _, _, _| {
        unreachable!("Rust read never calls script")
    })?;
    assert_eq!(
        stack.finish_native_return(&runtime)?,
        ReturnAction::Complete(Value::I32(42))
    );
    drop(stack);
    drop(encoded);
    drop(owned);
    assert_eq!(runtime.gc().active_roots(), 0);
    println!("linked one explicit Kagari trait impl; its Rust trait callback returned 42");
    Ok(())
}
