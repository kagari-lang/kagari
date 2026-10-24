use super::*;
use rstest::rstest;

fn namespace(
    functions: &[(&'static str, usize)],
    modules: &[(&'static str, usize)],
) -> Namespace<'static> {
    Namespace {
        functions: functions.iter().copied().collect(),
        modules: modules.iter().copied().collect(),
        ..Default::default()
    }
}

fn contract(root: usize) -> Contract {
    Contract {
        root,
        entry: None,
        canonical: BTreeMap::new(),
    }
}

#[rstest]
#[case::implicit_to_explicit(false)]
#[case::explicit_to_implicit(true)]
fn implicit_and_explicit_empty_roots_preserve_the_same_query_start(#[case] reverse: bool) {
    let root = ModuleIdentity::single_file("root");
    let implicit = ProgramExports::default();
    let explicit = ProgramExports {
        namespaces: BTreeMap::from([(root.clone(), Default::default())]),
        entry: None,
    };
    let (previous, candidate) = if reverse {
        (&explicit, &implicit)
    } else {
        (&implicit, &explicit)
    };
    let mut comparison = Comparison::new().unwrap();
    let definitions = comparison.context.snapshot();
    let left = comparison
        .capture(&root, previous, &definitions, [].iter(), |_| None)
        .unwrap();
    let right = comparison
        .capture(&root, candidate, &definitions, [].iter(), |_| None)
        .unwrap();
    comparison.compare(left, right).unwrap();
}

#[test]
fn preservation_is_directional_and_checks_each_candidate_target() {
    let mut comparison = Comparison::new().unwrap();
    comparison.namespaces = vec![
        namespace(&[], &[("a", 1), ("b", 1)]),
        namespace(&[("old", 0)], &[("again", 1)]),
        namespace(&[], &[("a", 3), ("b", 4)]),
        namespace(&[("old", 0), ("new", 1)], &[("again", 3)]),
        namespace(&[("old", 0)], &[("again", 4)]),
    ];
    comparison.compare(contract(0), contract(2)).unwrap();
    assert!(comparison.compare(contract(2), contract(0)).is_err());

    // The same old namespace reached through two aliases creates two distinct
    // obligations. A successful match through one cannot discharge the other.
    comparison.namespaces[4].functions.insert("old", 2);
    assert!(comparison.compare(contract(0), contract(2)).is_err());
}

#[test]
fn canonical_query_starts_survive_root_alias_retargeting() {
    let identity = ModuleIdentity::single_file("api");
    let mut comparison = Comparison::new().unwrap();
    comparison.namespaces = vec![
        namespace(&[], &[("api", 1)]),
        namespace(&[("run", 0)], &[]),
        namespace(&[], &[("api", 3)]),
        namespace(&[("run", 0)], &[]),
        namespace(&[], &[]),
    ];
    let old = || Contract {
        canonical: BTreeMap::from([(identity.clone(), 1)]),
        ..contract(0)
    };
    let candidate = || Contract {
        canonical: BTreeMap::from([(identity.clone(), 4)]),
        ..contract(2)
    };
    assert!(comparison.compare(old(), contract(2)).is_err());
    assert!(comparison.compare(old(), candidate()).is_err());
    comparison.namespaces[4].functions.insert("run", 0);
    comparison.compare(old(), candidate()).unwrap();
}

#[test]
fn additional_type_leaves_do_not_permit_changed_old_leaves() {
    let mut comparison = Comparison::new().unwrap();
    comparison.namespaces = vec![Namespace::default(), Namespace::default()];
    let one = TypeExport::OpenAlias { arity: 1 };
    let two = TypeExport::OpenAlias { arity: 2 };
    comparison.namespaces[0].types.insert("Old", one.clone());
    comparison.namespaces[1].types.insert("Old", one);
    comparison.namespaces[1].types.insert("New", two.clone());
    comparison.compare(contract(0), contract(1)).unwrap();
    comparison.namespaces[1].types.insert("Old", two);
    assert!(comparison.compare(contract(0), contract(1)).is_err());
}

#[test]
fn entry_can_be_added_but_not_removed_or_changed() {
    let mut comparison = Comparison::new().unwrap();
    comparison.namespaces.push(Namespace::default());
    let entry = |signature| Contract {
        entry: Some(signature),
        ..contract(0)
    };
    comparison.compare(contract(0), entry(7)).unwrap();
    comparison.compare(entry(7), entry(7)).unwrap();
    assert!(comparison.compare(entry(7), contract(0)).is_err());
    assert!(comparison.compare(entry(7), entry(8)).is_err());
}

#[test]
fn traversal_charges_duplicate_edges_before_allocating_or_enqueueing() {
    let mut traversal = Preservation::new(3);
    traversal.enqueue(0, 1).unwrap();
    traversal.enqueue(0, 1).unwrap();
    traversal.enqueue(0, 2).unwrap();
    assert_eq!(traversal.pending, [(0, 1), (0, 2)]);
    assert_eq!(traversal.seen.len(), 2);
    assert!(matches!(
        traversal.enqueue(1, 2),
        Err(ReloadValidationError::PublicExportComparisonLimit)
    ));
    assert_eq!(traversal.pending, [(0, 1), (0, 2)]);
    assert_eq!(traversal.seen.len(), 2);
}

#[test]
fn cycle_products_terminate_without_merging_directional_obligations() {
    let mut comparison = Comparison::new().unwrap();
    // Relatively prime cycles exercise all old/candidate pairs, not merely a
    // shared representative. Every pair must preserve the old function label.
    let left = 17;
    let right = 19;
    for index in 0..left {
        comparison
            .namespaces
            .push(namespace(&[("run", 0)], &[("next", (index + 1) % left)]));
    }
    for index in 0..right {
        comparison.namespaces.push(namespace(
            &[("run", 0), ("added", 1)],
            &[("next", left + (index + 1) % right)],
        ));
    }
    comparison.compare(contract(0), contract(left)).unwrap();
    comparison.namespaces[left + right - 1]
        .functions
        .remove("run");
    assert!(comparison.compare(contract(0), contract(left)).is_err());
}
