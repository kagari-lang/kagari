//! Author ordinary implementation expressions after trait signature projection.
use crate::native::{
    assembly::{conflict, references::DraftMember},
    binding::NativeResult,
    types::Type,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::declaration::{FnDecl, script::ScriptExpr};

pub(super) type ScriptBody = Box<dyn FnOnce(&ScriptMethodScope<'_>) -> NativeResult<ScriptExpr>>;

/// Body inputs in the actual implementation method's scope. Capture impl parameters
/// from `trait_implementation_with`; obtain method-local parameters here instead of
/// using the uninstantiated trait member's binder. This authors an expression, not
/// a dispatch witness: ordinary Script checking still has to prove the call.
pub struct ScriptMethodScope<'a> {
    pub(super) context: &'a DefinitionContext,
    pub(super) signature: &'a FnDecl,
    pub(super) impl_arity: usize,
}

impl ScriptMethodScope<'_> {
    /// Refer to the projected value parameter. For instance methods, zero is `self`.
    pub fn parameter(&self, position: usize) -> NativeResult<ScriptExpr> {
        self.signature
            .params
            .get(position)
            .map(|parameter| ScriptExpr::Name(parameter.name.clone()))
            .ok_or_else(|| conflict("Script method value parameter index"))
    }

    /// A type parameter declared by the method (index excludes the impl prefix).
    pub fn type_parameter(&self, position: usize) -> NativeResult<Type> {
        let parameter = self.signature.generic_params[self.impl_arity..]
            .get(position)
            .ok_or_else(|| conflict("Script method type parameter index"))?;
        Type::from_semantic(parameter.as_type(), &self.context.snapshot())
    }

    /// Call an identity-addressed trait method on a type, e.g. `D::from(self)`
    /// under `D: From<S>`. `type_arguments` are the member's own arguments only;
    /// the interface already contains its trait arguments. No native slot is allocated.
    pub fn trait_call(
        &self,
        member: &DraftMember,
        receiver: Type,
        interface: Type,
        type_arguments: impl IntoIterator<Item = Type>,
        arguments: impl IntoIterator<Item = ScriptExpr>,
    ) -> NativeResult<ScriptExpr> {
        let target = member
            .requirement(receiver, interface, type_arguments)?
            .lower(self.context)?;
        Ok(ScriptExpr::Call {
            callee: Box::new(ScriptExpr::TraitMethod {
                receiver: target.receiver,
                interface: target.interface,
                member: target.member,
            }),
            type_arguments: target.arguments,
            arguments: arguments.into_iter().collect(),
        })
    }
}
