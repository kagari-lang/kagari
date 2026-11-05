//! Admission of fixed environments and body inputs from a checked dependency closure.

use crate::{
    CheckedAnalysis,
    compile_input::{
        body::{BodyKey, BodyRoot, CheckedBodyInput},
        declarations::{
            DeclarationHeaders, ImplementationInput, NativeDefaultCall, RegisteredDeclarations,
        },
        environment::CheckedEnvironment,
    },
    hir::{
        ids::{BodyOwner, FunctionId, HirOwner, ImplId},
        item::Item,
    },
    native::NativeBinding,
    program::{CheckedProgram, exports::ProgramExports},
    resolver::resolved::ResolvedName,
    semantic::signatures::FunctionImplementation,
    source_map::SourceMap,
    typeck::table::CallTarget,
};
use kagari_common::{
    cancellation::CancellationToken, identity::table::module::ModuleHandle, span::Span,
};
use kagari_source::source::SourceFile;
use std::collections::HashMap;

/// Failure to admit a checked source closure for elaboration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    /// The caller interrupted admission; no compilation input is returned.
    Cancelled,
    /// The checked envelope lacks a required module/signature invariant.
    InvalidModule(&'static str),
    /// A source body lacks matching ownership or positive completion evidence.
    InvalidBody { key: BodyKey, reason: &'static str },
}

/// A module's fixed declaration environment and admitted body directory.
///
/// Function signatures explicitly distinguish script, native and required methods.
/// An absent body is never interpreted as native by lookup. Generated native default
/// forwarding bodies can exist alongside a native implementation contract.
pub struct CheckedModule<'a> {
    handle: ModuleHandle,
    source: &'a SourceFile,
    origins: &'a SourceMap,
    headers: DeclarationHeaders<'a>,
    registration: Option<RegisteredDeclarations<'a>>,
    native_defaults: HashMap<FunctionId, NativeDefaultCall<'a>>,
    implementations: HashMap<ImplId, ImplementationInput<'a>>,
    environment: CheckedEnvironment<'a>,
    bodies: HashMap<BodyOwner, CheckedBodyInput<'a>>,
}

impl<'a> CheckedModule<'a> {
    fn admit(
        module: &'a CheckedAnalysis,
        cancel: &CancellationToken,
    ) -> Result<Self, AdmissionError> {
        if module.lowered.source_map.arena() != module.lowered.module.body.arena() {
            return Err(AdmissionError::InvalidModule(
                "source map belongs to another lowering",
            ));
        }
        let handle = module
            .definitions()
            .lookup_module(module.lowered.source.module_identity())
            .ok_or(AdmissionError::InvalidModule(
                "module identity missing from checked scope",
            ))?;
        let mut result = Self {
            handle,
            source: &module.lowered.source,
            origins: &module.lowered.source_map,
            headers: DeclarationHeaders::new(&module.lowered.module),
            registration: if module.lowered.is_registered_native_api() {
                Some(RegisteredDeclarations {
                    definitions: module.lowered.registered_native_definitions().ok_or(
                        AdmissionError::InvalidModule("registered module has no definition scope"),
                    )?,
                    callables: module.lowered.registered_native_declarations(),
                    source_traits: module.lowered.registered_source_trait_references(),
                })
            } else {
                None
            },
            native_defaults: HashMap::new(),
            implementations: HashMap::new(),
            environment: CheckedEnvironment::new(module),
            bodies: HashMap::new(),
        };
        result.admit_implementations(module, cancel)?;
        result.admit_functions(module, cancel)?;
        result.admit_initializers(module, cancel)?;
        cancel.check().map_err(|_| AdmissionError::Cancelled)?;
        Ok(result)
    }

    fn admit_implementations(
        &mut self,
        module: &'a CheckedAnalysis,
        cancel: &CancellationToken,
    ) -> Result<(), AdmissionError> {
        for implementation in &module.lowered.module.impls {
            cancel.check().map_err(|_| AdmissionError::Cancelled)?;
            let input = if implementation.trait_ref.is_some() {
                let declaration = module.declarations.impl_identity(implementation.id).ok_or(
                    AdmissionError::InvalidModule("implementation has no declaration identity"),
                )?;
                let signature = module
                    .aggregates
                    .implementation_signature(declaration)
                    .ok_or(AdmissionError::InvalidModule(
                        "implementation has no checked signature",
                    ))?;
                ImplementationInput::trait_implementation(signature)
            } else {
                ImplementationInput::inherent(
                    implementation,
                    &module.declarations,
                    module.typed.type_table(),
                )?
            };
            self.implementations.insert(implementation.id, input);
        }
        Ok(())
    }

