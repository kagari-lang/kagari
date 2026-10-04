//! Lower checked propagation facts through ordinary selected calls and enum layouts.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_hir::{
    hir::ids::ExprId,
    typeck::table::{CallTarget, ResolvedCall},
};
use kagari_mir::instruction::{MirValue, Terminator};

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_propagation(
        &mut self,
        site: ExprId,
        operand: ExprId,
    ) -> Result<MirValue, MirLoweringError> {
        let value = self.lower_expr(operand)?;
        if self.current_block_terminated() {
            return Ok(value);
        }
        let fact = self
            .analyzed
            .typed
            .type_table
            .propagation(site)
            .cloned()
            .ok_or(MirLoweringError::MissingBinding(
                "checked propagation calls",
            ))?;
        let receiver = self
            .analyzed
            .typed
            .type_table
            .expr_type(operand)
            .ok_or(MirLoweringError::MissingExprType(operand))?;
        let branch_type = fact
            .branch
            .signature
            .as_ref()
            .ok_or(MirLoweringError::MissingBinding("checked branch signature"))?
            .return_type
            .clone();
        let CallTarget::TraitMethod { method, interface } = fact.branch.target else {
            return Err(MirLoweringError::MissingBinding("checked Try branch"));
        };
        let branch = self.lower_applied_operator(interface, receiver, &method, &[value])?;
        let success_variant = self
            .planner
            .catalog
            .variant(&fact.continue_variant)
            .ok_or(MirLoweringError::MissingBinding(
                "checked ControlFlow Continue",
            ))?
            .slot;
        let failure_variant = self
            .planner
            .catalog
            .variant(&fact.break_variant)
            .ok_or(MirLoweringError::MissingBinding(
                "checked ControlFlow Break",
            ))?
            .slot;
        let condition = self.test_enum_variant(&branch_type, branch, success_variant)?;
        let success = self.new_block();
        let failure = self.new_block();
        self.set_terminator(Terminator::Branch {
            cond: condition,
            then_block: success,
            else_block: failure,
        });
        self.switch_to_block(failure);
        let residual = self.read_enum_field(&branch_type, branch, failure_variant, 0)?;
        let ResolvedCall {
            target: CallTarget::TraitMethod { method, interface },
            ..
        } = fact.from_residual
        else {
            return Err(MirLoweringError::MissingBinding(
                "checked FromResidual call",
            ));
        };
        let returned =
            self.lower_applied_operator(interface, fact.return_type, &method, &[residual])?;
        self.set_terminator(Terminator::Return(Some(returned)));
        self.switch_to_block(success);
        self.read_enum_field(&branch_type, branch, success_variant, 0)
    }
}
