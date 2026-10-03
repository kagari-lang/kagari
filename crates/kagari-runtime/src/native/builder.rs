//! Explicit Kagari declarations plus checked local Rust bindings.
pub mod implementation;
pub mod inherent;
pub mod trait_builder;
pub mod type_builder;
use crate::{
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        builder::{
            implementation::ImplementationBuilder, trait_builder::TraitBuilder,
            type_builder::TypeBuilder,
        },
        catalog::DeclarationCatalog,
        declarations::{FunctionBuilder, FunctionDecl, normalize_bounds},
        functions::NativeFunction,
        language::LanguageContracts,
        module::NativeModule,
        storage::NativeStorage,
        types::{FunctionRef, Receiver},
    },
};
use kagari_abi::{
    callable::CallableImplementation, declaration::ModuleDecl, native_import::NativeSignature,
};
use kagari_common::identity::{DefinitionId, DefinitionKind, ModuleIdentity, PackageId};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct ModuleBuilder {
    pub(crate) declaration: ModuleDecl,
    pub(crate) providers: DeclarationCatalog,
    pub(crate) bindings: BTreeMap<DefinitionId, NativeBinding>,
    pub(crate) storage: BTreeMap<DefinitionId, NativeStorage>,
    function_parameters: BTreeMap<DefinitionId, Vec<String>>,
}

impl ModuleBuilder {
    pub fn new(identity: &str, language: &LanguageContracts) -> Self {
        let mut path = identity.split("::");
        let package = PackageId(path.next().unwrap_or_default().into());
        let identity = ModuleIdentity {
            package,
            path: path.map(str::to_owned).collect(),
        };
        Self {
            declaration: ModuleDecl::new(identity),
            providers: language
                .catalog()
                .expect("compiler-owned language contracts"),
            bindings: BTreeMap::new(),
            storage: BTreeMap::new(),
            function_parameters: BTreeMap::new(),
        }
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
        let signature = NativeSignature {
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

    pub fn implement<T>(
        &mut self,
        receiver: impl Into<Receiver>,
        configure: impl FnOnce(&mut ImplementationBuilder<'_>) -> NativeResult<T>,
    ) -> NativeResult<T> {
        let mut builder = ImplementationBuilder::new(self, receiver.into())?;
        configure(&mut builder)
    }
}
