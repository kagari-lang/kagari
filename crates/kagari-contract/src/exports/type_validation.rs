//! Shared source-free authority and application proofs for public type exports.

use crate::{
    exports::{MAX_EXPORT_RECORDS, TypeExport},
    layout::{self, StructLayout},
    types::{PublicItem, applications, proofs::ProofCatalog},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::{
        TraitDef, TypeDeclaration, TypeDef, TypeDefKind,
        applications::ApplicationValidator,
        verify::{bounds_valid, concrete_type_valid, parameters},
    },
    decode_limits::MAX_NESTED_RECORDS,
    ty::{
        GenericBound, GenericParam, NominalTy, Ty, inheritance,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    slice,
};

// One admission budget covers indexing, alias queries and all uncached walks.
// Individual type transforms and linked proof searches retain their own bounds.
const MAX_EXPORT_PROOF_WORK: usize = MAX_EXPORT_RECORDS * 64;

/// Borrowed facts from an independently checked executable module.
/// Private declarations must not be added to `public_items` to satisfy an export.
pub struct TypeExportModule<'a> {
    pub identity: &'a ModuleIdentity,
    pub public_items: &'a [PublicItem],
    /// Admitted private nominal contracts grant dependency proofs, never export authority.
    pub private_types: &'a [TypeDeclaration],
    pub structures: &'a [StructLayout],
}

#[derive(Clone, Copy)]
struct NominalDeclaration<'a> {
    identity: &'a ModuleIdentity,
    item: &'a PublicItem,
    record: &'a TypeDef,
}

/// Export proofs share the executable graph's already admitted linked catalog.
/// No function-local generic assumptions or source declarations grant authority.
/// Indexes and successful-proof caches belong only to this immutable verification
/// graph. Repeated aliases reuse evidence; changing any input requires a new validator.
pub struct TypeExportValidator<'a> {
    public_nominals: HashMap<DefinitionId, NominalDeclaration<'a>>,
    dependency_nominals: HashMap<DefinitionId, &'a TypeDef>,
    public_traits: HashMap<DefinitionId, &'a TraitDef>,
    layouts: HashMap<DefinitionId, Vec<&'a StructLayout>>,
    exact_layouts: HashSet<(DefinitionId, &'a [Ty])>,
    declarations_checked: RefCell<HashSet<DefinitionId>>,
    layouts_checked: RefCell<HashSet<DefinitionId>>,
    binders_checked: RefCell<HashSet<DefinitionId>>,
    public_types_checked: RefCell<HashSet<Ty>>,
    dependency_types_checked: RefCell<HashSet<Ty>>,
    work: Cell<usize>,
    catalog: &'a ProofCatalog<'a>,
    cancel: &'a CancellationToken,
}

