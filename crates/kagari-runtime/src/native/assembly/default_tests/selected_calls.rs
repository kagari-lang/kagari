//! Execute selected calls, including the provenance retained by declared bindings.
use crate::{
    Runtime,
    frame::{
        cursor::kernel::RegionExit,
        driver::{ExecutionAction, ReturnAction},
    },
    module::ExecutableModule,
    native::{
        adapter::NativeAdapter,
        assembly::{
            ModuleAssembly, ModuleDefinition, ModuleSetBuilder, authoring::PendingBinding, conflict,
        },
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::ScriptCall,
        declarations::{CallableRequirement, MethodDecl, SelectedCall},
        typed::NativeContext,
        types::{Type, same_definition},
    },
    session::ExecutionEvent,
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, NativeDeclaration, requirement::NativeCallableRequirement},
    ty::{NominalTy, Ty},
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Copy, Debug)]
enum Provenance {
    Authored,
    DeclaredAuthored,
    Sealed,
    ImportedSealed,
    ForeignTarget,
    ForeignScope,
}

#[derive(Clone, Default)]
struct Counts {
    factories: Arc<AtomicUsize>,
    prepares: Arc<AtomicUsize>,
    defaults: Arc<AtomicUsize>,
    selected: Arc<AtomicUsize>,
}

impl Counts {
    fn assert(&self, factories: usize, prepares: usize, defaults: usize, selected: usize) {
        assert_eq!(self.factories.load(Ordering::SeqCst), factories);
        assert_eq!(self.prepares.load(Ordering::SeqCst), prepares);
        assert_eq!(self.defaults.load(Ordering::SeqCst), defaults);
        assert_eq!(self.selected.load(Ordering::SeqCst), selected);
    }
}

struct Adapter {
    provenance: Provenance,
    token: SelectedCall,
    target: DefinitionMetadata<NativeDeclaration>,
    authored_requirement: DefinitionMetadata<NativeCallableRequirement>,
    counts: Counts,
}

impl NativeAdapter<(), ()> for Adapter {
    fn prepare(
        self,
        authored: &FnDecl,
        catalog: &DeclarationCatalog,
    ) -> NativeResult<NativeBinding> {
        self.counts.prepares.fetch_add(1, Ordering::SeqCst);
        assert_eq!(authored.name, "answer");
        assert_eq!(authored.params[0].name, "self");
        assert!(matches!(authored.params[0].ty, Ty::SelfType(_)));
        assert!(
            authored.generic_params.is_empty(),
            "adapter receives authored, not template binders"
        );
        let authored_owner = self.token.declaration;
        let token = self.token;
        let defaults = self.counts.defaults;
        let entry = move |cx: &mut NativeContext<'_>, (value,): (i32,)| {
            defaults.fetch_add(1, Ordering::SeqCst);
            let selected = cx.selected_method::<(i32,), i32>(&token)?;
            selected.call(cx, (value,))
        };
        match self.provenance {
            Provenance::Authored => Ok(NativeBinding::contextual(1, entry)),
            Provenance::DeclaredAuthored => {
                let declaration = DefinitionMetadata::checked(
                    catalog.definitions(),
                    NativeDeclaration {
                        declaration: authored_owner,
                        function: authored.clone(),
                        concrete_result: None,
                        callable_requirements: vec![self.authored_requirement.records().clone()],
                    },
                    &Default::default(),
                )
                .map_err(conflict)?;
                Ok(NativeBinding::declared(&declaration, entry))
            }
            _ => Ok(NativeBinding::declared(&self.target, entry)),
        }
    }
}

fn owner() -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("application".into()),
        path: vec!["tokens".into()],
    }
}

