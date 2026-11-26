//! Authenticate supplied ordinary Script expressions at the installed-library boundary.
//!
//! Product origins and instruction certificates establish internal consistency, not
//! the semantics of an installed Script body. The independent authority here is the
//! installed module's ordinary expression, never a digest supplied by the artifact.
mod trait_calls;

use super::{NativeRegistry, forwarding::source_member};
use crate::{
    error::RuntimeError,
    module::{VerifiedProgram, admission::DefinitionAdmission},
    native::binding::NativeResult,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction as I, CallTarget, Slot},
    module::{BytecodeFunction, BytecodeModule, CallableTarget},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::RemapDefinitions,
        table::DefinitionId,
    },
};
use kagari_contract::types::{
    InterfaceTable, PublicItem, named::NamedContract, proofs::ProofCatalog,
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{
        FnDecl,
        conversion::{forwarding_expression, is_library_forwarding_trait},
        module::ModuleDecl,
        script::ScriptExpr,
    },
    language::Protocol,
    scalar::BuiltinType,
    ty::{
        GenericBound, GenericParam, NominalTy, Ty,
        matching::{ImplementationPattern, match_receiver, projection_output},
        substitution::{TypeSubstitution, normalize_projections, resolve_associated_outputs},
    },
};
use std::collections::HashMap;
use trait_calls::{QualifiedMethod, proof_scope};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InstalledScript {
    function: FnDecl,
    body: ScriptExpr,
}

impl InstalledScript {
    fn product_body(
        &self,
        admission: &DefinitionAdmission,
        cancel: &CancellationToken,
    ) -> NativeResult<ScriptExpr> {
        // A qualified call can be the only explicit use of a trait member ID.
        // Bytecode dispatch can encode just a slot; its verifier interns the member
        // in the product table without adding an explicit admission reference.
        self.body
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    admission
                        .lexical_id(*id)
                        .or_else(|| {
                            let installed = admission.target().resolve(*id).ok()?;
                            admission.source().lookup(&installed.to_path())
                        })
                        .ok_or(DefinitionMappingError::InvalidContract)
                },
                admission.source(),
                cancel,
            ))
            .map_err(|_| invalid())
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("Script executable differs from its installed canonical body")
}

impl NativeRegistry {
    pub(crate) fn install_scripts(&mut self, module: &ModuleDecl) -> NativeResult<()> {
        let definitions = self.catalog.definitions();
        let mut functions = Vec::new();
        for function in &module.functions {
            functions.push((
                module
                    .definition(DefinitionKind::Function, &function.name, &definitions)
                    .map_err(|_| invalid())?,
                function,
                &[][..],
                &[][..],
            ));
        }
        for contract in &module.traits {
            let owner = module
                .definition(DefinitionKind::Trait, &contract.name, &definitions)
                .map_err(|_| invalid())?;
            for function in &contract.methods {
                functions.push((
                    ModuleDecl::method_id(owner, &function.name, &definitions)
                        .map_err(|_| invalid())?,
                    function,
                    &[][..],
                    &[][..],
                ));
            }
        }
        for (index, implementation) in module.implementations.iter().enumerate() {
            let owner = module
                .implementation_id(index, &definitions)
                .map_err(|_| invalid())?;
            for function in &implementation.methods {
                functions.push((
                    ModuleDecl::method_id(owner, &function.name, &definitions)
                        .map_err(|_| invalid())?,
                    function,
                    implementation.generic_params.as_slice(),
                    implementation.bounds.as_slice(),
                ));
            }
        }
        for (id, function, impl_parameters, impl_bounds) in functions {
            let Some(body) = module.script_bodies.get(&id) else {
                // Parsed Script source can be supplied without a structured body. It
                // grants no independent executable authority for an inherited default.
                continue;
            };
            let expected = InstalledScript {
                function: explicit_signature(function, impl_parameters, impl_bounds)?,
                body: body.clone(),
            };
            if self
                .scripts
                .get_id(id)
                .is_some_and(|previous| previous != &expected)
            {
                return Err(RuntimeError::metadata_conflict(
                    "conflicting canonical Script owner",
                ));
            }
            self.scripts
                .insert_id(id, expected)
                .map_err(|_| invalid())?;
        }
        Ok(())
    }

