//! Owner-scoped collection using the existing signature and implementation lowerers.
pub mod defaults;
mod imported;
mod inherent;
pub mod traits;
use crate::native::{
    assembly::{
        authoring::{MethodDefinition, PendingBinding},
        conflict,
        draft::{
            defaults::DefaultIntent,
            traits::{OverrideIntent, TraitImplementationDefinition},
        },
        function::FunctionDefinition,
        references::{DraftMember, DraftReference, ReferenceKind},
    },
    binding::NativeResult,
    builder::inherent::InherentMethodsBuilder,
    declarations::{FunctionBuilder, MethodDecl, normalize_bounds},
    storage::NativeStorage,
    types::{FunctionRef, Type},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, ModuleIdentity,
        map::DefinitionContext,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, NativeDeclaration, TraitDef, TypeDef, TypeDefKind,
        module::{ImplDecl, ModuleDecl},
        names::ExportName,
        native::NativeTypeConstructor,
    },
    ty::{GenericBound, GenericParam, Ty},
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct DraftState {
    pub(crate) declaration: ModuleDecl,
    pub(crate) bindings: BTreeMap<DefinitionId, PendingBinding>,
    pub(crate) storage: BTreeMap<DefinitionId, NativeStorage>,
    pub(crate) references: Vec<DraftReference>,
    pub(crate) members: Vec<DraftMember>,
    pub(crate) trait_intents: BTreeMap<usize, Vec<OverrideIntent>>,
    pub(crate) default_intents: Vec<DefaultIntent>,
}

impl DraftState {
    pub(crate) fn new(identity: ModuleIdentity) -> Self {
        Self {
            declaration: ModuleDecl::new(identity),
            bindings: BTreeMap::new(),
            storage: BTreeMap::new(),
            references: vec![],
            members: vec![],
            trait_intents: BTreeMap::new(),
            default_intents: vec![],
        }
    }
}

/// A recipe's only mutable view. It cannot change its canonical module owner or
/// inspect incomplete contracts as a checked provider catalog.
pub struct ModuleDraft<'a> {
    context: &'a DefinitionContext,
    state: &'a mut DraftState,
}

impl<'a> ModuleDraft<'a> {
    pub(crate) fn new(context: &'a DefinitionContext, state: &'a mut DraftState) -> Self {
        Self { context, state }
    }

    pub fn identity(&self) -> &ModuleIdentity {
        &self.state.declaration.identity
    }

    pub fn definitions(&self) -> DefinitionTable {
        self.context.snapshot()
    }

    pub fn documentation(&mut self, text: impl Into<String>) {
        self.state.declaration.module_documentation = text.into();
    }

    /// Attach Markdown to a local declaration or member identity. Foreign
    /// scopes and identities owned by another module reject.
    pub fn document(&mut self, id: DefinitionId, text: impl Into<String>) -> NativeResult<()> {
        self.check_local(id)?;
        self.state.declaration.documentation.insert(id, text.into());
        Ok(())
    }

    pub fn package_alias(&mut self, alias: impl Into<String>) {
        self.state.declaration.package_alias = Some(alias.into());
    }

    pub fn prelude(&mut self, enabled: bool) {
        self.state.declaration.prelude = enabled;
    }

    /// Intern a direct local identity, without claiming that it is defined.
    /// Intended for binder/member records, not reservation inventories.
    pub fn definition(&self, kind: DefinitionKind, name: &str) -> NativeResult<DefinitionId> {
        let root = self
            .context
            .intern_root(self.identity())
            .map_err(conflict)?;
        self.context
            .intern_child(root, kind, name, 0)
            .map_err(conflict)
    }

    pub fn member(
        &self,
        owner: DefinitionId,
        kind: DefinitionKind,
        name: &str,
    ) -> NativeResult<DefinitionId> {
        self.check_local(owner)?;
        self.context
            .intern_child(owner, kind, name, 0)
            .map_err(conflict)
    }