fn assembly(provenance: Provenance, counts: Counts) -> NativeResult<ModuleAssembly> {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default())?;
    set.add(ModuleDefinition::new(owner(), move |module| {
        let interface_id = module.definition(DefinitionKind::Trait, "Run")?;
        let base_id = module.member(interface_id, DefinitionKind::Method, "base")?;
        module.member(interface_id, DefinitionKind::Method, "answer")?;
        let base = module.trait_method(interface_id, MethodDecl::instance("base").returns(Type::i32()))?;
        let interface = module.define_trait("Run", 0, |record, _| {
            record.methods.push(base);
            Ok(())
        })?;
        let requirement = DefinitionMetadata::checked(module.definitions(), NativeCallableRequirement {
            receiver: Ty::SelfType(interface_id),
            interface: NominalTy { declaration: interface_id, arguments: vec![], associated_types: BTreeMap::new() },
            member: base_id, arguments: vec![],
        }, &Default::default()).map_err(conflict)?;
        let configured_requirement = requirement.clone();
        let token = Rc::new(RefCell::new(None));
        let configured_token = token.clone();
        let factory_counts = counts.clone();
        module.define_trait_default(&interface,
            MethodDecl::instance("answer").returns(Type::i32())
                .configure(move |function| {
                    *configured_token.borrow_mut() = Some(function.requires(CallableRequirement { requirement: configured_requirement })?);
                    Ok(())
                })
                .implementation_with(move |preparation| {
                    factory_counts.factories.fetch_add(1, Ordering::SeqCst);
                    let sealed = preparation.declaration();
                    let records = sealed.records();
                    assert!(matches!(records.function.implementation, CallableImplementation::Native(id) if id == records.declaration));
                    assert_eq!(records.function.generic_params.len(), 1);
                    assert_eq!(records.callable_requirements.len(), 1);
                    assert_eq!(preparation.catalog().declarations.get_id(records.declaration), Some(records));
                    let target = match provenance {
                        Provenance::ImportedSealed | Provenance::ForeignScope => {
                            let context = DefinitionContext::new().map_err(conflict)?;
                            let imported = sealed.import_into(&context, &Default::default()).map_err(conflict)?;
                            if matches!(provenance, Provenance::ForeignScope) {
                                let root = context.intern_root(&ModuleIdentity { package: PackageId("foreign".into()), path: vec!["tokens".into()] }).map_err(conflict)?;
                                let foreign = context.intern_child(root, DefinitionKind::Function, "answer", 0).map_err(conflict)?;
                                let mut records = imported.into_records();
                                records.declaration = foreign;
                                DefinitionMetadata::checked(context.snapshot(), records, &Default::default()).map_err(conflict)?
                            } else { imported }
                        }
                        Provenance::ForeignTarget => {
                            let other = preparation.catalog().declarations.values()
                                .find(|declaration| declaration.declaration != records.declaration)
                                .expect("a different real native target").clone();
                            DefinitionMetadata::checked(sealed.definitions().clone(), other, &Default::default()).map_err(conflict)?
                        }
                        _ => sealed.clone(),
                    };
                    let token = match provenance {
                        Provenance::Sealed | Provenance::ImportedSealed => SelectedCall::from_declaration(&target, &target.records().callable_requirements[0])?,
                        _ => token.borrow().as_ref().expect("configured authored token").clone(),
                    };
                    Ok(Adapter { provenance, token, target, authored_requirement: requirement, counts: factory_counts })
                })
        )?;
        let selected_calls = counts.selected;
        module.trait_implementation(Type::i32(), interface.apply([])?, vec![("base".into(), PendingBinding::implementation(
            NativeBinding::new(vec![Codec::Exact(Type::i32())], Codec::Exact(Type::i32()), move |cx| {
                selected_calls.fetch_add(1, Ordering::SeqCst);
                let Value::I32(value) = cx.argument(0)? else { return Err(conflict("expected i32 receiver")); };
                Ok(Value::I32(value + 2))
            })
        ))])
    }))?;
    set.finish()
}

// Use the runtime's ordinary frame driver for native-only synchronous reentry.
// The selected operation is an installed native override, not a simulated result.
fn invoke_selected(
    runtime: &Runtime,
    _: &ExecutableModule,
    target: ScriptCall<'_>,
    arguments: &[Value],
) -> NativeResult<Value> {
    let ScriptCall::Pinned(function) = target else {
        panic!("test only selects a pinned native operation")
    };
    let stack = runtime.enter_pinned_execution_stack(function)?;
    stack.push_pinned_call(runtime, function, arguments)?;
    stack.start_native_entry(runtime, invoke_selected)?;
    let ReturnAction::Complete(result) = stack.finish_native_return(runtime)? else {
        panic!("selected return")
    };
    Ok(result)
}

