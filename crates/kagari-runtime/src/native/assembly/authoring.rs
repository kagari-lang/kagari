//! Complete authored methods keep Rust implementation ownership outside metadata.
use crate::native::{
    adapter::NativeAdapter,
    binding::{NativeBinding, NativeResult},
    catalog::DeclarationCatalog,
    declarations::{FunctionBuilder, MethodDecl},
    types::Type,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_types::{
    declaration::{FnDecl, NativeDeclaration, host::HostCallPolicy},
    reference::ReferenceAccess,
};

type Prepare = Box<dyn FnOnce(&FnDecl, &SealedPreparation<'_>) -> NativeResult<NativeBinding>>;
pub(crate) type SendPrepare =
    Box<dyn FnOnce(&FnDecl, &SealedPreparation<'_>) -> NativeResult<NativeBinding> + Send>;
type Configure = Box<dyn FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<()>>;

/// Read-only preparation facts for this exact sealed binding target.
///
/// The scoped declaration includes lowered generic parameters, ordered callable
/// requirements and the concrete result. Factories can pass `declaration()` to
/// `NativeBinding::declared` and `SelectedCall::from_declaration` without discovering
/// generated names. The catalog contains the complete sealed dependency graph.
/// These facts do not authorize a different declaration's requirement tokens.
///
/// For trait defaults, this is the private template, while `NativeAdapter::prepare`
/// still receives the original authored method signature. A binding without an
/// explicit requirement owner uses authored-method tokens. Pair template tokens
/// with `NativeBinding::declared(declaration(), callback)` so their provenance is
/// retained. Defaults reject explicit owners other than their authored method or
/// sealed template, comparing complete scoped identities.
///
/// Factories run only when `ModuleAssembly::into_modules` consumes the assembly.
pub struct SealedPreparation<'a> {
    declaration: &'a DefinitionMetadata<NativeDeclaration>,
    catalog: &'a DeclarationCatalog,
}

impl<'a> SealedPreparation<'a> {
    pub(crate) fn new(
        declaration: &'a DefinitionMetadata<NativeDeclaration>,
        catalog: &'a DeclarationCatalog,
    ) -> Self {
        Self {
            declaration,
            catalog,
        }
    }

    pub fn declaration(&self) -> &DefinitionMetadata<NativeDeclaration> {
        self.declaration
    }

    pub fn catalog(&self) -> &DeclarationCatalog {
        self.catalog
    }
}

/// Deferred adapter selection. Raw bindings and primitives retain their entry form.
/// Preparation is owned and sequential, not a shared cache: adapters and factories
/// may retain thread-local authoring state. The resulting NativeBinding retains
/// its own Send/Sync callback requirements; this wrapper does not weaken them.
pub struct PendingBinding(PendingPreparation);

enum PendingPreparation {
    Local(Prepare),
    Transferable(SendPrepare),
}

impl PendingBinding {
    pub fn implementation<M, R>(adapter: impl NativeAdapter<M, R> + 'static) -> Self {
        Self(PendingPreparation::Local(Box::new(
            move |declaration, preparation| adapter.prepare(declaration, preparation.catalog()),
        )))
    }

    pub fn implementation_with<M, R, A>(
        factory: impl FnOnce(&SealedPreparation<'_>) -> NativeResult<A> + 'static,
    ) -> Self
    where
        A: NativeAdapter<M, R>,
    {
        Self(PendingPreparation::Local(Box::new(
            move |declaration, preparation| {
                factory(preparation)?.prepare(declaration, preparation.catalog())
            },
        )))
    }

    /// Retain a transferable adapter; preparation still occurs only in into_modules.
    /// Only the stored adapter must be Send, not its marker or result types.
    pub fn implementation_send<M, R>(adapter: impl NativeAdapter<M, R> + Send + 'static) -> Self {
        Self(PendingPreparation::Transferable(Box::new(
            move |declaration, preparation| adapter.prepare(declaration, preparation.catalog()),
        )))
    }

    /// Retain a transferable factory. Its returned adapter may be thread-local:
    /// it is created and immediately prepared on the consuming thread.
    pub fn implementation_with_send<M, R, A>(
        factory: impl FnOnce(&SealedPreparation<'_>) -> NativeResult<A> + Send + 'static,
    ) -> Self
    where
        A: NativeAdapter<M, R>,
    {
        Self(PendingPreparation::Transferable(Box::new(
            move |declaration, preparation| {
                factory(preparation)?.prepare(declaration, preparation.catalog())
            },
        )))
    }

    /// Preserve transferability while preparing against an authored signature and
    /// applying the same sealed-contract checks to either closure category.
    pub(crate) fn with_signature(
        self,
        signature: FnDecl,
        check: impl FnOnce(NativeBinding, &SealedPreparation<'_>) -> NativeResult<NativeBinding>
        + Send
        + 'static,
    ) -> Self {
        match self.0 {
            PendingPreparation::Local(prepare) => Self(PendingPreparation::Local(Box::new(
                move |_, preparation| check(prepare(&signature, preparation)?, preparation),
            ))),
            PendingPreparation::Transferable(prepare) => {
                Self(PendingPreparation::Transferable(Box::new(
                    move |_, preparation| check(prepare(&signature, preparation)?, preparation),
                )))
            }
        }
    }

    pub(crate) fn into_transferable(self) -> Option<SendPrepare> {
        match self.0 {
            PendingPreparation::Local(_) => None,
            PendingPreparation::Transferable(prepare) => Some(prepare),
        }
    }

    pub(crate) fn from_transferable(prepare: SendPrepare) -> Self {
        Self(PendingPreparation::Transferable(prepare))
    }

    pub(crate) fn prepare(
        self,
        declaration: &FnDecl,
        preparation: &SealedPreparation<'_>,
    ) -> NativeResult<NativeBinding> {
        match self.0 {
            PendingPreparation::Local(prepare) => prepare(declaration, preparation),
            PendingPreparation::Transferable(prepare) => prepare(declaration, preparation),
        }
    }
}

/// Semantic signature/facts plus exactly one direct implementation association.
/// Created by MethodDecl or MethodAuthoring's terminal implementation methods.
/// Native factories remain separate from portable signature and proof metadata.
pub struct MethodDefinition {
    pub(crate) declaration: MethodDecl,
    pub(crate) concrete_result: Option<Type>,
    pub(crate) configure: Option<Configure>,
    pub(crate) binding: PendingBinding,
}

/// An authoring wrapper, not a portable declaration record.
/// Created by MethodDecl::produces or MethodDecl::configure. Signature setters
/// remain available until implementation selection completes the definition.
pub struct MethodAuthoring {
    declaration: MethodDecl,
    concrete_result: Option<Type>,
    configure: Option<Configure>,
}

impl MethodAuthoring {
    pub(crate) fn new(declaration: MethodDecl) -> Self {
        Self {
            declaration,
            concrete_result: None,
            configure: None,
        }
    }

    pub fn documentation(mut self, text: impl Into<String>) -> Self {
        self.declaration = self.declaration.documentation(text);
        self
    }

    pub fn parameter(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.declaration = self.declaration.parameter(name, ty);
        self
    }

    pub fn returns(mut self, ty: Type) -> Self {
        self.declaration = self.declaration.returns(ty);
        self
    }

    pub fn host_policy(mut self, policy: HostCallPolicy) -> Self {
        self.declaration = self.declaration.host_policy(policy);
        self
    }

    pub fn receiver_access(mut self, access: ReferenceAccess) -> Self {
        self.declaration = self.declaration.receiver_access(access);
        self
    }

    /// State the concrete result behind an interface; sealing checks the proof.
    pub fn produces(mut self, ty: Type) -> Self {
        self.concrete_result = Some(ty);
        self
    }

    /// Configure ordinary generic parameters, bounds and callable requirements
    /// through the existing semantic builder, before any adapter is prepared.
    /// Callbacks run once in insertion order after all fluent signature setters
    /// and `produces`, regardless of their position in the chain. The first error
    /// stops configuration and rejects the complete method submission.
    pub fn configure(
        mut self,
        configure: impl FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<()> + 'static,
    ) -> Self {
        let previous = self.configure.take();
        self.configure = Some(Box::new(move |function| {
            if let Some(previous) = previous {
                previous(function)?;
            }
            configure(function)
        }));
        self
    }

    /// Attach an adapter without preparing it or imposing additional Send bounds.
    pub fn implementation<M, R>(
        self,
        adapter: impl NativeAdapter<M, R> + 'static,
    ) -> MethodDefinition {
        self.complete(PendingBinding::implementation(adapter))
    }

    /// Retain a one-shot factory until its scoped target and catalog are sealed.
    pub fn implementation_with<M, R, A>(
        self,
        factory: impl FnOnce(&SealedPreparation<'_>) -> NativeResult<A> + 'static,
    ) -> MethodDefinition
    where
        A: NativeAdapter<M, R>,
    {
        self.complete(PendingBinding::implementation_with(factory))
    }

    /// Attach a transferable adapter without constraining authoring/configuration state.
    pub fn implementation_send<M, R>(
        self,
        adapter: impl NativeAdapter<M, R> + Send + 'static,
    ) -> MethodDefinition {
        self.complete(PendingBinding::implementation_send(adapter))
    }

    /// Attach a Send factory; the adapter it creates need not be Send.
    pub fn implementation_with_send<M, R, A>(
        self,
        factory: impl FnOnce(&SealedPreparation<'_>) -> NativeResult<A> + Send + 'static,
    ) -> MethodDefinition
    where
        A: NativeAdapter<M, R>,
    {
        self.complete(PendingBinding::implementation_with_send(factory))
    }

    fn complete(self, binding: PendingBinding) -> MethodDefinition {
        MethodDefinition {
            declaration: self.declaration,
            concrete_result: self.concrete_result,
            configure: self.configure,
            binding,
        }
    }
}