    pub fn reference_type(
        &mut self,
        path: DefinitionPath,
        kind: TypeDefKind,
        arity: usize,
    ) -> NativeResult<DraftReference> {
        self.reference(path, ReferenceKind::Type(kind), arity)
    }

    pub fn reference_trait(
        &mut self,
        path: DefinitionPath,
        arity: usize,
    ) -> NativeResult<DraftReference> {
        self.reference(path, ReferenceKind::Trait, arity)
    }

    fn reference(
        &mut self,
        path: DefinitionPath,
        kind: ReferenceKind,
        arity: usize,
    ) -> NativeResult<DraftReference> {
        let expected = match kind {
            ReferenceKind::Type(kind) => kind.definition_kind(),
            ReferenceKind::Trait => DefinitionKind::Trait,
        };
        if path.path.len() != 1 || path.path[0].kind != expected || path.path[0].occurrence != 0 {
            return Err(conflict(
                "forward reference category differs from its identity",
            ));
        }
        let id = self.context.intern(&path).map_err(conflict)?;
        let reference = DraftReference {
            id,
            definitions: self.definitions(),
            kind,
            arity,
        };
        self.state.references.push(reference.clone());
        Ok(reference)
    }

    /// Collect a nominal record under its final binder. The callback runs once;
    /// semantic validation is deferred until reciprocal contracts are available.
    pub fn define_type(
        &mut self,
        name: &str,
        kind: TypeDefKind,
        arity: usize,
        configure: impl FnOnce(&mut TypeDef, &DefinitionTable) -> NativeResult<()>,
    ) -> NativeResult<DraftReference> {
        self.check_type_name(name)?;
        let id = self.definition(kind.definition_kind(), name)?;
        let mut record = TypeDef {
            name: name.into(),
            kind,
            generic_params: (0..arity)
                .map(|position| GenericParam {
                    owner: id,
                    position,
                })
                .collect(),
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        };
        configure(&mut record, &self.definitions())?;
        if record.name != name || record.kind != kind || record.generic_params.len() != arity {
            return Err(conflict("type configuration changed its identity or arity"));
        }
        self.check_scope(&record)?;
        self.state.declaration.types.push(record);
        self.state
            .declaration
            .collect_definitions(self.context)
            .map_err(conflict)?;
        Ok(DraftReference {
            id,
            definitions: self.definitions(),
            kind: ReferenceKind::Type(kind),
            arity,
        })
    }

    /// Constructor metadata establishes builtin receiver ownership, not heap storage.
    pub fn define_constructor(
        &mut self,
        name: &str,
        constructor: NativeTypeConstructor,
    ) -> NativeResult<DraftReference> {
        self.define_type(
            name,
            TypeDefKind::Native(constructor),
            constructor.arity(),
            |_, _| Ok(()),
        )
    }

    /// Collect a trait record, including conversion authority and required methods.
    /// Add complete native defaults with `define_trait_default`; seal lowers their
    /// intents against the complete records using the shared semantic lowerer.
    pub fn define_trait(
        &mut self,
        name: &str,
        arity: usize,
        configure: impl FnOnce(&mut TraitDef, &DefinitionTable) -> NativeResult<()>,
    ) -> NativeResult<DraftReference> {
        self.check_type_name(name)?;
        let id = self.definition(DefinitionKind::Trait, name)?;
        let mut record = TraitDef {
            name: name.into(),
            conversion_adapter: None,
            generic_params: (0..arity)
                .map(|position| GenericParam {
                    owner: id,
                    position,
                })
                .collect(),
            bounds: vec![],
            supertraits: vec![],
            methods: vec![],
            associated_types: vec![],
            associated_consts: vec![],
        };
        configure(&mut record, &self.definitions())?;
        if record.name != name || record.generic_params.len() != arity {
            return Err(conflict(
                "trait configuration changed its identity or arity",
            ));
        }
        self.check_scope(&record)?;
        self.state.declaration.traits.push(record);
        self.state
            .declaration
            .collect_definitions(self.context)
            .map_err(conflict)?;
        Ok(DraftReference {
            id,
            definitions: self.definitions(),
            kind: ReferenceKind::Trait,
            arity,
        })
    }