    fn admit_functions(
        &mut self,
        module: &'a CheckedAnalysis,
        cancel: &CancellationToken,
    ) -> Result<(), AdmissionError> {
        for function in &module.lowered.module.functions {
            cancel.check().map_err(|_| AdmissionError::Cancelled)?;
            let signature = module.typed.signatures().function(function.id).ok_or(
                AdmissionError::InvalidModule("function signature missing from checked module"),
            )?;
            if signature.return_type.is_unresolved()
                || signature
                    .params
                    .iter()
                    .any(|param| param.ty.is_unresolved())
            {
                return Err(AdmissionError::InvalidModule(
                    "unresolved callable signature",
                ));
            }
            let owner = BodyOwner::Function(function.id);
            if signature.params.len() != function.params.len()
                || signature
                    .params
                    .iter()
                    .zip(&function.params)
                    .any(|(typed, source)| {
                        typed.id != source.id
                            || typed.id.arena() != module.lowered.module.body.arena()
                            || typed.id.owner() != HirOwner::Body(owner)
                    })
            {
                return Err(AdmissionError::InvalidModule(
                    "parameter signature belongs to another body",
                ));
            }
            if let Some(root) = function.body {
                if !module.typed.valid_bodies.contains(&function.id) {
                    return Err(AdmissionError::InvalidBody {
                        key: BodyKey {
                            module: self.handle,
                            arena: module.lowered.module.body.arena(),
                            owner,
                            closure: None,
                        },
                        reason: "function has no positive completion evidence",
                    });
                }
                self.bodies.insert(
                    owner,
                    CheckedBodyInput::new(module, self.handle, owner, BodyRoot::Block(root))?,
                );
            } else if matches!(signature.implementation, FunctionImplementation::Script) {
                return Err(AdmissionError::InvalidModule(
                    "script function has no source body",
                ));
            }
            if matches!(
                signature.implementation,
                FunctionImplementation::Native(NativeBinding::Default(_))
            ) {
                if function.body.is_none() {
                    return Err(AdmissionError::InvalidModule(
                        "native default has no checked forwarding body",
                    ));
                }
                let call = module
                    .typed
                    .type_table()
                    .native_default_call_ref(function.id)
                    .ok_or(AdmissionError::InvalidModule(
                        "native default has no checked forwarding call",
                    ))?;
                let declaration = match call.target {
                    CallTarget::Function(id) => self
                        .environment
                        .definition(ResolvedName::Function(id))
                        .ok_or(AdmissionError::InvalidModule(
                            "native default target has no declaration",
                        ))?,
                    CallTarget::SourceFunction(id) => id,
                    _ => {
                        return Err(AdmissionError::InvalidModule(
                            "native default target is not a declaration",
                        ));
                    }
                };
                let signature = call
                    .signature
                    .as_ref()
                    .ok_or(AdmissionError::InvalidModule(
                        "native default has no applied signature",
                    ))?;
                self.native_defaults.insert(
                    function.id,
                    NativeDefaultCall {
                        declaration,
                        arguments: &call.type_arguments,
                        signature,
                    },
                );
            }
        }
        Ok(())
    }

    fn admit_initializers(
        &mut self,
        module: &'a CheckedAnalysis,
        cancel: &CancellationToken,
    ) -> Result<(), AdmissionError> {
        for item in &module.lowered.module.statics {
            cancel.check().map_err(|_| AdmissionError::Cancelled)?;
            let owner = BodyOwner::Static(item.id);
            if !module.typed.valid_static_initializers.contains(&item.id) {
                return Err(AdmissionError::InvalidBody {
                    key: BodyKey {
                        module: self.handle,
                        arena: module.lowered.module.body.arena(),
                        owner,
                        closure: None,
                    },
                    reason: "static initializer has no positive completion evidence",
                });
            }
            self.bodies.insert(
                owner,
                CheckedBodyInput::new(
                    module,
                    self.handle,
                    owner,
                    BodyRoot::Expression(item.initializer),
                )?,
            );
        }
        for item in &module.lowered.module.consts {
            cancel.check().map_err(|_| AdmissionError::Cancelled)?;
            let owner = BodyOwner::Const(item.id);
            if self
                .environment
                .local_constant(item.id)
                .is_none_or(|constant| constant.ty.is_unresolved())
            {
                return Err(AdmissionError::InvalidBody {
                    key: BodyKey {
                        module: self.handle,
                        arena: module.lowered.module.body.arena(),
                        owner,
                        closure: None,
                    },
                    reason: "constant has no checked type and evaluated value",
                });
            }
            self.bodies.insert(
                owner,
                CheckedBodyInput::new(
                    module,
                    self.handle,
                    owner,
                    BodyRoot::Expression(item.initializer),
                )?,
            );
        }
        Ok(())
    }

    /// Returns this module's checked definition identity.
    pub fn handle(&self) -> ModuleHandle {
        self.handle
    }

