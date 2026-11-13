//! Runs: each call, what it refuses, and resuming a saved progress.

#![expect(
    clippy::unwrap_used,
    reason = "test support: a plan that fails to build fails the test that uses it"
)]

use rungs::{
    Failed, MAX_STEPS, Need, Outcome, Plan, Progress, ResumeError, RunError, State, Status, Step,
    Steps,
};

fn chain() -> Plan {
    Plan::new([
        Step::new(2).attempts(2),
        Step::new(1).after([Need::outcome(0, [Outcome(0)])]),
        Step::new(1).after([Need::settled(1)]),
        Step::new(1),
    ])
    .unwrap()
}

#[test]
fn a_refused_call_changes_nothing() {
    let plan = chain();
    let mut run = plan.start();
    let fresh = run;

    let refused = [
        run.settle(0, Outcome(0)),
        run.fail(0).map(|_| ()),
        run.restart(0).map(|_| ()),
        run.settle(4, Outcome(0)),
        run.skip(4),
        run.restart(MAX_STEPS).map(|_| ()),
    ];

    assert_eq!(
        refused,
        [
            Err(RunError::WrongStatus {
                step: 0,
                status: Status::Pending
            }),
            Err(RunError::WrongStatus {
                step: 0,
                status: Status::Pending
            }),
            Err(RunError::WrongStatus {
                step: 0,
                status: Status::Pending
            }),
            Err(RunError::NoSuchStep { step: 4 }),
            Err(RunError::NoSuchStep { step: 4 }),
            Err(RunError::NoSuchStep { step: MAX_STEPS }),
        ]
    );
    assert_eq!(run, fresh);

    assert_eq!(run.advance(), [0, 3]);
    let started = run;

    let refused = [
        run.settle(0, Outcome(2)),
        run.settle(0, Outcome::FAILED),
        run.skip(0),
        run.settle(1, Outcome(0)),
    ];

    assert_eq!(
        refused,
        [
            Err(RunError::NoSuchOutcome {
                step: 0,
                outcome: Outcome(2)
            }),
            Err(RunError::NoSuchOutcome {
                step: 0,
                outcome: Outcome::FAILED
            }),
            Err(RunError::WrongStatus {
                step: 0,
                status: Status::Running
            }),
            Err(RunError::WrongStatus {
                step: 1,
                status: Status::Pending
            }),
        ]
    );
    assert_eq!(run, started);
}

#[test]
fn a_repeated_reply_is_refused() {
    let plan = chain();
    let mut run = plan.start();

    let _ = run.advance();
    run.settle(0, Outcome(1)).unwrap();

    assert_eq!(
        run.settle(0, Outcome(0)),
        Err(RunError::WrongStatus {
            step: 0,
            status: Status::Settled(Outcome(1))
        })
    );
    assert_eq!(run.status(0), Some(Status::Settled(Outcome(1))));
}

#[test]
fn a_retry_keeps_the_step_running_until_its_attempts_are_spent() {
    let plan = chain();
    let mut run = plan.start();

    let _ = run.advance();

    assert_eq!(run.fail(0), Ok(Failed::Retry(1)));
    assert_eq!(run.status(0), Some(Status::Running));
    assert_eq!(run.fail(0), Ok(Failed::Final));
    assert_eq!(run.status(0), Some(Status::Settled(Outcome::FAILED)));
    assert_eq!(run.progress().failed[0], 2);
}

#[test]
fn restart_moves_the_step_and_what_depends_on_it() {
    let plan = chain();
    let mut run = plan.start();

    let _ = run.advance();
    run.settle(0, Outcome(0)).unwrap();
    run.settle(3, Outcome(0)).unwrap();
    assert_eq!(run.advance(), [1]);

    assert_eq!(run.restart(0).unwrap(), [0, 1]);
    assert_eq!(run.status(2), Some(Status::Pending));
    assert_eq!(run.status(3), Some(Status::Settled(Outcome(0))));

    assert_eq!(run.advance(), [0]);
}

