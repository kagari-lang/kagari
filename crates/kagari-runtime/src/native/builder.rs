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
        catalog::DeclarationCatalog,
        conversion::{IntoKagari, arguments::FromKagariArguments},
        declarations::{FunctionBuilder, FunctionDecl, normalize_bounds},
        functions::NativeFunction,
        module::NativeModule,
        storage::NativeStorage,
        typed::NativeContext,
        types::{FunctionRef, Receiver},
    },
};
use kagari_common::identity::{DefinitionKind, DefinitionPath, ModuleIdentity, PackageId};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::module::ModuleDecl,
};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct ModuleBuilder {
    pub(crate) declaration: ModuleDecl,
    pub(crate) providers: DeclarationCatalog,
    pub(crate) bindings: BTreeMap<DefinitionPath, NativeBinding>,
    pub(crate) storage: BTreeMap<DefinitionPath, NativeStorage>,
    function_parameters: BTreeMap<DefinitionPath, Vec<String>>,
}

impl ModuleBuilder {
    /// Build an explicit declaration set with its Rust entries and providers.
    pub fn from_declaration(
        declaration: ModuleDecl,
        providers: &DeclarationCatalog,
        bindings: BTreeMap<DefinitionPath, NativeBinding>,
    ) -> Self {
        Self {
            declaration,
            providers: providers.clone(),
            bindings,
            storage: BTreeMap::new(),
            function_parameters: BTreeMap::new(),
        }
    }

    pub fn new(identity: &str, providers: &DeclarationCatalog) -> Self {
        let mut path = identity.split("::");
        let identity = ModuleIdentity {
            package: PackageId(path.next().unwrap_or_default().into()),
            path: path.map(str::to_owned).collect(),
        };
        Self::from_declaration(ModuleDecl::new(identity), providers, BTreeMap::new())
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
        let id = self
            .declaration
            .definition(DefinitionKind::Function, &declaration.name);
        if let Some(documentation) = declaration.documentation.take() {
            self.declaration
                .documentation
                .insert(id.clone(), documentation);
        }
        let function = declaration.lower(CallableImplementation::Native(id.clone()));
        self.declaration.functions.push(function);
        Ok(FunctionRef { id })
    }

    pub fn function<T>(
        &mut self,
        function: &FunctionRef,
        configure: impl FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<T>,
    ) -> NativeResult<T> {
        if function.id.module != self.declaration.identity {
            return Err(RuntimeError::metadata_conflict(
                "function belongs to another module",
            ));
        }
        let signature = self
            .declaration
            .functions
            .iter_mut()
            .find(|signature| signature.name == function.id.path[0].name)
            .ok_or_else(|| RuntimeError::metadata_conflict("unknown function declaration"))?;
        let result = configure(&mut FunctionBuilder {
            id: function.id.clone(),
            concrete_results: &mut self.declaration.concrete_results,
            signature,
            requirements: self
                .declaration
                .callable_requirements
                .entry(function.id.clone())
                .or_default(),
            names: self
                .function_parameters
                .entry(function.id.clone())
                .or_default(),
        })?;
        normalize_bounds(signature);
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
            .native_declarations()
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
        binding.check(&signature, &self.providers)?;
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
            .native_declarations()
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

    pub fn define_trait(&mut self, name: impl Into<String>) -> TraitBuilder<'_> {
        TraitBuilder::new(self, name.into())
    }

    pub fn define_type(&mut self, name: impl Into<String>) -> TypeBuilder<'_> {
        TypeBuilder::new(self, name.into())
    }

    /// Declare an ordinary managed enum; no opaque Rust storage is required.
    pub fn define_enum(&mut self, name: impl Into<String>) -> EnumBuilder<'_> {
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