    /// Lower a trait method through the same MethodDecl lowering used by builders.
    pub fn trait_method(
        &self,
        owner: DefinitionId,
        declaration: MethodDecl,
    ) -> NativeResult<FnDecl> {
        self.check_local(owner)?;
        let receiver = Type::from_semantic(Ty::SelfType(owner), &self.definitions())?;
        declaration.lower(receiver, self.context)
    }

    pub fn export(&mut self, name: ExportName, target: DefinitionId) -> NativeResult<()> {
        self.definitions().resolve(target).map_err(conflict)?;
        if self.state.declaration.exports.contains_key(&name) {
            return Err(conflict("duplicate export name"));
        }
        self.state.declaration.exports.insert(name, target);
        Ok(())
    }

    pub fn export_variants(&mut self, reference: &DraftReference) -> NativeResult<()> {
        self.check_local(reference.id)?;
        if reference.kind != ReferenceKind::Type(TypeDefKind::Enum) {
            return Err(conflict("variant exports require an enum"));
        }
        let definitions = self.definitions();
        let name = definitions
            .resolve(reference.id)
            .map_err(conflict)?
            .last()
            .expect("enum identity")
            .name;
        if !self.state.declaration.variant_exports.insert(name.into()) {
            return Err(conflict("duplicate variant export"));
        }
        Ok(())
    }

    pub fn bind_storage(
        &mut self,
        reference: &DraftReference,
        storage: NativeStorage,
    ) -> NativeResult<()> {
        self.check_local(reference.id)?;
        if reference.kind != ReferenceKind::Type(TypeDefKind::NativeStorage(storage.layout()))
            || !storage.layout().valid_parameters(reference.arity)
            || self.state.storage.contains_key(&reference.id)
        {
            return Err(conflict("storage differs from its local declaration"));
        }
        self.state.storage.insert(reference.id, storage);
        Ok(())
    }

    /// Submit a complete free function under its final binder. Configuration
    /// failure publishes no signature, callable facts or pending binding.
    pub fn define_function(
        &mut self,
        definition: impl Into<FunctionDefinition>,
    ) -> NativeResult<FunctionRef> {
        let FunctionDefinition {
            mut declaration,
            concrete_result,
            configure,
            binding,
        } = definition.into();
        if self
            .state
            .declaration
            .functions
            .iter()
            .any(|item| item.name == declaration.name)
        {
            return Err(conflict("duplicate function declaration"));
        }
        let id = self.definition(DefinitionKind::Function, &declaration.name)?;
        let documentation = declaration.documentation.take();
        let mut signature = declaration.lower(CallableImplementation::Native(id), self.context)?;
        let mut concrete_results = BTreeMap::new();
        let mut requirements = vec![];
        let mut names = vec![];
        let mut function = FunctionBuilder {
            id,
            context: self.context,
            signature: &mut signature,
            concrete_results: &mut concrete_results,
            requirements: &mut requirements,
            names: &mut names,
        };
        if let Some(result) = concrete_result {
            function.produces(result)?;
        }
        if let Some(configure) = configure {
            configure(&mut function)?;
        }
        normalize_bounds(&mut signature, &self.definitions())?;
        self.check_scope(&signature)?;
        self.state.declaration.functions.push(signature);
        self.state
            .declaration
            .concrete_results
            .extend(concrete_results);
        if !requirements.is_empty() {
            self.state
                .declaration
                .callable_requirements
                .insert(id, requirements);
        }
        if let Some(text) = documentation {
            self.state.declaration.documentation.insert(id, text);
        }
        self.state.bindings.insert(id, binding);
        Ok(FunctionRef {
            id,
            definitions: self.definitions(),
        })
    }