// Enter through the compiled caller so shared defaults receive their checked
// generic environment and selected operations from the actual call site.
fn execute_main(runtime: &Runtime, loaded: &ExecutableModule) -> Value {
    let (owner, entry) = loaded.entry_function().expect("script main");
    let stack = runtime.enter_execution_stack(&owner).unwrap();
    stack
        .push_callable(
            runtime,
            owner.clone(),
            CallableTarget::Script(entry),
            &[],
            None,
        )
        .unwrap();
    for _ in 0..32 {
        runtime.resources().poll_execution().unwrap();
        let returned = match stack.next_action(runtime).unwrap() {
            ExecutionAction::Script => {
                stack.current_mut().unwrap().prepare_instruction();
                runtime.gc_safepoint().unwrap();
                runtime
                    .observe_execution(ExecutionEvent::BeforeInstruction)
                    .unwrap();
                match stack
                    .execute_region(runtime, &mut None, invoke_selected)
                    .unwrap()
                {
                    RegionExit::Return(value) => stack.finish_return(runtime, value).unwrap(),
                    RegionExit::Call(_) | RegionExit::Safepoint => continue,
                    RegionExit::Boundary => panic!("unexpected script boundary"),
                    RegionExit::Slice => panic!("unsliced execution cannot exhaust a slice"),
                }
            }
            ExecutionAction::NativeEntry => {
                runtime.gc_safepoint().unwrap();
                runtime
                    .observe_execution(ExecutionEvent::BeforeInstruction)
                    .unwrap();
                stack.start_native_entry(runtime, invoke_selected).unwrap();
                continue;
            }
            ExecutionAction::NativeReturn => stack.finish_native_return(runtime).unwrap(),
            ExecutionAction::Await => panic!("synchronous default cannot await"),
        };
        if let ReturnAction::Complete(value) = returned {
            return value;
        }
    }
    panic!("default call did not complete within the test transition bound")
}

#[test]
fn authored_and_sealed_tokens_execute_the_actual_selected_default_call() {
    for provenance in [
        Provenance::Authored,
        Provenance::DeclaredAuthored,
        Provenance::Sealed,
        Provenance::ImportedSealed,
    ] {
        let counts = Counts::default();
        let assembly = assembly(provenance, counts.clone()).unwrap();
        let snapshot = assembly.declarations();
        counts.assert(0, 0, 0, 0);
        let default = &snapshot.modules()[0].records().traits[0].methods[1];
        let CallableImplementation::NativeDefault(application) = &default.implementation else {
            panic!("default must be lowered")
        };
        let template = application.declaration;
        let definitions = snapshot.modules()[0].definitions();
        let modules = assembly.into_modules().unwrap();
        counts.assert(1, 1, 0, 0);
        let mut analysis = AnalysisDatabase::default();
        analysis.set_native_modules(snapshot.modules().iter().cloned().map(Arc::new).collect());
        let mut sources = SourceDatabase::default();
        let root = sources
            .set(
                "selected-default.kgr",
                "use application::tokens::Run; fn main() -> i32 { 40.answer() }".into(),
                SourceLayer::Base,
            )
            .unwrap();
        let checked = analysis
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap()
            .check_program(root, &Default::default())
            .unwrap();
        let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
        let bytecode = lower_program_to_bytecode(&mir, &Default::default())
            .map(|program| program.into_unverified())
            .unwrap();
        let mut runtime = Runtime::default();
        for module in modules {
            module.install(&mut runtime).unwrap();
        }
        let loaded_program = runtime.load_program("selected-default", bytecode).unwrap();
        let loaded = runtime.select_program(&loaded_program).unwrap();
        loaded
            .members()
            .find_map(|owner| {
                owner
                    .bytecode
                    .native_imports
                    .iter()
                    .position(|import| {
                        same_definition(
                            import.instance.declaration,
                            owner.definitions(),
                            template,
                            definitions,
                        )
                    })
                    .map(|slot| (owner, slot))
            })
            .expect("compiler-selected default import");
        let result = execute_main(&runtime, &loaded);
        assert_eq!(result, Value::I32(42), "{provenance:?}");
        counts.assert(1, 1, 1, 1);
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}

#[test]
fn foreign_requirement_owners_are_rejected_before_default_execution() {
    for provenance in [Provenance::ForeignTarget, Provenance::ForeignScope] {
        let counts = Counts::default();
        let assembly = assembly(provenance, counts.clone()).unwrap();
        let snapshot = assembly.declarations();
        counts.assert(0, 0, 0, 0);
        let error = assembly
            .into_modules()
            .expect_err("foreign owner must not be silently replaced");
        assert!(
            error
                .message()
                .contains("foreign native default requirement owner"),
            "{error:?}"
        );
        counts.assert(1, 1, 0, 0);
        assert_eq!(snapshot.modules().len(), 1);
    }
}
