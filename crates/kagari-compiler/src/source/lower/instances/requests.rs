//! Retained update products enter the same bounded queues as checked source calls.

use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::span::Span;
use kagari_contract::updates::{DemandRequests, InterfaceRequest};
use kagari_hir::{
    aggregates::implementations::{ImplementationSearchContext, ImplementationSearchLimits},
    types::semantic::{raise_nominal_type, raise_type},
};
use kagari_types::ty::GenericParam;

impl InstancePlanner<'_> {
    pub(in crate::source::lower) fn request_products(
        &mut self,
        requests: &DemandRequests,
    ) -> Result<(), MirLoweringError> {
        let span = Span::default();
        for ty in &requests.layouts {
            self.check()?;
            self.layout_roots.push((raise_type(ty), span));
        }
        for request in requests.functions.iter().chain(&requests.constructions) {
            self.check()?;
            if self.module_handle(request.declaration) != self.owner_handle() {
                return Err(MirLoweringError::MissingBinding("requested instance owner"));
            }
            let arguments = request.arguments.iter().map(raise_type).collect::<Vec<_>>();
            if self
                .catalog
                .implementation_signature(&request.declaration)
                .is_some()
            {
                self.record_interface(&request.declaration, &arguments, span)?;
            } else if !self.prepare_native_target(&request.declaration, &arguments, span)? {
                self.enqueue_interface_method(&request.declaration, &arguments, span)?;
            }
        }
        for application in &requests.shared {
            self.check()?;
            let body = &application.body;
            if self.module_handle(body.declaration) != self.owner_handle() {
                return Err(MirLoweringError::MissingBinding(
                    "requested shared body owner",
                ));
            }
            // Identity arguments specialize the receiver. Entry arguments describe
            // binders erased from that identity; request their canonical symbols,
            // never their concrete values, so the body stays shared.
            let arguments = body
                .arguments
                .iter()
                .map(raise_type)
                .chain((0..application.arguments.len()).map(|position| {
                    raise_type(
                        &GenericParam {
                            owner: body.declaration,
                            position,
                        }
                        .as_type(),
                    )
                }))
                .collect::<Vec<_>>();
            if !self.prepare_native_target(&body.declaration, &arguments, span)? {
                self.enqueue_declaration(&body.declaration, arguments, span)?;
            }
        }
        for request in &requests.interfaces {
            self.request_interface(request, span)?;
        }
        Ok(())
    }

    fn request_interface(
        &mut self,
        request: &InterfaceRequest,
        span: Span,
    ) -> Result<(), MirLoweringError> {
        self.check()?;
        let invalid = || MirLoweringError::MissingBinding("retained interface method");
        let receiver = request.receiver.as_ref().ok_or_else(invalid)?;
        let receiver = raise_type(receiver);
        let interface = raise_nominal_type(&request.interface);
        let contract = self
            .catalog
            .trait_(&interface.declaration)
            .ok_or_else(invalid)?;
        let method = contract
            .methods
            .get(request.method_slot as usize)
            .ok_or_else(invalid)?;
        if method.generic_params.len() != interface.arguments.len() + request.arguments.len() {
            return Err(invalid());
        }
        let member = method.id;
        let definitions = self.definitions();
        let limits = ImplementationSearchLimits {
            max_checks: 100_000,
            max_depth: 64,
        };
        if let Some((declaration, arguments)) = self.catalog.concrete_interface_implementation(
            &definitions,
            &interface,
            &receiver,
            &Default::default(),
            limits,
            &self.options.cancel,
        )? {
            if self.module_handle(declaration) != self.owner_handle() {
                return Err(invalid());
            }
            self.record_interface(&declaration, &arguments, span)?;
            let (method, mut arguments) = self
                .catalog
                .implementation_method(
                    &definitions,
                    &member,
                    &interface,
                    &receiver,
                    ImplementationSearchContext {
                        assumptions: &Default::default(),
                        limits,
                        cancel: &self.options.cancel,
                    },
                )?
                .ok_or_else(invalid)?;
            arguments.extend(request.arguments.iter().map(raise_type));
            if !self.prepare_native_target(&method, &arguments, span)? {
                // Receiver methods already have their canonical shared entry from
                // record_interface. Static methods need their requested application.
                let is_static = self.catalog.trait_method(&member).is_some_and(|method| {
                    method
                        .params
                        .first()
                        .is_none_or(|parameter| parameter.name != "self")
                });
                if is_static {
                    self.enqueue_declaration(&method, arguments, span)?;
                }
            }
        } else if self.catalog.intrinsic_implementation(
            &definitions,
            &interface,
            &receiver,
            &Default::default(),
        ) {
            self.intrinsic_interface_bridge(&receiver, &interface, span)?;
        } else {
            return Err(invalid());
        }
        self.layout_roots.push((receiver, span));
        Ok(())
    }
}