    /// Collect an already lowered private default template. This advanced entry
    /// preserves its binder, requirements and concrete-result proof; it does not
    /// lower defaults or infer a signature from a Rust callback.
    pub fn define_template(
        &mut self,
        declaration: NativeDeclaration,
        binding: PendingBinding,
    ) -> NativeResult<()> {
        let id = self.definition(DefinitionKind::Function, &declaration.function.name)?;
        if declaration.declaration != id
            || declaration.function.implementation != CallableImplementation::Native(id)
            || self
                .state
                .declaration
                .functions
                .iter()
                .any(|item| item.name == declaration.function.name)
        {
            return Err(conflict("duplicate or foreign default template"));
        }
        self.check_scope(&declaration)?;
        if let Some(ty) = declaration.concrete_result {
            self.state.declaration.concrete_results.insert(id, ty);
        }
        self.state
            .declaration
            .callable_requirements
            .insert(id, declaration.callable_requirements);
        self.state.declaration.private_functions.insert(id);
        self.state.declaration.functions.push(declaration.function);
        self.state.bindings.insert(id, binding);
        Ok(())
    }

    /// Record direct overrides; omitted members keep existing trait defaults.
    /// Lowering waits for every trait contract, not recipe execution order.
    pub fn trait_implementation(
        &mut self,
        receiver: Type,
        interface: Type,
        bindings: Vec<(String, PendingBinding)>,
    ) -> NativeResult<()> {
        self.trait_implementation_with(0, |_| {
            Ok(TraitImplementation {
                receiver,
                interface,
                bounds: vec![],
                bindings,
            })
        })
    }

    /// Author a generic implementation under its actual implementation binder.
    /// Parameters are supplied once; no registration-order IDs or completed fake
    /// trait handles are needed to express forward associated outputs.
    pub fn trait_implementation_with<I: Into<TraitImplementationDefinition>>(
        &mut self,
        arity: usize,
        configure: impl FnOnce(&[Type]) -> NativeResult<I>,
    ) -> NativeResult<()> {
        let owner = self.next_implementation_id()?;
        let parameters = (0..arity)
            .map(|position| GenericParam { owner, position })
            .collect::<Vec<_>>();
        let arguments = parameters
            .iter()
            .map(|parameter| Type::from_semantic(parameter.as_type(), &self.definitions()))
            .collect::<NativeResult<Vec<_>>>()?;
        let TraitImplementationDefinition {
            implementation,
            overrides,
        } = configure(&arguments)?.into();
        let TraitImplementation {
            receiver,
            interface,
            bounds,
            bindings,
        } = implementation;
        for bound in &bounds {
            self.check_scope(bound)?;
        }
        let Ty::Trait(interface) = interface.adopt(self.context)? else {
            return Err(conflict("implementation requires a trait application"));
        };
        let receiver = receiver.adopt(self.context)?;
        let mut names = BTreeSet::new();
        let mut identities = vec![];
        let mut pending = vec![];
        for (name, binding) in bindings {
            if !names.insert(name.clone()) {
                return Err(conflict("duplicate trait override"));
            }
            let id = self
                .context
                .intern_child(owner, DefinitionKind::Method, &name, 0)
                .map_err(conflict)?;
            identities.push(OverrideIntent {
                name,
                id,
                facts: None,
            });
            pending.push((id, binding));
        }
        for entry in overrides {
            entry.member.check_scope(self.context)?;
            if entry.member.kind != DefinitionKind::Method
                || entry.member.owner.id != interface.declaration
            {
                return Err(conflict(
                    "override member belongs to another trait or category",
                ));
            }
            let name = entry.member.name.clone();
            if !names.insert(name.clone()) {
                return Err(conflict("duplicate trait override"));
            }
            let id = self
                .context
                .intern_child(owner, DefinitionKind::Method, &name, 0)
                .map_err(conflict)?;
            let facts = entry.lower(self.context)?;
            identities.push(OverrideIntent {
                name,
                id,
                facts: Some(facts.0),
            });
            pending.push((id, facts.1));
        }
        self.state
            .trait_intents
            .insert(self.state.declaration.implementations.len(), identities);
        self.state.declaration.implementations.push(ImplDecl {
            generic_params: parameters,
            bounds,
            trait_type: Some(interface),
            for_type: receiver,
            methods: vec![],
        });
        self.state.bindings.extend(pending);
        Ok(())
    }