    /// Borrows retained source identity/revision and text for origins and diagnostics.
    pub fn source(&self) -> &'a SourceFile {
        self.source
    }

    /// Borrows immutable declaration contracts, separate from body nodes and facts.
    pub fn environment(&self) -> CheckedEnvironment<'a> {
        self.environment
    }

    /// Borrows declaration metadata without exposing the shared body arena.
    pub fn headers(&self) -> DeclarationHeaders<'a> {
        self.headers
    }

    /// Borrows exact installed declarations and their scope, or `None` for source modules.
    pub fn registration(&self) -> Option<RegisteredDeclarations<'a>> {
        self.registration
    }

    /// Returns a declaration's physical source extent, or `None` for an absent local slot.
    ///
    /// Plain item IDs require this module's context; they cannot detect a coincident
    /// numeric slot from another module. Use canonical identities across modules.
    pub fn declaration_span(&self, item: Item) -> Option<Span> {
        self.headers
            .contains(item)
            .then(|| self.origins.item_span(item))
    }

    /// Borrows the proved forwarding application for a native default in this module.
    ///
    /// Ordinary script/native functions and required methods return `None`. Admission
    /// rejects a native default missing this proof instead of treating it as bodyless.
    pub fn native_default(&self, function: FunctionId) -> Option<NativeDefaultCall<'a>> {
        self.native_defaults.get(&function).copied()
    }

    /// Borrows the checked receiver/binder contract of a local implementation header.
    ///
    /// Includes inherent implementations with no methods. `ImplId` is module-local;
    /// use the returned canonical declaration identity when crossing module boundaries.
    pub fn implementation(&self, id: ImplId) -> Option<&ImplementationInput<'a>> {
        self.implementations.get(&id)
    }

    /// Finds a source body; callers inspect the signature implementation for bodyless declarations.
    pub fn body(&self, key: BodyKey) -> Option<CheckedBodyInput<'a>> {
        let body = *self.bodies.get(&key.owner)?;
        let body = if let Some(closure) = key.closure {
            body.closure(closure)?
        } else {
            body
        };
        (body.key() == key).then_some(body)
    }

    /// Visits admitted bodies; order is unspecified and does not define execution order.
    pub fn bodies(&self) -> impl Iterator<Item = CheckedBodyInput<'a>> + '_ {
        self.bodies.values().copied()
    }
}

/// Fixed environments and source-body inputs for a complete checked dependency closure.
///
/// The borrowed lifetime retains the original immutable proof and source storage.
/// Construction accepts only a checked closure, then verifies body completion and
/// source ownership. It cannot be assembled from independent headers and body facts.
/// Native/required declarations keep their explicit signature implementation kind.
/// No source analysis, type inference or execution is performed by this object.
///
/// This is the HIR-to-elaboration contract; the compiler's short-lived expanded
/// body is a separate representation for MIR construction.
pub struct CheckedCompilation<'a> {
    modules: Vec<CheckedModule<'a>>,
    root: ModuleHandle,
    dependencies: HashMap<ModuleHandle, &'a [ModuleHandle]>,
    exports: &'a ProgramExports,
}

impl<'a> CheckedCompilation<'a> {
    /// Admits all bodies of an already checked source dependency closure.
    ///
    /// # Errors
    ///
    /// Returns cancellation or missing/inconsistent completion, signature or body
    /// ownership evidence. It never drops invalid unreachable bodies to admit a program.
    pub fn from_program(
        program: &'a CheckedProgram,
        cancel: &CancellationToken,
    ) -> Result<Self, AdmissionError> {
        cancel.check().map_err(|_| AdmissionError::Cancelled)?;
        let mut modules = Vec::with_capacity(program.modules().len());
        let mut dependencies = HashMap::new();
        let mut root = None;
        for module in program.modules() {
            let admitted = CheckedModule::admit(module, cancel)?;
            if module.lowered.source.id() == program.root().lowered.source.id() {
                root = Some(admitted.handle);
            }
            dependencies.insert(
                admitted.handle,
                program
                    .dependencies(&admitted.handle)
                    .ok_or(AdmissionError::InvalidModule(
                        "dependency directory missing module",
                    ))?,
            );
            modules.push(admitted);
        }
        cancel.check().map_err(|_| AdmissionError::Cancelled)?;
        Ok(Self {
            modules,
            root: root.ok_or(AdmissionError::InvalidModule("root absent from closure"))?,
            dependencies,
            exports: program.exports(),
        })
    }

    /// Returns the root module's identity in this fixed closure.
    pub fn root(&self) -> ModuleHandle {
        self.root
    }

    /// Visits modules in the checked closure's deterministic order, including cycles once.
    pub fn modules(&self) -> &[CheckedModule<'a>] {
        &self.modules
    }

    /// Looks up direct dependency edges; foreign module identities return `None`.
    pub fn dependencies(&self, module: ModuleHandle) -> Option<&'a [ModuleHandle]> {
        self.dependencies.get(&module).copied()
    }

    /// Borrows checked public bindings and the separately selected root entry point.
    pub fn exports(&self) -> &'a ProgramExports {
        self.exports
    }
}

#[cfg(test)]
mod tests;