    pub(super) fn validate_scripts(
        &self,
        module: &BytecodeModule,
        program: &VerifiedProgram,
        admission: &DefinitionAdmission,
    ) -> NativeResult<()> {
        let cancel = CancellationToken::default();
        let defs = program.definitions();
        let convert = Protocol::From.module();
        let mut blankets = HashMap::new();
        if !self.catalog.contains_module(&convert)
            && let Some(owner) = program.modules().iter().find(|m| m.identity == convert)
        {
            for item in &owner.public_items {
                if let PublicItem::InterfaceTable(table) = item
                    && let Ty::Trait(interface) = &table.trait_type
                    && is_library_forwarding_trait(defs, interface.declaration)
                {
                    blankets.insert(source_member(table, defs).ok_or_else(invalid)?, table);
                }
            }
        }
        let mut proofs = None;
        for function in &module.functions {
            let identity = function.identity.as_ref().ok_or_else(invalid)?;
            let placement = identity.declaration;
            let inherited = program
                .modules()
                .iter()
                .flat_map(|owner| &owner.public_items)
                .filter_map(|item| match item {
                    PublicItem::InterfaceTable(table) => Some(table),
                    _ => None,
                })
                .find_map(|table| {
                    table
                        .method_origins
                        .iter()
                        .find(|origin| origin.placement == placement)
                        .map(|origin| (table, origin.authored))
                });
            let canonical = inherited
                .map(|(_, id)| id)
                .or_else(|| {
                    module
                        .function_origins
                        .iter()
                        .find(|origin| origin.placement == placement)
                        .map(|origin| origin.authored)
                })
                .unwrap_or(placement);
            if canonical != placement
                && (blankets.contains_key(&canonical)
                    || blankets.contains_key(&placement)
                    || self
                        .scripts
                        .get_id(admission.runtime_id(placement)?)
                        .is_some())
            {
                // An authored implementation cannot be replaced by an inherited body.
                return Err(invalid());
            }
            let installed = admission.runtime_id(canonical)?;
            let (mut body, function_contract) = if let Some(script) = self.scripts.get_id(installed)
            {
                (
                    script.product_body(admission, &cancel)?,
                    admission.product(&script.function, &cancel)?,
                )
            } else if inherited.is_some()
                && self
                    .catalog
                    .contains_module(defs.resolve(canonical).map_err(|_| invalid())?.module())
            {
                // An installed default needs independent body evidence. Ordinary authored
                // traits have no installed owner and use the verified product instead.
                return Err(invalid());
            } else if let Some(table) = blankets.get(&placement) {
                let method = &table.methods[0];
                (
                    forwarding_expression(defs, &table.bounds[0], method.name == "try_into")
                        .ok_or_else(invalid)?,
                    explicit_signature(method, &table.generic_params, &table.bounds)?,
                )
            } else {
                continue;
            };
            let arguments = identity
                .arguments
                .iter()
                .cloned()
                .chain(
                    function
                        .metadata
                        .semantic
                        .generic
                        .iter()
                        .flat_map(|generic| {
                            generic
                                .parameters
                                .iter()
                                .map(|parameter| parameter.as_type())
                        }),
                )
                .collect::<Vec<_>>();
            let mut bindings = TypeSubstitution::default();
            let mut applied_interface = None;
            let mut applied_receiver = None;
            if let Some((table, _)) = inherited {
                for (parameter, argument) in table.generic_params.iter().zip(&arguments) {
                    bindings.bind(&parameter.owner, parameter.position, argument);
                }
                let Ty::Trait(interface) = &table.trait_type else {
                    return Err(invalid());
                };
                applied_interface = Some(
                    bindings
                        .apply_nominal(interface, &cancel)
                        .map_err(|_| invalid())?,
                );
                applied_receiver = Some(
                    bindings
                        .apply(&table.for_type, &cancel)
                        .map_err(|_| invalid())?,
                );
            }
            let mut application = TypeSubstitution::default();
            if let Some(interface) = &applied_interface {
                for (position, argument) in interface.arguments.iter().enumerate() {
                    application.bind(&interface.declaration, position, argument);
                }
                application.bind_receiver(
                    &interface.declaration,
                    applied_receiver.as_ref().ok_or_else(invalid)?,
                );
            }
            let offset = inherited.map_or(0, |(table, _)| table.generic_params.len());
            if function_contract.generic_params.len() + offset != arguments.len() {
                return Err(invalid());
            }
            for (parameter, argument) in function_contract
                .generic_params
                .iter()
                .zip(&arguments[offset..])
            {
                application.bind(&parameter.owner, parameter.position, argument);
            }
            let interface = match inherited {
                Some((table, _)) => Some(AppliedInterface {
                    table,
                    interface: applied_interface.as_ref().ok_or_else(invalid)?,
                    receiver: applied_receiver.as_ref().ok_or_else(invalid)?,
                }),
                None => None,
            };

            // A blanket forwards its argument without even constructing a closure.
            // Installed expressions may legitimately name a function value.
            let trace = Trace::read(
                module,
                function,
                program,
                !blankets.contains_key(&placement),
            )?;
            let qualified = trait_calls::contains(&body);
            if qualified && proofs.is_none() {
                proofs = Some(proof_scope(module, program)?);
            }
            let mut expression = Expression {
                program,
                proofs: proofs.as_ref(),
                canonical,
                function: &function_contract,
                application: &application,
                interface,
                trace: &trace,
                next: 0,
            };
            if qualified {
                if function.metadata.semantic.generic.is_none() && !identity.arguments.is_empty() {
                    expression.check_specialization(function_signature(function)?)?;
                }
                expression.fold_identity_calls(&mut body)?;
            }
            let expected_result = expression.apply_type(&function_contract.return_type)?;
            let result = expression
                .value(&body, Some(&expected_result))
                .map_err(|error| {
                    if blankets.contains_key(&placement) {
                        RuntimeError::module_validation(format!(
                            "{} differs from the required qualified forward call",
                            function_contract.name
                        ))
                    } else {
                        error
                    }
                })?;
            if result != trace.result || expression.next != trace.calls.len() {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

// Executable instances order impl arguments before method-local arguments. A
// projected structured FnDecl may already carry that prefix; never bind it twice.
fn explicit_signature(
    function: &FnDecl,
    prefix: &[GenericParam],
    impl_bounds: &[GenericBound],
) -> NativeResult<FnDecl> {
    let mut function = function.clone();
    if !function.generic_params.starts_with(prefix) {
        if function.generic_params.iter().any(|p| prefix.contains(p)) {
            return Err(invalid());
        }
        function.generic_params.splice(0..0, prefix.iter().cloned());
    }
    for bound in impl_bounds {
        if !function.bounds.contains(bound) {
            function.bounds.push(bound.clone());
        }
    }
    Ok(function)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Parameter(usize),
    Definition(DefinitionId),
    TraitMethod(Box<QualifiedMethod>),
    Convert,
    Result(usize),
    Unit,
}

struct AppliedCall {
    callee: Value,
    entry: Option<CallableImplementation>,
    arguments: Vec<Value>,
    types: Vec<Ty>,
    result: Value,
    signature: Signature,
}

impl AppliedCall {
    // Unqualified Script names refer to authored functions. A qualified call instead
    // authenticates the actual executable placement against the selected impl member.
    fn names(&self, expected: &Value, program: &VerifiedProgram) -> bool {
        match (&self.callee, &self.entry, expected) {
            (
                Value::Definition(target),
                Some(CallableImplementation::Script),
                Value::Definition(id),
            ) => canonical(*target, program) == *id,
            _ => self.callee == *expected,
        }
    }
}

struct Trace {
    calls: Vec<AppliedCall>,
    result: Value,
}

fn canonical(declaration: DefinitionId, program: &VerifiedProgram) -> DefinitionId {
    program
        .modules()
        .iter()
        .flat_map(|module| &module.function_origins)
        .find(|origin| origin.placement == declaration)
        .map_or(declaration, |origin| origin.authored)
}

impl Trace {
    // The supplied grammar contains only values and left-to-right calls. Moves
    // preserve symbolic values; every other executable effect needs a matching
    // expression, so an extra or substituted effect cannot hide in discarded code.
    fn read(
        module: &BytecodeModule,
        function: &BytecodeFunction,
        program: &VerifiedProgram,
        allow_closures: bool,
    ) -> NativeResult<Self> {
        let mut slots = HashMap::new();
        for (index, slot) in function.metadata.parameter_slots.iter().enumerate() {
            slots.insert(*slot, Value::Parameter(index));
        }
        let mut calls = Vec::new();
        let get = |slots: &HashMap<Slot, Value>, slot: &Slot| {
            slots.get(slot).cloned().ok_or_else(invalid)
        };
        let mut result = None;
        for instruction in &function.instructions {
            if result.is_some() {
                return Err(invalid());
            }
            match instruction {
                I::Move { dst, src } => {
                    slots.insert(*dst, get(&slots, src)?);
                }
                I::Convert {
                    dst,
                    src,
                    conversion,
                } => {
                    let (input, output) = conversion.contract().ok_or_else(invalid)?;
                    let result = Value::Result(calls.len());
                    calls.push(AppliedCall {
                        callee: Value::Convert,
                        entry: None,
                        types: vec![],
                        arguments: vec![get(&slots, src)?],
                        result: result.clone(),
                        signature: Signature {
                            params: vec![input],
                            result: output,
                        },
                    });
                    slots.insert(*dst, result);
                }
                I::MakeClosure {
                    dst,
                    function,
                    captures,
                } if allow_closures && captures.is_empty() => {
                    let identity = module
                        .functions
                        .get(function.index())
                        .and_then(|function| function.identity.as_ref())
                        .ok_or_else(invalid)?;
                    if !identity.arguments.is_empty() {
                        return Err(invalid());
                    }
                    slots.insert(
                        *dst,
                        Value::Definition(canonical(identity.declaration, program)),
                    );
                }
                I::Call { dst, callee, args } => {
                    let (callee, entry, types, signature) = match callee {
                        CallTarget::Native(id) => {
                            let import =
                                module.native_imports.get(id.index()).ok_or_else(invalid)?;
                            (
                                Value::Definition(import.instance.declaration),
                                Some(CallableImplementation::Native(import.binding)),
                                import.instance.arguments.clone(),
                                import.signature.clone(),
                            )
                        }
                        CallTarget::Shared {
                            module,
                            target,
                            contract,
                        } => {
                            let owner =
                                program.modules().get(module.index()).ok_or_else(invalid)?;
                            let (identity, entry, types) = match target {
                                CallableTarget::Script(id) => {
                                    let identity = owner
                                        .functions
                                        .get(id.index())
                                        .and_then(|f| f.identity.as_ref())
                                        .ok_or_else(invalid)?;
                                    let types = identity
                                        .arguments
                                        .iter()
                                        .chain(&contract.arguments)
                                        .cloned()
                                        .collect();
                                    (identity, CallableImplementation::Script, types)
                                }
                                CallableTarget::Native(id) => {
                                    let import =
                                        owner.native_imports.get(id.index()).ok_or_else(invalid)?;
                                    let template = import.generic.as_ref().ok_or_else(invalid)?;
                                    let mut subst = TypeSubstitution::default();
                                    for (param, ty) in
                                        template.parameters.iter().zip(&contract.arguments)
                                    {
                                        subst.bind(&param.owner, param.position, ty);
                                    }
                                    let types = import
                                        .instance
                                        .arguments
                                        .iter()
                                        .map(|ty| {
                                            subst
                                                .apply(ty, &Default::default())
                                                .map_err(|_| invalid())
                                        })
                                        .collect::<NativeResult<_>>()?;
                                    (
                                        &import.instance,
                                        CallableImplementation::Native(import.binding),
                                        types,
                                    )
                                }
                            };
                            (
                                Value::Definition(identity.declaration),
                                Some(entry),
                                types,
                                contract.signature.clone(),
                            )
                        }
                        CallTarget::InterfaceMethod { module, contract } => {
                            let owner =
                                program.modules().get(module.index()).ok_or_else(invalid)?;
                            let (method, signature) = QualifiedMethod::checked_slot(
                                contract,
                                owner,
                                program.definitions(),
                            )?;
                            (
                                Value::TraitMethod(Box::new(method)),
                                None,
                                contract.arguments.clone(),
                                signature,
                            )
                        }
                        CallTarget::Function(id) => {
                            let target = module.functions.get(id.index()).ok_or_else(invalid)?;
                            let identity = target.identity.as_ref().ok_or_else(invalid)?;
                            (
                                Value::Definition(identity.declaration),
                                Some(CallableImplementation::Script),
                                identity.arguments.clone(),
                                function_signature(target)?,
                            )
                        }
                        CallTarget::ModuleFunction { module, function } => {
                            let target = program
                                .modules()
                                .get(module.index())
                                .and_then(|owner| owner.functions.get(function.index()))
                                .ok_or_else(invalid)?;
                            let identity = target.identity.as_ref().ok_or_else(invalid)?;
                            (
                                Value::Definition(identity.declaration),
                                Some(CallableImplementation::Script),
                                identity.arguments.clone(),
                                function_signature(target)?,
                            )
                        }
                        CallTarget::Slot(slot)
                        | CallTarget::ClosureRegister { register: slot, .. } => {
                            let Ty::Function { params, result } = function
                                .metadata
                                .semantic
                                .slots
                                .get(&slot.index())
                                .ok_or_else(invalid)?
                            else {
                                return Err(invalid());
                            };
                            (
                                get(&slots, slot)?,
                                None,
                                vec![],
                                Signature {
                                    params: params.clone(),
                                    result: *result.clone(),
                                },
                            )
                        }
                        _ => return Err(invalid()),
                    };
                    let arguments = args
                        .iter()
                        .map(|slot| get(&slots, slot))
                        .collect::<NativeResult<_>>()?;
                    let result = if let Some(dst) = dst {
                        let result = Value::Result(calls.len());
                        slots.insert(*dst, result.clone());
                        result
                    } else {
                        Value::Unit
                    };
                    calls.push(AppliedCall {
                        callee,
                        entry,
                        types,
                        arguments,
                        result,
                        signature,
                    });
                }
                I::Return(value) => {
                    result = Some(
                        value
                            .as_ref()
                            .map(|slot| get(&slots, slot))
                            .transpose()?
                            .unwrap_or(Value::Unit),
                    );
                }
                _ => return Err(invalid()),
            }
        }
        Ok(Self {
            calls,
            result: result.ok_or_else(invalid)?,
        })
    }
}

/// A selected, verified impl application, not a catalog for Runtime trait search.
struct AppliedInterface<'a> {
    table: &'a InterfaceTable,
    interface: &'a NominalTy,
    receiver: &'a Ty,
}

struct Expression<'a, 'b> {
    program: &'a VerifiedProgram,
    proofs: Option<&'a ProofCatalog<'a>>,
    canonical: DefinitionId,
    function: &'a FnDecl,
    application: &'a TypeSubstitution<'b>,
    interface: Option<AppliedInterface<'a>>,
    trace: &'a Trace,
    next: usize,
}

fn function_signature(function: &BytecodeFunction) -> NativeResult<Signature> {
    let semantic = &function.metadata.semantic;
    Ok(Signature {
        params: (0..usize::from(function.parameter_count))
            .map(|index| semantic.params.get(&index).cloned().ok_or_else(invalid))
            .collect::<NativeResult<_>>()?,
        result: semantic.result.clone().ok_or_else(invalid)?,
    })
}

fn call_count(expression: &ScriptExpr) -> usize {
    match expression {
        ScriptExpr::Call {
            callee, arguments, ..
        } => 1 + call_count(callee) + arguments.iter().map(call_count).sum::<usize>(),
        _ => 0,
    }
}

impl Expression<'_, '_> {
    fn normalize_type(&self, ty: &Ty) -> NativeResult<Ty> {
        let Some(applied) = &self.interface else {
            return Ok(ty.clone());
        };
        let Ty::Trait(template) = &applied.table.trait_type else {
            return Err(invalid());
        };
        let cancel = CancellationToken::default();
        let resolved = resolve_associated_outputs(ty, applied.interface, applied.receiver, &cancel)
            .map_err(|_| invalid())?;
        // Reuse only this checked impl's outputs/families. The bounded transform
        // leaves unrelated projections symbolic and rejects cyclic expansions.
        normalize_projections(
            &resolved,
            &|interface, receiver, member, arguments| {
                projection_output(
                    ImplementationPattern {
                        parameters: &applied.table.generic_params,
                        receiver: &applied.table.for_type,
                        interface: template,
                    },
                    &applied.table.associated_type_families,
                    interface,
                    receiver,
                    member,
                    arguments,
                    &cancel,
                )
            },
            &cancel,
        )
        .map_err(|_| invalid())
    }

    fn apply_type(&self, ty: &Ty) -> NativeResult<Ty> {
        self.normalize_type(
            &self
                .application
                .apply(ty, &CancellationToken::default())
                .map_err(|_| invalid())?,
        )
    }

    fn callable(&self, id: DefinitionId) -> NativeResult<&FnDecl> {
        self.program
            .modules()
            .iter()
            .find_map(|owner| {
                owner
                    .native_declarations
                    .iter()
                    .find(|declaration| declaration.declaration == id)
                    .map(|declaration| &declaration.function)
                    .or_else(|| {
                        owner.named_declarations.iter().find_map(|declaration| {
                            if declaration.declaration != id {
                                return None;
                            }
                            match &declaration.contract {
                                NamedContract::Function(function) => Some(function),
                                _ => None,
                            }
                        })
                    })
            })
            .ok_or_else(invalid)
    }

    fn value_type(&self, value: &Value) -> NativeResult<Ty> {
        Ok(match value {
            Value::Parameter(index) => {
                self.apply_type(&self.function.params.get(*index).ok_or_else(invalid)?.ty)?
            }
            Value::Definition(id) => {
                let function = self.callable(*id)?;
                self.normalize_type(&Ty::Function {
                    params: function
                        .params
                        .iter()
                        .map(|parameter| parameter.ty.clone())
                        .collect(),
                    result: Box::new(function.return_type.clone()),
                })?
            }
            Value::Result(index) => self.normalize_type(
                &self
                    .trace
                    .calls
                    .get(*index)
                    .ok_or_else(invalid)?
                    .signature
                    .result,
            )?,
            Value::TraitMethod(_) | Value::Convert => return Err(invalid()),
            Value::Unit => Ty::Builtin(BuiltinType::Unit),
        })
    }

    fn inferred_arguments(
        &self,
        function: &FnDecl,
        values: &[Value],
        expected: Option<&Ty>,
        apply: impl Fn(&Ty) -> NativeResult<Ty>,
    ) -> NativeResult<Vec<Ty>> {
        let mut template = function
            .params
            .iter()
            .map(|parameter| apply(&parameter.ty))
            .collect::<NativeResult<Vec<_>>>()?;
        let mut actual = values
            .iter()
            .map(|value| self.value_type(value))
            .collect::<NativeResult<Vec<_>>>()?;
        if let Some(expected) = expected {
            template.push(apply(&function.return_type)?);
            actual.push(self.normalize_type(expected)?);
        }
        let template = Ty::Tuple(template);
        let actual = Ty::Tuple(actual);
        // Authenticate an already checked application by structural signature
        // matching, not trait search or Runtime type inference. Every omitted
        // argument must be determined by ordinary inputs or the result context.
        let bindings = match_receiver(
            &function.generic_params,
            &template,
            &actual,
            &CancellationToken::default(),
        )
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
        function
            .generic_params
            .iter()
            .map(|parameter| {
                bindings
                    .parameter(&parameter.owner, parameter.position)
                    .cloned()
                    .ok_or_else(invalid)
            })
            .collect()
    }

    fn value(&mut self, expression: &ScriptExpr, expected: Option<&Ty>) -> NativeResult<Value> {
        let value = match expression {
            ScriptExpr::Name(name) => {
                if let Some(index) = self
                    .function
                    .params
                    .iter()
                    .position(|parameter| &parameter.name == name)
                {
                    Value::Parameter(index)
                } else {
                    let definitions = self.program.definitions();
                    let owner = definitions.resolve(self.canonical).map_err(|_| invalid())?;
                    let root = definitions
                        .lookup_root(owner.module())
                        .ok_or_else(invalid)?;
                    Value::Definition(
                        definitions
                            .lookup_child(root, DefinitionKind::Function, name, 0)
                            .ok_or_else(invalid)?,
                    )
                }
            }
            ScriptExpr::Definition(id) => Value::Definition(*id),
            // A trait member ID alone does not authenticate a selected implementation.
            ScriptExpr::TraitMethod { .. } => return Err(invalid()),
            ScriptExpr::Call {
                callee,
                type_arguments,
                arguments,
            } => {
                let outer = self.next + call_count(expression) - 1;
                let signature = self
                    .trace
                    .calls
                    .get(outer)
                    .ok_or_else(invalid)?
                    .signature
                    .clone();
                if signature.params.len() != arguments.len() {
                    return Err(invalid());
                }
                let callee = match callee.as_ref() {
                    ScriptExpr::TraitMethod {
                        receiver,
                        interface,
                        member,
                    } => {
                        Value::TraitMethod(Box::new(self.qualified(receiver, interface, *member)?))
                    }
                    other => self.value(other, None)?,
                };
                let arguments = arguments
                    .iter()
                    .zip(&signature.params)
                    .map(|(argument, parameter)| self.value(argument, Some(parameter)))
                    .collect::<NativeResult<Vec<_>>>()?;
                let types = type_arguments
                    .iter()
                    .map(|ty| self.apply_type(ty))
                    .collect::<NativeResult<Vec<_>>>()?;
                let actual = self.trace.calls.get(self.next).ok_or_else(invalid)?;
                if actual.arguments != arguments {
                    return Err(invalid());
                }
                if let Value::TraitMethod(method) = callee {
                    self.authenticate_method(&method, &types, actual, &arguments, expected)?;
                } else if !actual.names(&callee, self.program) {
                    return Err(invalid());
                } else if types.is_empty() && !actual.types.is_empty() {
                    let Value::Definition(id) = callee else {
                        return Err(invalid());
                    };
                    let inferred =
                        self.inferred_arguments(self.callable(id)?, &arguments, expected, |ty| {
                            self.normalize_type(ty)
                        })?;
                    let applied = actual
                        .types
                        .iter()
                        .map(|ty| self.normalize_type(ty))
                        .collect::<NativeResult<Vec<_>>>()?;
                    if inferred != applied {
                        return Err(invalid());
                    }
                } else if actual.types != types {
                    return Err(invalid());
                }
                let value = actual.result.clone();
                self.next += 1;
                value
            }
        };

        Ok(value)
    }
}