#[test]
fn restart_reaches_dependents_through_skipped_steps() {
    let plan = chain();
    let mut run = plan.start();

    let _ = run.advance();
    run.settle(0, Outcome(1)).unwrap();
    assert_eq!(run.advance(), [2]);

    assert_eq!(run.restart(0).unwrap(), [0, 1, 2]);
}

#[test]
fn skip_takes_any_pending_step() {
    let plan = chain();
    let mut run = plan.start();

    run.skip(2).unwrap();
    assert_eq!(run.status(2), Some(Status::Skipped));
    assert_eq!(
        run.skip(2),
        Err(RunError::WrongStatus {
            step: 2,
            status: Status::Skipped
        })
    );

    assert_eq!(run.advance(), [0, 3]);
}

#[test]
fn a_failure_no_started_step_waited_for_is_unhandled() {
    // fix runs only after checks failed and lint passed.
    let plan = Plan::new([
        Step::new(1),
        Step::new(2),
        Step::new(1).after([
            Need::outcome(0, [Outcome::FAILED]),
            Need::outcome(1, [Outcome(0)]),
        ]),
    ])
    .unwrap();
    let mut run = plan.start();

    let _ = run.advance();
    assert_eq!(run.fail(0), Ok(Failed::Final));
    run.settle(1, Outcome(1)).unwrap();

    assert_eq!(run.state(), State::Over { unhandled: Some(0) });

    let mut handled = plan.start();
    let _ = handled.advance();
    let _ = handled.fail(0);
    handled.settle(1, Outcome(0)).unwrap();

    assert_eq!(handled.state(), State::Running);
    assert_eq!(handled.advance(), [2]);
    assert_eq!(handled.state(), State::Running);
    handled.settle(2, Outcome(0)).unwrap();
    assert_eq!(handled.state(), State::Over { unhandled: None });
}

#[test]
fn only_a_started_step_naming_the_failure_handles_it() {
    // B waits for A's outcome 0, so it is skipped; C waits for A to finish,
    // and D for A's outcome 0 or B to finish: both run, neither names the
    // failure.
    let plan = Plan::new([
        Step::new(1),
        Step::new(1).after_any([Need::outcome(0, [Outcome(0)])]),
        Step::new(1).after([Need::settled(0)]),
        Step::new(1).after_any([Need::outcome(0, [Outcome(0)]), Need::settled(1)]),
    ])
    .unwrap();
    let mut run = plan.start();

    let _ = run.advance();
    assert_eq!(run.fail(0), Ok(Failed::Final));
    assert_eq!(run.advance(), [2, 3]);
    run.settle(2, Outcome(0)).unwrap();
    run.settle(3, Outcome(0)).unwrap();

    assert_eq!(run.state(), State::Over { unhandled: Some(0) });
}

#[test]
fn a_step_naming_the_failure_in_both_lists_handles_it() {
    let plan = Plan::new([
        Step::new(1),
        Step::new(1)
            .after([Need::outcome(0, [Outcome::FAILED])])
            .after_any([Need::outcome(0, [Outcome::FAILED])]),
    ])
    .unwrap();
    let mut run = plan.start();

    let _ = run.advance();
    assert_eq!(run.fail(0), Ok(Failed::Final));
    assert_eq!(run.advance(), [1]);
    run.settle(1, Outcome(0)).unwrap();

    assert_eq!(run.state(), State::Over { unhandled: None });
}

#[test]
fn a_step_past_the_plan_has_no_status() {
    let plan = chain();
    let run = plan.start();

    assert_eq!(run.status(3), Some(Status::Pending));
    assert_eq!(run.status(4), None);
}

#[test]
fn settled_and_skipped_steps_are_finished() {
    assert!(Status::Settled(Outcome(0)).is_finished());
    assert!(Status::Skipped.is_finished());
    assert!(!Status::Pending.is_finished());
    assert!(!Status::Running.is_finished());
}

#[test]
fn a_saved_progress_resumes() {
    let plan = chain();
    let mut run = plan.start();

    let _ = run.advance();
    let _ = run.fail(0);
    let saved = run.progress();

    let resumed = plan.resume(saved).unwrap();

    assert_eq!(resumed, run);
    assert_eq!(resumed.plan(), &plan);
}