    fn next_implementation_id(&self) -> NativeResult<DefinitionId> {
        let root = self
            .context
            .intern_root(self.identity())
            .map_err(conflict)?;
        let occurrence =
            u32::try_from(self.state.declaration.implementations.len()).map_err(conflict)?;
        self.context
            .intern_child(root, DefinitionKind::Impl, "", occurrence)
            .map_err(conflict)
    }

    fn check_local(&self, id: DefinitionId) -> NativeResult<()> {
        if self.definitions().resolve(id).map_err(conflict)?.module() != self.identity() {
            return Err(conflict("definition belongs to another module"));
        }
        Ok(())
    }

    fn check_scope(&self, record: &impl DefinitionReferences) -> NativeResult<()> {
        let definitions = self.definitions();
        record
            .visit_definition_ids(
                &mut |id| Ok(definitions.resolve(*id).map(|_| ())?),
                &CancellationToken::default(),
            )
            .map_err(conflict)
    }

    fn check_type_name(&self, name: &str) -> NativeResult<()> {
        if self
            .state
            .declaration
            .types
            .iter()
            .any(|item| item.name == name)
            || self
                .state
                .declaration
                .traits
                .iter()
                .any(|item| item.name == name)
        {
            return Err(conflict("duplicate type or trait declaration"));
        }
        Ok(())
    }
}

/// Semantic implementation inputs collected independently of adapter preparation.
pub struct TraitImplementation {
    pub receiver: Type,
    pub interface: Type,
    pub bounds: Vec<GenericBound>,
    pub bindings: Vec<(String, PendingBinding)>,
}

/// An inherent group commits only after its callback succeeds. Each complete
/// method is lowered transactionally, so a rejected method does not reserve a name.
pub struct MethodDraft {
    builder: InherentMethodsBuilder,
    bounds: Vec<GenericBound>,
    bindings: BTreeMap<DefinitionId, PendingBinding>,
}

impl MethodDraft {
    pub fn define(&mut self, definition: MethodDefinition) -> NativeResult<FunctionRef> {
        let mut candidate = InherentMethodsBuilder {
            context: self.builder.context.clone(),
            catalog: self.builder.catalog.clone(),
            owner: self.builder.owner,
            receiver: self.builder.receiver.clone(),
            receiver_codec: None,
            methods: BTreeMap::new(),
            documentation: BTreeMap::new(),
            method_parameters: BTreeMap::new(),
            requirements: BTreeMap::new(),
            concrete_results: BTreeMap::new(),
            bindings: BTreeMap::new(),
        };
        let reference = candidate.define_method(definition.declaration)?;
        if self.builder.methods.contains_key(&reference.id) {
            return Err(conflict("duplicate inherent method declaration"));
        }
        candidate.method(&reference, |function| {
            if let Some(ty) = definition.concrete_result {
                function.produces(ty)?;
            }
            if let Some(configure) = definition.configure {
                configure(function)?;
            }
            Ok(())
        })?;
        self.builder.methods.extend(candidate.methods);
        self.builder.documentation.extend(candidate.documentation);
        self.builder
            .method_parameters
            .extend(candidate.method_parameters);
        self.builder.requirements.extend(candidate.requirements);
        self.builder
            .concrete_results
            .extend(candidate.concrete_results);
        self.bindings.insert(reference.id, definition.binding);
        Ok(reference)
    }
}
