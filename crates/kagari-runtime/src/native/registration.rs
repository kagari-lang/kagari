//! Documented concrete signatures lower into the existing declaration model.
use crate::{
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{IntoKagari, arguments::FromKagariArguments},
        declarations::FunctionDecl,
        typed::NativeContext,
        types::{FunctionRef, Type},
    },
};
use kagari_common::identity::DefinitionKind;
use kagari_types::{
    callable::{CallableImplementation, Signature},
    ty::Ty,
};
use std::collections::{BTreeMap, BTreeSet};

/// Parameter names and Markdown are explicit; concrete types come from Rust.
/// An optional interface return keeps its exported contract while the compiler
/// prepares the adapter for the callback's concrete result.
#[derive(Debug, Clone)]
pub struct FunctionSpec {
    name: String,
    parameters: Vec<String>,
    documentation: String,
    parameter_docs: BTreeMap<String, String>,
    return_docs: Option<String>,
    result: Option<Type>,
}

impl FunctionSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            parameters: Vec::new(),
            documentation: String::new(),
            parameter_docs: BTreeMap::new(),
            return_docs: None,
            result: None,
        }
    }

    pub fn parameter_names(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.parameters = names.into_iter().map(Into::into).collect();
        self
    }

    pub fn documentation(mut self, text: impl Into<String>) -> Self {
        self.documentation = text.into();
        self
    }

    pub fn parameter_documentation(
        mut self,
        name: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        self.parameter_docs.insert(name.into(), text.into());
        self
    }

    pub fn return_documentation(mut self, text: impl Into<String>) -> Self {
        self.return_docs = Some(text.into());
        self
    }

    pub fn returns(mut self, result: Type) -> Self {
        self.result = Some(result);
        self
    }

    fn declaration(self, parameters: Vec<Type>, result: Type) -> NativeResult<FunctionDecl> {
        let names: BTreeSet<_> = self.parameters.iter().collect();
        if parameters.len() != self.parameters.len()
            || names.len() != parameters.len()
            || self.parameter_docs.keys().any(|name| !names.contains(name))
        {
            return Err(RuntimeError::metadata_conflict(
                "typed function parameter names or documentation",
            ));
        }
        let mut documentation = self.documentation;
        if !self.parameter_docs.is_empty() {
            documentation.push_str("\n\n# Parameters\n");
            for name in &self.parameters {
                if let Some(text) = self.parameter_docs.get(name) {
                    documentation.push_str(&format!("\n## `{name}`\n\n{text}\n"));
                }
            }
        }
        if let Some(text) = self.return_docs {
            documentation.push_str(&format!("\n\n# Returns\n\n{text}"));
        }
        let mut declaration = FunctionDecl::new(self.name)
            .documentation(documentation)
            .returns(self.result.unwrap_or(result));
        for (name, ty) in self.parameters.into_iter().zip(parameters) {
            declaration = declaration.parameter(name, ty);
        }
        Ok(declaration)
    }
}

impl ModuleBuilder {
    /// Prepare a concrete declaration and binding before publishing either.
    /// Explicit generic declarations use `define_function` and `bind_with`,
    /// sharing the same catalog and binding validator.
    pub fn add_function<A, R>(
        &mut self,
        spec: FunctionSpec,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> NativeResult<FunctionRef>
    where
        A: FromKagariArguments,
        R: IntoKagari,
    {
        let mut catalog = self.providers.clone();
        catalog.merge(&DeclarationCatalog::declared([&self.declaration])?)?;
        let parameters = A::argument_types(&catalog)?;
        let result = R::kagari_type(&catalog)?;
        let declaration = spec.declaration(parameters.clone(), result.clone())?;
        let produces = (declaration.result != result).then_some(result.clone());
        if produces.is_some() && !matches!(declaration.result.abi(), Ty::Trait(_)) {
            return Err(RuntimeError::metadata_conflict(
                "typed function return differs from its declaration",
            ));
        }
        let binding = NativeBinding::typed(&catalog, entry)?;
        binding.check(
            &Signature {
                params: parameters.into_iter().map(|ty| ty.0).collect(),
                result: result.0,
            },
            &catalog,
        )?;
        let mut candidate = self.declaration.clone();
        let id = candidate.definition(DefinitionKind::Function, &declaration.name);
        candidate.functions.push(
            declaration
                .clone()
                .lower(CallableImplementation::Native(id.clone())),
        );
        if let Some(produces) = &produces {
            candidate.concrete_results.insert(id, produces.0.clone());
        }
        let owners = catalog.receiver_owners(Some(&candidate))?;
        candidate
            .validate(&|receiver| owners.owner(receiver))
            .map_err(|cause| RuntimeError::metadata_conflict(cause.to_string()))?;
        let function = self.define_function(declaration)?;
        if let Some(produces) = produces {
            self.declaration
                .concrete_results
                .insert(function.id.clone(), produces.0);
        }
        self.bindings.insert(function.id.clone(), binding);
        Ok(function)
    }
}
