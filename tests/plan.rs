//! Plans: what `Plan::new` accepts, refuses and reads back.

use rungs::{MAX_STEPS, Need, Outcome, Plan, PlanError, Step, Wants};

fn audit() -> Plan {
    Plan::new([
        Step::new(1),
        Step::new(1).after([Need::outcome(0, [Outcome(0)])]),
        Step::new(2)
            .after([Need::outcome(0, [Outcome(0)])])
            .attempts(2),
        Step::new(1).after_any([
            Need::outcome(2, [Outcome(0)]),
            Need::outcome(1, [Outcome::FAILED]),
        ]),
    ])
    .unwrap()
}

#[test]
fn a_plan_reads_back_as_written() {
    let plan = audit();

    assert_eq!(plan.steps(), 4);
    assert_eq!(plan.outcomes(2), Some(2));
    assert_eq!(plan.attempts(2), Some(2));
    assert_eq!(plan.attempts(3), Some(1));
    assert_eq!(plan.outcomes(4), None);
    assert_eq!(plan.attempts(4), None);
    assert!(plan.after(4).is_none());
    assert!(plan.after_any(4).is_none());

    let after: Vec<Need> = plan.after(1).unwrap().collect();
    assert_eq!(after, [Need::outcome(0, [Outcome(0)])]);
    assert_eq!(plan.after(3).unwrap().count(), 0);

    let any: Vec<Need> = plan.after_any(3).unwrap().collect();
    assert_eq!(any.len(), 2);
    assert_eq!(any[0].step(), 1);
    assert_eq!(any[1], Need::outcome(2, [Outcome(0)]));

    let Wants::Outcome(set) = any[0].wants() else {
        panic!("an outcome need");
    };
    assert_eq!(set.iter().collect::<Vec<_>>(), [Outcome::FAILED]);
}

#[test]
fn depth_is_the_longest_chain_of_needs() {
    assert_eq!(audit().depth(), 3);
    assert_eq!(Plan::new([Step::new(1), Step::new(1)]).unwrap().depth(), 1);

    let chain = Plan::new([
        Step::new(1),
        Step::new(1).after([Need::settled(0)]),
        Step::new(1).after_any([Need::settled(1)]),
        Step::new(1).after([Need::settled(0)]),
    ])
    .unwrap();
    assert_eq!(chain.depth(), 3);

    let full = Plan::new(
        (0..MAX_STEPS).map(|index| Step::new(1).after(index.checked_sub(1).map(Need::settled))),
    )
    .unwrap();
    assert_eq!(full.depth(), MAX_STEPS);
}

fn refused<const N: usize>(steps: [Step; N]) -> PlanError {
    Plan::new(steps).unwrap_err()
}

#[test]
fn a_plan_with_too_few_or_too_many_steps_is_refused() {
    assert_eq!(refused([]), PlanError::NoSteps);
    assert_eq!(
        refused([Step::new(1); MAX_STEPS + 1]),
        PlanError::TooManySteps
    );
}

#[test]
fn a_step_with_no_outcomes_too_many_or_no_attempts_is_refused() {
    let outcomes = |outcomes| PlanError::Outcomes { step: 0, outcomes };

    assert_eq!(refused([Step::new(0)]), outcomes(0));
    assert_eq!(refused([Step::new(127)]), outcomes(127));
    assert_eq!(
        refused([Step::new(1).attempts(0)]),
        PlanError::NoAttempts { step: 0 }
    );
}

#[test]
fn a_need_on_the_step_itself_or_a_later_one_is_refused() {
    assert_eq!(
        refused([Step::new(1).after([Need::settled(0)])]),
        PlanError::NotEarlier { step: 0, need: 0 }
    );
    assert_eq!(
        refused([Step::new(1), Step::new(1).after_any([Need::settled(2)])]),
        PlanError::NotEarlier { step: 1, need: 2 }
    );
    assert_eq!(
        refused([
            Step::new(1),
            Step::new(1)
                .after([Need::settled(2)])
                .after_any([Need::settled(2)]),
            Step::new(1),
        ]),
        PlanError::NotEarlier { step: 1, need: 2 }
    );
    assert_eq!(
        refused([
            Step::new(1),
            Step::new(1).after([Need::settled(usize::MAX)])
        ]),
        PlanError::NotEarlier {
            step: 1,
            need: usize::MAX
        }
    );
}