impl<'a> TypeExportValidator<'a> {
    /// Index checked declaration/layout facts once, with cancellation and a shared
    /// work bound. Errors never produce a partially initialized proof owner.
    pub fn new(
        modules: impl IntoIterator<Item = TypeExportModule<'a>>,
        catalog: &'a ProofCatalog<'a>,
        cancel: &'a CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        let mut validator = Self {
            public_nominals: HashMap::new(),
            dependency_nominals: HashMap::new(),
            public_traits: HashMap::new(),
            layouts: HashMap::new(),
            exact_layouts: HashSet::new(),
            declarations_checked: RefCell::default(),
            layouts_checked: RefCell::default(),
            binders_checked: RefCell::default(),
            public_types_checked: RefCell::default(),
            dependency_types_checked: RefCell::default(),
            work: Cell::new(MAX_EXPORT_PROOF_WORK),
            catalog,
            cancel,
        };
        for module in modules {
            validator.spend(1)?;
            let root = catalog.definitions().lookup_root(module.identity);
            for item in module.public_items {
                validator.spend(1)?;
                let (kind, name) = match item {
                    PublicItem::Type(record) => (record.kind.definition_kind(), &record.name),
                    PublicItem::Trait(record) => (DefinitionKind::Trait, &record.name),
                    _ => continue,
                };
                let id = root
                    .and_then(|root| catalog.definitions().lookup_child(root, kind, name, 0))
                    .ok_or(TypeTransformError::InvalidContract)?;
                match item {
                    PublicItem::Type(record) => {
                        require(validator.dependency_nominals.insert(id, record).is_none())?;
                        validator.public_nominals.insert(
                            id,
                            NominalDeclaration {
                                identity: module.identity,
                                item,
                                record,
                            },
                        );
                    }
                    PublicItem::Trait(record) => {
                        validator.public_traits.insert(id, record);
                    }
                    _ => unreachable!(),
                }
            }
            for declaration in module.private_types {
                validator.spend(1)?;
                require(
                    validator
                        .dependency_nominals
                        .insert(declaration.declaration, &declaration.definition)
                        .is_none(),
                )?;
            }
            for layout in module.structures {
                validator.spend(1)?;
                for ty in &layout.arguments {
                    validator.charge_type(ty)?;
                }
                validator
                    .exact_layouts
                    .insert((layout.declaration, &layout.arguments));
                validator
                    .layouts
                    .entry(layout.declaration)
                    .or_default()
                    .push(layout);
            }
        }
        Ok(validator)
    }

    /// Prove a leaf against public authority, exact binders and executable layouts.
    /// Invalid authority/applications, work exhaustion and cancellation fail closed.
    pub fn validate(&self, export: &TypeExport) -> Result<(), TypeTransformError> {
        self.spend(1)?;
        match export {
            TypeExport::OpenAlias { arity } => {
                require(*arity > 0 && (*arity as usize) <= MAX_NESTED_RECORDS)
            }
            TypeExport::Closed(ty) => self.concrete(ty, true),
            TypeExport::Declaration(id) => {
                if self.declarations_checked.borrow().contains(id) {
                    return Ok(());
                }
                if let Some(declaration) = self.public_nominals.get(id).copied() {
                    let record = declaration.record;
                    self.binders(*id, &record.generic_params, &record.bounds)?;
                    self.layout_candidates(*id, declaration)?;
                    if record.kind == TypeDefKind::Struct && record.generic_params.is_empty() {
                        self.concrete(
                            &Ty::Struct(NominalTy {
                                declaration: *id,
                                arguments: vec![],
                                associated_types: Default::default(),
                            }),
                            true,
                        )?;
                    }
                } else if let Some(record) = self.public_traits.get(id) {
                    require(parameters(&record.generic_params, id, &Default::default()).is_some())?;
                } else {
                    return Err(TypeTransformError::InvalidContract);
                }
                self.declarations_checked.borrow_mut().insert(*id);
                Ok(())
            }
        }
    }

    fn definitions(&self) -> &DefinitionTable {
        self.catalog.definitions()
    }

    fn spend(&self, count: usize) -> Result<(), TypeTransformError> {
        self.cancel
            .check()
            .map_err(|_| TypeTransformError::Cancelled)?;
        self.work.set(
            self.work
                .get()
                .checked_sub(count)
                .ok_or(TypeTransformError::LimitExceeded)?,
        );
        Ok(())
    }

    fn charge_type(&self, ty: &Ty) -> Result<(), TypeTransformError> {
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            self.spend(1)?;
            children(ty, &mut pending);
        }
        Ok(())
    }

    fn binders(
        &self,
        id: DefinitionId,
        declared: &[GenericParam],
        bounds: &[GenericBound],
    ) -> Result<(), TypeTransformError> {
        if self.binders_checked.borrow().contains(&id) {
            return Ok(());
        }
        self.spend(1 + declared.len() + bounds.len())?;
        let scope = parameters(declared, &id, &Default::default())
            .ok_or(TypeTransformError::InvalidContract)?;
        require(bounds_valid(
            bounds,
            &scope,
            self.cancel,
            self.definitions(),
        ))?;
        self.binders_checked.borrow_mut().insert(id);
        Ok(())
    }

    fn concrete(&self, ty: &Ty, public: bool) -> Result<(), TypeTransformError> {
        self.spend(1)?;
        // Bound trees before hashing or cloning a cache key. Caches distinguish
        // public target authority from dependency-only application evidence.
        require(ty.within_wire_limits())?;
        self.charge_type(ty)?;
        let checked = if public {
            &self.public_types_checked
        } else {
            &self.dependency_types_checked
        };
        if checked.borrow().contains(ty) {
            return Ok(());
        }
        require(concrete_type_valid(ty, self.cancel, self.definitions()) && ty.is_concrete())?;
        let applications = ApplicationValidator::new(
            self.cancel,
            |id| self.catalog.trait_contract(id),
            |id| {
                self.dependency_nominals
                    .get(id)
                    .map(|record| (record.kind, record.generic_params.len()))
            },
        );
        applications::validate_types(&applications, [ty], self.cancel, self.definitions())?;
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            self.spend(1)?;
            match ty {
                Ty::Struct(applied) | Ty::Enum(applied) | Ty::NativeObject(applied) => {
                    if public {
                        require(self.public_nominals.contains_key(&applied.declaration))?;
                    }
                    let record = self
                        .dependency_nominals
                        .get(&applied.declaration)
                        .ok_or(TypeTransformError::InvalidContract)?;
                    self.binders(applied.declaration, &record.generic_params, &record.bounds)?;
                    self.obligations(ty, applied, &record.bounds)?;
                    if public && matches!(ty, Ty::Struct(_)) {
                        self.layout_candidates(
                            applied.declaration,
                            self.public_nominals[&applied.declaration],
                        )?;
                        require(
                            self.exact_layouts
                                .contains(&(applied.declaration, applied.arguments.as_slice())),
                        )?;
                    }
                }
                Ty::Trait(applied) => {
                    if public {
                        require(self.public_traits.contains_key(&applied.declaration))?;
                    }
                    self.trait_obligations(ty, applied)?;
                }
                Ty::Host(id) => {
                    if public {
                        require(self.public_nominals.contains_key(id))?;
                    }
                    let record = self
                        .dependency_nominals
                        .get(id)
                        .ok_or(TypeTransformError::InvalidContract)?;
                    require(
                        matches!(record.kind, TypeDefKind::Host(_))
                            && record.generic_params.is_empty(),
                    )?;
                }
                Ty::Parameter { .. } | Ty::SelfType(_) | Ty::Projection { .. } => {
                    return Err(TypeTransformError::InvalidContract);
                }
                _ => {}
            }
            children(ty, &mut pending);
        }
        checked.borrow_mut().insert(ty.clone());
        Ok(())
    }

    fn trait_obligations(&self, ty: &Ty, applied: &NominalTy) -> Result<(), TypeTransformError> {
        // Private supertraits are dependency proofs, not additional public names.
        for view in
            inheritance::interface_views(applied, ty, self.cancel, self.definitions(), &|id| {
                self.catalog.trait_contract(id)
            })?
        {
            self.spend(1)?;
            let contract = self
                .catalog
                .trait_contract(&view.declaration)
                .ok_or(TypeTransformError::InvalidContract)?;
            self.binders(view.declaration, &contract.generic_params, &contract.bounds)?;
            let mut bounds = contract.bounds.clone();
            for member in &contract.associated_types {
                self.spend(1)?;
                let output = view
                    .associated_types
                    .get(&member.declaration)
                    .ok_or(TypeTransformError::InvalidContract)?;
                bounds.push(GenericBound {
                    ty: output.clone(),
                    constraints: member.bounds.clone(),
                });
            }
            self.obligations(&Ty::Trait(view.clone()), &view, &bounds)?;
        }
        Ok(())
    }

    fn obligations(
        &self,
        ty: &Ty,
        applied: &NominalTy,
        bounds: &[GenericBound],
    ) -> Result<(), TypeTransformError> {
        self.spend(1 + bounds.len())?;
        if bounds.is_empty() {
            return Ok(());
        }
        let mut substitution =
            TypeSubstitution::for_owner(&applied.declaration, &applied.arguments);
        substitution.bind_receiver(&applied.declaration, ty);
        for bound in substitution.apply_bounds(bounds, self.definitions(), self.cancel)? {
            self.charge_type(&bound.ty)?;
            for constraint in &bound.constraints {
                self.spend(1)?;
                for argument in constraint
                    .arguments
                    .iter()
                    .chain(constraint.associated_types.values())
                {
                    self.charge_type(argument)?;
                }
            }
            require(self.catalog.constraints_hold(
                &bound.ty,
                &bound.constraints,
                &[],
                self.cancel,
            )?)?;
        }
        Ok(())
    }

    fn layout_candidates(
        &self,
        id: DefinitionId,
        declaration: NominalDeclaration<'a>,
    ) -> Result<(), TypeTransformError> {
        if self.layouts_checked.borrow().contains(&id) {
            return Ok(());
        }
        self.spend(1)?;
        let record = declaration.record;
        let template = record
            .generic_params
            .iter()
            .map(GenericParam::as_type)
            .collect::<Vec<_>>();
        for layout in self.layouts.get(&id).into_iter().flatten() {
            self.spend(1 + layout.fields.len())?;
            require(
                record.kind == TypeDefKind::Struct
                    && layout.arguments.len() == template.len()
                    && (layout.arguments == template
                        || layout.arguments.iter().all(Ty::is_concrete)),
            )?;
            require(layout.types_valid(self.cancel, self.definitions()))?;
            for field in &layout.fields {
                self.charge_type(&field.ty)?;
            }
            require(
                layout::struct_abi_matches(
                    slice::from_ref(*layout),
                    declaration.identity,
                    slice::from_ref(declaration.item),
                    self.cancel,
                    self.definitions(),
                )
                .map_err(|_| TypeTransformError::Cancelled)?,
            )?;
            if layout.arguments.iter().all(Ty::is_concrete) {
                let applied = NominalTy {
                    declaration: id,
                    arguments: layout.arguments.clone(),
                    associated_types: Default::default(),
                };
                // Walk every nested nominal obligation without demanding layouts
                // recursively. Module verification already admits executable fields;
                // a dependency proof must not export a private declaration.
                self.concrete(&Ty::Struct(applied), false)?;
            }
        }
        self.layouts_checked.borrow_mut().insert(id);
        Ok(())
    }
}

fn children<'a>(ty: &'a Ty, pending: &mut Vec<&'a Ty>) {
    match ty {
        Ty::Struct(applied)
        | Ty::Enum(applied)
        | Ty::NativeObject(applied)
        | Ty::Trait(applied) => {
            pending.extend(&applied.arguments);
            pending.extend(applied.associated_types.values());
        }
        Ty::Reference { target, .. } => pending.push(target),
        Ty::Tuple(items) => pending.extend(items),
        Ty::Function { params, result } => {
            pending.extend(params);
            pending.push(result);
        }
        Ty::Array(item) | Ty::Set(item, _) | Ty::Iter(item) | Ty::Range(item, _) => {
            pending.push(item)
        }
        Ty::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
        Ty::Projection {
            receiver,
            interface,
            arguments,
            ..
        } => {
            pending.push(receiver);
            pending.extend(&interface.arguments);
            pending.extend(interface.associated_types.values());
            pending.extend(arguments);
        }
        Ty::Builtin(_) | Ty::Host(_) | Ty::SelfType(_) | Ty::Parameter { .. } => {}
    }
}

fn require(valid: bool) -> Result<(), TypeTransformError> {
    if valid {
        Ok(())
    } else {
        Err(TypeTransformError::InvalidContract)
    }
}
