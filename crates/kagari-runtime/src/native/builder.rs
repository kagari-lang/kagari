//! Explicit Kagari declarations plus checked local Rust bindings.
pub mod enum_builder;
pub mod implementation;
pub mod inherent;
pub mod trait_builder;
pub mod type_builder;
use crate::{
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        builder::{
            enum_builder::EnumBuilder, implementation::ImplementationBuilder,
            trait_builder::TraitBuilder, type_builder::TypeBuilder,
        },
        catalog::{DeclarationCatalog, import::CatalogImports},
        conversion::{IntoKagari, arguments::FromKagariArguments},
        declarations::{FunctionBuilder, FunctionDecl, normalize_bounds},
        functions::NativeFunction,
        module::NativeModule,
        storage::NativeStorage,
        typed::NativeContext,
        types::{FunctionRef, Receiver},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, PackageId,
        map::DefinitionContext,
        mapping::DefinitionMapper,
        metadata::DefinitionMetadata,
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::module::ModuleDecl,
};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone)]
pub struct ModuleBuilder {
    pub(crate) declaration: ModuleDecl,
    pub(crate) providers: DeclarationCatalog,
    pub(crate) bindings: BTreeMap<DefinitionId, NativeBinding>,
    pub(crate) storage: BTreeMap<DefinitionId, NativeStorage>,
    function_parameters: BTreeMap<DefinitionId, Vec<String>>,
}