#[test]
fn a_need_on_no_outcome_or_one_the_step_lacks_is_refused() {
    let no_such = |outcome| PlanError::NoSuchOutcome {
        step: 1,
        need: 0,
        outcome,
    };

    assert_eq!(
        refused([Step::new(1), Step::new(1).after([Need::outcome(0, [])])]),
        PlanError::NoOutcomes { step: 1, need: 0 }
    );
    assert_eq!(
        refused([
            Step::new(1),
            Step::new(1).after([Need::outcome(0, [Outcome(1)])])
        ]),
        no_such(Outcome(1))
    );
    assert_eq!(
        refused([
            Step::new(126),
            Step::new(1).after_any([Need::outcome(0, [Outcome(126)])]),
        ]),
        no_such(Outcome(126))
    );
    assert_eq!(
        refused([
            Step::new(1),
            Step::new(1).after([Need::outcome(0, [Outcome(0), Outcome(200)])]),
        ]),
        no_such(Outcome(200))
    );
}

#[test]
fn needs_on_one_step_read_back_as_one() {
    let plan = Plan::new([
        Step::new(126),
        Step::new(1)
            .after([
                Need::outcome(0, [Outcome(0), Outcome(125), Outcome::FAILED]),
                Need::outcome(0, [Outcome(125), Outcome::FAILED]),
                Need::settled(0),
            ])
            .after_any([Need::settled(0); 20])
            .attempts(u8::MAX),
        Step::new(1).after_any([Need::outcome(0, [Outcome(1)]), Need::settled(0)]),
    ])
    .unwrap();

    let after: Vec<Need> = plan.after(1).unwrap().collect();
    assert_eq!(after, [Need::outcome(0, [Outcome(125), Outcome::FAILED])]);

    let any: Vec<Need> = plan.after_any(1).unwrap().collect();
    assert_eq!(any, [Need::settled(0)]);

    let either: Vec<Need> = plan.after_any(2).unwrap().collect();
    assert_eq!(either, [Need::settled(0)]);
    assert_eq!(plan.attempts(1), Some(u8::MAX));
}

#[test]
fn refusals_read_as_sentences() {
    let messages = [
        (PlanError::NoSteps, "a plan needs at least one step"),
        (PlanError::TooManySteps, "a plan has at most 16 steps"),
        (
            PlanError::Outcomes {
                step: 2,
                outcomes: 0,
            },
            "step 2 has 0 outcomes; a step has 1 to 126",
        ),
        (PlanError::NoAttempts { step: 1 }, "step 1 has no attempts"),
        (
            PlanError::NotEarlier { step: 1, need: 3 },
            "step 1 needs step 3, which is not earlier",
        ),
        (
            PlanError::NoOutcomes { step: 1, need: 0 },
            "step 1 needs an empty set of step 0's outcomes",
        ),
        (
            PlanError::NoSuchOutcome {
                step: 1,
                need: 0,
                outcome: Outcome(4),
            },
            "step 1 needs outcome 4 of step 0, which has no such outcome",
        ),
    ];

    for (error, message) in messages {
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn an_outcome_set_holds_only_what_it_was_given() {
    let need = Need::outcome(0, [Outcome(0), Outcome(31), Outcome(32), Outcome::FAILED]);

    let Wants::Outcome(set) = need.wants() else {
        panic!("an outcome need");
    };

    assert_eq!(
        set.iter().collect::<Vec<_>>(),
        [Outcome(0), Outcome(31), Outcome(32), Outcome::FAILED]
    );
    assert!(!set.contains(Outcome(1)));
    assert!(!set.contains(Outcome(200)));
    assert!(!set.is_empty());
    assert_eq!(
        format!("{set:?}"),
        "{Outcome(0), Outcome(31), Outcome(32), Outcome(127)}"
    );
}
