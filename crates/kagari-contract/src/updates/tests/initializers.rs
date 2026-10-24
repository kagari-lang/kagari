use super::*;
use crate::updates::admission::validate_shape;

#[test]
fn lexical_generated_bodies_are_local_but_retained_calls_do_not_establish_ownership() {
    let f = Fixture::new();
    let mut old = f.initializer_evidence();
    let mut parent = old.bodies[0].requirement.origin.clone();
    let first_owned = old.bodies.len();
    for (index, name) in ["closure", "async", "resume", "adapter"]
        .into_iter()
        .enumerate()
    {
        let mut body = f.body(name, vec![]);
        body.classification = match index {
            0 => BodyClassification::Closure(parent),
            1 => BodyClassification::Async(parent),
            2 => BodyClassification::Resume(parent),
            3 => BodyClassification::Adapter(parent),
            _ => unreachable!(),
        };
        parent = body.requirement.origin.clone();
        old.sources[0].bodies.push(parent.clone());
        old.bodies.push(body);
    }
    let mut helper_closure = f.body("helper_closure", vec![]);
    helper_closure.classification = BodyClassification::Closure(f.identity("helper"));
    old.bodies[0]
        .requirement
        .calls
        .push(CallRequirement::RetainedBody(BodyApplication {
            body: helper_closure.requirement.origin.clone(),
            arguments: vec![],
        }));
    old.bodies.push(helper_closure);
    validate_shape(&old, &f.context.snapshot(), &mut f.budget()).unwrap();
    let mut candidate = old.clone();
    candidate.bodies.last_mut().unwrap().actual_instructions[0].opcode = 99;
    compare_initializers(&old, &candidate, &mut f.budget()).unwrap();
    let mut reordered = candidate.clone();
    reordered.sources[0].bodies.reverse();
    reordered.bodies.reverse();
    reordered.targets.reverse();
    validate_shape(&reordered, &f.context.snapshot(), &mut f.budget()).unwrap();
    compare_initializers(&old, &reordered, &mut f.budget()).unwrap();
    for index in first_owned..first_owned + 4 {
        let mut changed = candidate.clone();
        changed.bodies[index].actual_instructions[0].opcode = 99;
        assert_eq!(
            compare_initializers(&old, &changed, &mut f.budget()),
            Err(UpdateError::InitializerChange {
                declaration: old.sources[0].declaration,
            }),
        );
    }
    let mut forged = old.clone();
    // The call record is not authority to claim a callee-owned closure as initializer code.
    let foreign = forged.bodies.last().unwrap().requirement.origin.clone();
    forged.sources[0].bodies.push(foreign);
    assert_eq!(
        validate_shape(&forged, &f.context.snapshot(), &mut f.budget()),
        Err(UpdateError::InvalidEvidence),
    );
    assert_eq!(
        compare_initializers(&forged, &forged, &mut f.budget()),
        Err(UpdateError::InvalidEvidence),
    );
}

#[test]
fn static_inventory_source_and_owned_recipe_cannot_be_omitted_or_reclassified() {
    let f = Fixture::new();
    let old = f.initializer_evidence();
    validate_shape(&old, &f.context.snapshot(), &mut f.budget()).unwrap();
    for case in 0..6 {
        let mut candidate = old.clone();
        match case {
            0 => candidate.initializers.clear(),
            1 => candidate.sources.clear(),
            2 => candidate.sources[0].bodies.clear(),
            3 => {
                candidate.bodies[0].classification =
                    BodyClassification::Named(candidate.bodies[0].requirement.origin.declaration);
            }
            4 => candidate
                .initializers
                .push(candidate.initializers[0].clone()),
            5 => {
                let duplicate = candidate.sources[0].bodies[0].clone();
                candidate.sources[0].bodies.push(duplicate);
            }
            _ => unreachable!(),
        }
        assert!(validate_shape(&candidate, &f.context.snapshot(), &mut f.budget()).is_err());
        assert!(compare_initializers(&old, &candidate, &mut f.budget()).is_err());
    }
}

#[test]
fn new_statics_are_allowed_without_replacing_existing_recipes() {
    let f = Fixture::new();
    let old = f.initializer_evidence();
    let mut candidate = old.clone();
    let mut body = f.body("new_static", vec![]);
    let declaration = body.requirement.origin.declaration;
    body.classification = BodyClassification::StaticInitializer(declaration);
    candidate
        .initializers
        .push(StaticInitializer { declaration });
    candidate.sources.push(SourceRecord {
        declaration,
        generic: None,
        tokens: vec![SourceToken {
            kind: 2,
            text: "1".into(),
        }],
        inputs: vec![CanonicalOperand::Signed(1)],
        calls: vec![],
        bodies: vec![body.requirement.origin.clone()],
    });
    candidate.bodies.push(body);
    validate_shape(&candidate, &f.context.snapshot(), &mut f.budget()).unwrap();
    compare_initializers(&old, &candidate, &mut f.budget()).unwrap();
    assert!(matches!(
        compare_initializers(&candidate, &old, &mut f.budget()),
        Err(UpdateError::InitializerChange { declaration: missing }) if missing == declaration
    ));
}

#[test]
fn checked_calls_preserved_instructions_and_owned_provenance_are_expression_identity() {
    let f = Fixture::new();
    let mut old = f.initializer_evidence();
    old.sources[0]
        .calls
        .push(CallRequirement::Direct(f.identity("helper")));
    for case in 0..5 {
        let mut candidate = old.clone();
        match case {
            0 => {
                candidate.sources[0].calls[0] = CallRequirement::Direct(f.identity("handler"));
            }
            1 => {
                candidate.bodies[0].requirement.calls[0] =
                    CallRequirement::Direct(f.identity("handler"));
            }
            2 => candidate.bodies[0].instructions[0].opcode = 99,
            3 => candidate.bodies[0].format += 1,
            4 => {
                let mut closure = f.body("added_closure", vec![]);
                closure.classification = BodyClassification::Closure(f.identity("hash"));
                candidate.sources[0]
                    .bodies
                    .push(closure.requirement.origin.clone());
                candidate.bodies.push(closure);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            compare_initializers(&old, &candidate, &mut f.budget()),
            Err(UpdateError::InitializerChange {
                declaration: old.sources[0].declaration,
            }),
        );
    }
}
