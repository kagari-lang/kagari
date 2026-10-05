//! Bind public applied interface members once; calls use verified table ordinals.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::{ScopedSignature, TypeArgument},
    module::{LoadedModule, ModuleEpochRetention, retention::ProgramLease},
    native::{
        binding::NativeResult,
        conversion::{KagariType, arguments::KagariArguments, context::ConversionContext},
        interfaces::{
            Interface,
            applications::{ApplicationEvidence, AppliedMethod},
        },
    },
};
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
use kagari_contract::{callable::interface::InterfaceCallContract, types::PublicItem};
use kagari_types::{
    declaration::TraitDef,
    ty::{Ty, inheritance::interface_views},
};
use std::{
    any::TypeId,
    collections::BTreeMap,
    marker::PhantomData,
    sync::{Arc, Weak},
};

#[derive(Debug, Clone)]
pub struct InterfaceMember {
    pub(super) source: TypeArgument,
    pub(super) interface: TypeArgument,
    pub(super) owner: LoadedModule,
    pub(super) slot: usize,
    declaration: DefinitionPath,
    pub(super) contract: TraitDef,
    _program: ProgramLease,
}

impl InterfaceMember {
    pub fn declaration(&self) -> &DefinitionPath {
        &self.declaration
    }
}

#[derive(Debug)]
pub struct InterfaceMethod<A, R> {
    pub(super) record: Arc<MethodRecord>,
    mapping: PhantomData<fn(A) -> R>,
}

impl<A, R> Clone for InterfaceMethod<A, R> {
    fn clone(&self) -> Self {
        Self {
            record: self.record.clone(),
            mapping: PhantomData,
        }
    }
}

#[derive(Debug)]
pub(crate) struct MethodRecord {
    pub(super) member: InterfaceMember,
    pub(super) signature: ScopedSignature,
    pub(super) application: Option<AppliedMethod>,
    mapping: TypeId,
}

#[derive(Debug, Default)]
pub(crate) struct InterfaceCache(Vec<Weak<MethodRecord>>);

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("public interface member has no supported checked signature")
}

impl<S> Interface<S> {
    /// Look up a public declaration, including inherited members. Ambiguous
    /// names require selecting the declaring trait explicitly.
    pub fn member(&self, runtime: &Runtime, name: &str) -> NativeResult<InterfaceMember> {
        if !self.root.is_valid(runtime.gc()) {
            return Err(invalid());
        }
        runtime.interface_member(&self.owner, &self.argument, name)
    }

    pub fn member_declaration(
        &self,
        runtime: &Runtime,
        declaration: &DefinitionPath,
    ) -> NativeResult<InterfaceMember> {
        if !self.root.is_valid(runtime.gc()) {
            return Err(invalid());
        }
        runtime.interface_member_declaration(&self.owner, &self.argument, declaration)
    }
}

fn resolve_member(
    runtime: &Runtime,
    source_owner: &LoadedModule,
    source: &TypeArgument,
    accepts: impl Fn(&DefinitionPath, &str) -> bool,
) -> NativeResult<InterfaceMember> {
    runtime.gc().ensure_no_native_borrow()?;
    runtime.validate_loaded_module(source_owner)?;
    source.validate(runtime)?;
    let catalog = &runtime.native_entries.catalog;
    let mut contracts = BTreeMap::new();
    for owner in source_owner.members() {
        for item in &owner.bytecode.public_items {
            if let PublicItem::Trait(contract) = item {
                let declaration = DefinitionPath {
                    module: owner.bytecode.identity.clone(),
                    path: vec![DefinitionPathSegment {
                        kind: DefinitionKind::Trait,
                        name: contract.name.clone(),
                        occurrence: 0,
                    }],
                };
                contracts.insert(declaration, (catalog.paths(contract)?, true));
            }
        }
        for contract in &owner.bytecode.trait_contracts {
            contracts
                .entry(owner.definition(contract.declaration)?.to_path())
                .or_insert((catalog.paths(&contract.abi)?, false));
        }
    }
    let Ty::Trait(root) = catalog.paths(source.ty())? else {
        return Err(invalid());
    };
    if !contracts
        .get(&root.declaration)
        .is_some_and(|(_, public)| *public)
    {
        return Err(invalid());
    }
    let lookup = |id: &DefinitionPath| contracts.get(id).map(|(contract, _)| contract);
    let ancestry = interface_views(
        &root,
        &Ty::Trait(root.clone()),
        &Default::default(),
        &lookup,
    )
    .map_err(|_| invalid())?;
    let mut found = None;
    for interface in ancestry {
        let (contract, public) = contracts.get(&interface.declaration).ok_or_else(invalid)?;
        if !public {
            continue;
        }
        for (slot, method) in contract.methods.iter().enumerate() {
            let mut declaration = interface.declaration.clone();
            declaration.path.push(DefinitionPathSegment {
                kind: DefinitionKind::Method,
                name: method.name.clone(),
                occurrence: 0,
            });
            if !accepts(&declaration, &method.name) {
                continue;
            }
            if found.is_some() {
                return Err(RuntimeError::module_validation(
                    "ambiguous inherited interface member",
                ));
            }
            let argument = source.derive(runtime, source_owner, |expression| {
                let Ty::Trait(root) = catalog.paths(expression).ok()? else {
                    return None;
                };
                let ancestry = interface_views(
                    &root,
                    &Ty::Trait(root.clone()),
                    &Default::default(),
                    &lookup,
                )
                .ok()?;
                let parent = ancestry
                    .into_iter()
                    .find(|parent| parent.declaration == interface.declaration)?;
                catalog.scope(&Ty::Trait(parent)).ok()
            })?;
            let program = runtime
                .retain_program(source_owner, ModuleEpochRetention::RuntimeValue)
                .ok_or_else(invalid)?;
            found = Some(InterfaceMember {
                source: source.clone(),
                interface: argument,
                owner: source_owner.clone(),
                slot,
                declaration,
                contract: contract.clone(),
                _program: program,
            });
        }
    }
    found.ok_or_else(invalid)
}