#[test]
fn a_progress_the_plan_could_not_reach_is_refused() {
    let plan = chain();

    let with = |step: usize, status: Status, failed: u8| {
        let mut progress = Progress::default();
        progress.status[step] = status;
        progress.failed[step] = failed;
        progress
    };

    let mut ran_early = Progress::default();
    ran_early.status[0] = Status::Running;
    ran_early.status[1] = Status::Settled(Outcome(0));

    let refused = [
        (
            with(4, Status::Skipped, 0),
            ResumeError::PastLastStep { step: 4 },
        ),
        (
            with(MAX_STEPS - 1, Status::Pending, 1),
            ResumeError::PastLastStep { step: 15 },
        ),
        (
            with(3, Status::Pending, 2),
            ResumeError::TooManyFailures { step: 3 },
        ),
        (
            with(0, Status::Settled(Outcome::FAILED), 1),
            ResumeError::FailedEarly { step: 0 },
        ),
        (
            with(0, Status::Settled(Outcome(2)), 0),
            ResumeError::NoSuchOutcome {
                step: 0,
                outcome: Outcome(2),
            },
        ),
        (ran_early, ResumeError::NeedsUnmet { step: 1 }),
        (
            with(2, Status::Running, 0),
            ResumeError::NeedsUnmet { step: 2 },
        ),
    ];

    for (progress, error) in refused {
        assert_eq!(plan.resume(progress).unwrap_err(), error);
    }

    let accepted = [
        with(0, Status::Settled(Outcome::FAILED), 2),
        with(0, Status::Running, 2),
        with(2, Status::Skipped, 0),
        with(3, Status::Pending, 1),
    ];

    for progress in accepted {
        assert!(plan.resume(progress).is_ok(), "{progress:?}");
    }
}

#[test]
fn steps_iterate_lowest_first() {
    let plan = Plan::new([Step::new(1); MAX_STEPS]).unwrap();
    let mut run = plan.start();

    for index in (0..MAX_STEPS).filter(|index| ![0, 3, 15].contains(index)) {
        run.skip(index).unwrap();
    }

    let set = run.advance();

    // Bounded, so a broken iterator fails here instead of running forever.
    assert_eq!(set.iter().take(4).collect::<Vec<_>>(), [0, 3, 15]);
    assert_eq!(set.into_iter().take(4).count(), 3);
    assert_eq!(set.len(), 3);
    assert!(set.contains(15));
    assert!(!set.contains(1));
    assert!(!set.contains(16));
    assert!(!set.is_empty());
    assert_ne!(set, [0, 3]);
    assert_eq!(format!("{set:?}"), "[0, 3, 15]");
    assert!(Steps::default().is_empty());
}

#[test]
fn refusals_read_as_sentences() {
    let messages = [
        (
            RunError::NoSuchStep { step: 4 }.to_string(),
            "the plan has no step 4",
        ),
        (
            RunError::WrongStatus {
                step: 1,
                status: Status::Pending,
            }
            .to_string(),
            "step 1 is Pending",
        ),
        (
            RunError::NoSuchOutcome {
                step: 1,
                outcome: Outcome(3),
            }
            .to_string(),
            "step 1 has no outcome 3",
        ),
        (
            ResumeError::PastLastStep { step: 5 }.to_string(),
            "step 5 is past the plan's last step but has progress",
        ),
        (
            ResumeError::TooManyFailures { step: 1 }.to_string(),
            "step 1 failed more times than its attempts",
        ),
        (
            ResumeError::FailedEarly { step: 1 }.to_string(),
            "step 1 settled failed before its attempts were spent",
        ),
        (
            ResumeError::NoSuchOutcome {
                step: 1,
                outcome: Outcome(3),
            }
            .to_string(),
            "step 1 settled outcome 3, which it doesn't have",
        ),
        (
            ResumeError::NeedsUnmet { step: 1 }.to_string(),
            "step 1 started though its needs don't hold",
        ),
    ];

    for (message, expected) in messages {
        assert_eq!(message, expected);
    }
}