impl ModuleBuilder {
    /// Adopt declaration records and binding keys from the supplied source scope.
    pub fn from_declaration(
        declaration: DefinitionMetadata<ModuleDecl>,
        providers: &DeclarationCatalog,
        bindings: BTreeMap<DefinitionId, NativeBinding>,
    ) -> NativeResult<Self> {
        let providers = construction_catalog(providers)?;
        let context = providers.types.context();
        let mut references = bindings.keys().copied().collect::<HashSet<_>>();
        let cancel = CancellationToken::default();
        declaration
            .records()
            .visit_definition_ids(
                &mut |id| {
                    references.insert(*id);
                    Ok(())
                },
                &cancel,
            )
            .map_err(conflict)?;
        let remap = context
            .import(declaration.definitions(), references)
            .map_err(conflict)?;
        let declaration = declaration
            .records()
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| remap.map(*id).map_err(Into::into),
                &cancel,
            ))
            .map_err(conflict)?;
        declaration.collect_definitions(context).map_err(conflict)?;
        let bindings = bindings
            .into_iter()
            .map(|(id, binding)| Ok((remap.map(id).map_err(conflict)?, binding)))
            .collect::<NativeResult<_>>()?;
        Ok(Self {
            declaration,
            providers,
            bindings,
            storage: BTreeMap::new(),
            function_parameters: BTreeMap::new(),
        })
    }

    pub fn new(identity: &str, providers: &DeclarationCatalog) -> NativeResult<Self> {
        let mut path = identity.split("::");
        let identity = ModuleIdentity {
            package: PackageId(path.next().unwrap_or_default().into()),
            path: path.map(str::to_owned).collect(),
        };
        let providers = construction_catalog(providers)?;
        let declaration = ModuleDecl::new(identity);
        declaration
            .collect_definitions(providers.types.context())
            .map_err(conflict)?;
        Ok(Self {
            declaration,
            providers,
            bindings: BTreeMap::new(),
            storage: BTreeMap::new(),
            function_parameters: BTreeMap::new(),
        })
    }

    pub(crate) fn context(&self) -> &DefinitionContext {
        self.providers.types.context()
    }

    pub(crate) fn definition(
        &self,
        kind: DefinitionKind,
        name: &str,
    ) -> NativeResult<DefinitionId> {
        let root = self
            .providers
            .definitions()
            .lookup_root(&self.declaration.identity)
            .expect("construction module root");
        self.context()
            .intern_child(root, kind, name, 0)
            .map_err(conflict)
    }

    pub(crate) fn next_implementation_id(&self) -> NativeResult<DefinitionId> {
        let root = self
            .providers
            .definitions()
            .lookup_root(&self.declaration.identity)
            .expect("construction module root");
        let occurrence = u32::try_from(self.declaration.implementations.len())
            .map_err(|_| conflict("too many native implementations"))?;
        self.context()
            .intern_child(root, DefinitionKind::Impl, "", occurrence)
            .map_err(conflict)
    }

    /// Set the complete Markdown overview emitted as module documentation.
    pub fn documentation(&mut self, text: impl Into<String>) {
        self.declaration.module_documentation = text.into();
    }

    /// Provider declarations plus local types/traits that have finished validation.
    /// Use this catalog when preparing an explicit typed binding.
    pub fn declarations(&self) -> &DeclarationCatalog {
        &self.providers
    }

    pub fn with_modules(mut self, modules: &[&NativeModule]) -> NativeResult<Self> {
        self.providers
            .merge(&DeclarationCatalog::from_modules(modules)?)?;
        Ok(self)
    }

    pub fn define_function(&mut self, mut declaration: FunctionDecl) -> NativeResult<FunctionRef> {
        if self
            .declaration
            .functions
            .iter()
            .any(|function| function.name == declaration.name)
        {
            return Err(RuntimeError::metadata_conflict(
                "duplicate function declaration",
            ));
        }
        let id = self.definition(DefinitionKind::Function, &declaration.name)?;
        if let Some(documentation) = declaration.documentation.take() {
            self.declaration.documentation.insert(id, documentation);
        }
        let function = declaration.lower(CallableImplementation::Native(id), self.context())?;
        self.declaration.functions.push(function);
        Ok(FunctionRef {
            id,
            definitions: self.providers.definitions(),
        })
    }

    pub fn function<T>(
        &mut self,
        function: &FunctionRef,
        configure: impl FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<T>,
    ) -> NativeResult<T> {
        let definitions = self.providers.definitions();
        let view = definitions.resolve(function.id).map_err(conflict)?;
        if view.module() != &self.declaration.identity {
            return Err(RuntimeError::metadata_conflict(
                "function belongs to another module",
            ));
        }
        let signature = self
            .declaration
            .functions
            .iter_mut()
            .find(|signature| signature.name == view.last().expect("function identity").name)
            .ok_or_else(|| RuntimeError::metadata_conflict("unknown function declaration"))?;
        let result = configure(&mut FunctionBuilder {
            id: function.id,
            context: self.providers.types.context(),
            concrete_results: &mut self.declaration.concrete_results,
            signature,
            requirements: self
                .declaration
                .callable_requirements
                .entry(function.id)
                .or_default(),
            names: self.function_parameters.entry(function.id).or_default(),
        })?;
        normalize_bounds(signature, &self.providers.definitions())?;
        Ok(result)
    }

    pub fn bind<A, R>(
        &mut self,
        function: FunctionRef,
        entry: impl NativeFunction<A, R>,
    ) -> NativeResult<()> {
        self.bind_with(function, entry.binding())
    }

    pub fn bind_with(&mut self, function: FunctionRef, binding: NativeBinding) -> NativeResult<()> {
        let declaration = self
            .declaration
            .native_declarations(&self.providers.definitions())
            .map_err(conflict)?
            .into_iter()
            .find(|declaration| declaration.declaration == function.id)
            .ok_or_else(|| {
                RuntimeError::metadata_conflict("unknown native function declaration")
            })?;
        let signature = Signature {
            params: declaration
                .function
                .params
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect(),
            result: declaration
                .concrete_result
                .clone()
                .unwrap_or_else(|| declaration.function.return_type.clone()),
        };
        binding.check(&signature, &self.providers.definitions(), &self.providers)?;
        if self.bindings.contains_key(&function.id) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate native function binding",
            ));
        }
        self.bindings.insert(function.id, binding);
        Ok(())
    }

    /// Bind against the authored signature, including generic parameters and
    /// contextual handles. Rust mappings are checked against each concrete
    /// application before any argument converter or callback runs.
    pub fn bind_typed<A: FromKagariArguments, R: IntoKagari>(
        &mut self,
        function: FunctionRef,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> NativeResult<()> {
        let declaration = self
            .declaration
            .native_declarations(&self.providers.definitions())
            .map_err(conflict)?
            .into_iter()
            .find(|declaration| declaration.declaration == function.id)
            .ok_or_else(|| {
                RuntimeError::metadata_conflict("unknown native function declaration")
            })?;
        self.bind_with(
            function,
            NativeBinding::contextual(declaration.function.params.len(), entry),
        )
    }

    pub fn finish(self) -> NativeResult<NativeModule> {
        NativeModule::checked(
            self.declaration,
            self.bindings.into_iter().collect(),
            self.storage,
            &self.providers,
        )
    }

    pub fn define_trait(&mut self, name: impl Into<String>) -> NativeResult<TraitBuilder<'_>> {
        TraitBuilder::new(self, name.into())
    }

    pub fn define_type(&mut self, name: impl Into<String>) -> NativeResult<TypeBuilder<'_>> {
        TypeBuilder::new(self, name.into())
    }

    /// Declare an ordinary managed enum; no opaque Rust storage is required.
    pub fn define_enum(&mut self, name: impl Into<String>) -> NativeResult<EnumBuilder<'_>> {
        EnumBuilder::new(self, name.into())
    }

    pub fn implement<T>(
        &mut self,
        receiver: impl Into<Receiver>,
        configure: impl FnOnce(&mut ImplementationBuilder<'_>) -> NativeResult<T>,
    ) -> NativeResult<T> {
        let mut builder = ImplementationBuilder::new(self, receiver.into())?;
        configure(&mut builder)
    }
}

fn construction_catalog(providers: &DeclarationCatalog) -> NativeResult<DeclarationCatalog> {
    let context = DefinitionContext::new().map_err(conflict)?;
    CatalogImports::new(context).import(providers)
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