impl Runtime {
    /// Prepare against an installed applied trait before constructing a value.
    pub fn interface_member(
        &self,
        owner: &LoadedModule,
        interface: &TypeArgument,
        name: &str,
    ) -> NativeResult<InterfaceMember> {
        resolve_member(self, owner, interface, |_, method| method == name)
    }

    pub fn interface_member_declaration(
        &self,
        owner: &LoadedModule,
        interface: &TypeArgument,
        declaration: &DefinitionPath,
    ) -> NativeResult<InterfaceMember> {
        resolve_member(self, owner, interface, |candidate, _| {
            candidate == declaration
        })
    }

    pub fn bind_interface_method<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        member: &InterfaceMember,
    ) -> NativeResult<InterfaceMethod<A, R>> {
        self.bind_interface_method_application(member, &[])
    }

    /// Prepare a method-local application using installed, checked call evidence.
    pub fn bind_interface_method_application<
        A: KagariArguments + 'static,
        R: KagariType + 'static,
    >(
        &self,
        member: &InterfaceMember,
        arguments: &[TypeArgument],
    ) -> NativeResult<InterfaceMethod<A, R>> {
        self.gc().ensure_no_native_borrow()?;
        self.validate_loaded_module(&member.owner)?;
        member.source.validate(self)?;
        if member.contract.methods[member.slot].generic_params.len() != arguments.len() {
            return Err(invalid());
        }
        let evidence = if arguments.is_empty() {
            None
        } else {
            Some(ApplicationEvidence::find(self, member, arguments)?)
        };
        let signature = match &evidence {
            Some(evidence) => evidence.signature(self, member)?,
            None => self.closed_interface_signature(member)?,
        };
        let cx = ConversionContext::new(self, &member.owner)?;
        A::check_types(&cx, &signature.params)?;
        cx.check_type::<R>(&signature.result)?;
        let mapping = TypeId::of::<(A, R)>();
        let mut cache = self
            .interface_bindings
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("interface binding cache is borrowed"))?;
        cache.0.retain(|entry| entry.strong_count() != 0);
        if let Some(record) = cache.0.iter().filter_map(Weak::upgrade).find(|record| {
            let previous = &record.member;
            record.mapping == mapping
                && record
                    .application
                    .as_ref()
                    .map(|application| application.key)
                    == evidence.as_ref().map(|evidence| evidence.key)
                && previous.slot == member.slot
                && previous.owner.program_root().key() == member.owner.program_root().key()
                && previous.interface.ty() == member.interface.ty()
                && previous.source.ty() == member.source.ty()
                && previous
                    .source
                    .view(&previous.owner)
                    .compatible(member.source.view(&member.owner))
        }) {
            return Ok(InterfaceMethod {
                record,
                mapping: PhantomData,
            });
        }
        let record = Arc::new(MethodRecord {
            member: member.clone(),
            signature,
            application: evidence
                .as_ref()
                .map(|evidence| evidence.prepare(self, member))
                .transpose()?,
            mapping,
        });
        cache
            .0
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("interface binding cache"))?;
        cache.0.push(Arc::downgrade(&record));
        Ok(InterfaceMethod {
            record,
            mapping: PhantomData,
        })
    }
    fn closed_interface_signature(
        &self,
        member: &InterfaceMember,
    ) -> NativeResult<ScopedSignature> {
        let catalog = &self.native_entries.catalog;
        let function = member.interface.derive(self, &member.owner, |expression| {
            let Ty::Trait(interface) = catalog.paths(expression).ok()? else {
                return None;
            };
            let call = InterfaceCallContract {
                normalizations: vec![],
                receiver: None,
                operations: vec![],
                interface,
                method_slot: u32::try_from(member.slot).ok()?,
                arguments: vec![],
            };
            let signature = call.signature(&member.contract, &Default::default()).ok()?;
            catalog
                .scope(&Ty::Function {
                    params: signature.params,
                    result: Box::new(signature.result),
                })
                .ok()
        })?;
        let Ty::Function { params, .. } = function.ty() else {
            return Err(invalid());
        };
        let params = (1..params.len())
            .map(|index| {
                function.derive(self, &member.owner, |ty| match ty {
                    Ty::Function { params, .. } => params.get(index).cloned(),
                    _ => None,
                })
            })
            .collect::<NativeResult<Vec<_>>>()?;
        let result = function.derive(self, &member.owner, |ty| match ty {
            Ty::Function { result, .. } => Some(*result.clone()),
            _ => None,
        })?;
        Ok(ScopedSignature { params, result })
    }
}
