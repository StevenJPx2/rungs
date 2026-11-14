//! atelier's `build` and `security-audit` pipelines, and Chauffeur's slice 3
//! escalation and `after` cases, driven through rungs.

use rungs::{Failed, Need, Outcome, Plan, RunError, State, Status, Step};

const PASSED: Outcome = Outcome(0);
const FAILED: Outcome = Outcome::FAILED;

const OVER: State = State::Over { unhandled: None };

mod security_audit {
    use super::*;

    const WORKTREE: usize = 0;
    const DEPS: usize = 1;
    const AUDIT: usize = 2;
    const FIX: usize = 3;

    const FOUND: Outcome = Outcome(0);
    const CLEAN: Outcome = Outcome(1);

    fn plan() -> Plan {
        Plan::new([
            Step::new(1),
            Step::new(1).after([Need::outcome(WORKTREE, [PASSED])]),
            Step::new(2).after([Need::outcome(WORKTREE, [PASSED])]),
            Step::new(1).after_any([Need::outcome(AUDIT, [FOUND]), Need::outcome(DEPS, [FAILED])]),
        ])
        .unwrap()
    }

    #[test]
    fn a_finding_starts_the_fix() {
        let plan = plan();
        let mut run = plan.start();

        assert_eq!(run.advance(), [WORKTREE]);
        run.settle(WORKTREE, PASSED).unwrap();
        assert_eq!(run.advance(), [DEPS, AUDIT]);

        run.settle(AUDIT, FOUND).unwrap();
        assert_eq!(run.advance(), []);
        assert_eq!(run.state(), State::Running);

        run.settle(DEPS, PASSED).unwrap();
        assert_eq!(run.advance(), [FIX]);

        run.settle(FIX, Outcome(0)).unwrap();
        assert_eq!(run.advance(), []);
        assert_eq!(run.state(), OVER);
    }

    #[test]
    fn both_clean_skip_the_fix() {
        let plan = plan();
        let mut run = plan.start();

        let _ = run.advance();
        run.settle(WORKTREE, PASSED).unwrap();
        let _ = run.advance();
        run.settle(DEPS, PASSED).unwrap();
        run.settle(AUDIT, CLEAN).unwrap();

        assert_eq!(run.state(), OVER);
        assert_eq!(run.advance(), []);
        assert_eq!(run.status(FIX), Some(Status::Skipped));
    }

    #[test]
    fn a_failed_dependency_check_starts_the_fix() {
        let plan = plan();
        let mut run = plan.start();

        let _ = run.advance();
        run.settle(WORKTREE, PASSED).unwrap();
        let _ = run.advance();

        assert_eq!(run.fail(DEPS), Ok(Failed::Final));
        run.settle(AUDIT, CLEAN).unwrap();
        assert_eq!(run.advance(), [FIX]);

        run.settle(FIX, Outcome(0)).unwrap();
        assert_eq!(run.state(), OVER);
    }

    #[test]
    fn a_failed_worktree_skips_everything_and_blocks() {
        let plan = plan();
        let mut run = plan.start();

        let _ = run.advance();
        assert_eq!(run.fail(WORKTREE), Ok(Failed::Final));

        assert_eq!(run.advance(), []);
        assert_eq!(
            run.state(),
            State::Over {
                unhandled: Some(WORKTREE)
            }
        );
        assert_eq!(run.status(FIX), Some(Status::Skipped));
    }
}

mod build {
    use super::*;

    const WORKTREE: usize = 0;
    const BUILDER: usize = 1;
    const CHECKS: usize = 2;
    const READY: usize = 3;
    const MERGE: usize = 4;

    const OPENED: Outcome = Outcome(0);
    const NOTHING: Outcome = Outcome(1);

    fn plan() -> Plan {
        Plan::new([
            Step::new(1),
            Step::new(2).after([Need::outcome(WORKTREE, [PASSED])]),
            Step::new(1)
                .after([Need::outcome(BUILDER, [OPENED])])
                .attempts(5),
            Step::new(1).after([Need::outcome(CHECKS, [PASSED])]),
            Step::new(1).after([Need::outcome(READY, [PASSED])]),
        ])
        .unwrap()
    }

    fn opened(plan: &Plan) -> rungs::Run<'_> {
        let mut run = plan.start();

        let _ = run.advance();
        run.settle(WORKTREE, PASSED).unwrap();
        assert_eq!(run.advance(), [BUILDER]);
        run.settle(BUILDER, OPENED).unwrap();
        assert_eq!(run.advance(), [CHECKS]);

        run
    }

    #[test]
    fn an_issue_goes_from_worktree_to_merge() {
        let plan = plan();
        let mut run = opened(&plan);

        assert_eq!(run.fail(CHECKS), Ok(Failed::Retry(1)));
        assert_eq!(run.status(CHECKS), Some(Status::Running));
        run.settle(CHECKS, PASSED).unwrap();

        assert_eq!(run.advance(), [READY]);
        run.settle(READY, PASSED).unwrap();
        assert_eq!(run.advance(), [MERGE]);
        run.settle(MERGE, Outcome(0)).unwrap();

        assert_eq!(run.state(), OVER);
    }

    #[test]
    fn a_push_restarts_the_checks_and_keeps_their_failures() {
        let plan = plan();
        let mut run = opened(&plan);

        for failed in 1..=3 {
            assert_eq!(run.fail(CHECKS), Ok(Failed::Retry(failed)));
        }

        run.settle(CHECKS, PASSED).unwrap();
        assert_eq!(run.advance(), [READY]);

        let moved = run.restart(CHECKS).unwrap();

        assert_eq!(moved, [CHECKS, READY]);
        assert_eq!(run.status(MERGE), Some(Status::Pending));
        assert_eq!(run.status(BUILDER), Some(Status::Settled(OPENED)));
        assert_eq!(run.progress().failed[CHECKS], 3);

        assert_eq!(run.advance(), [CHECKS]);
        assert_eq!(run.fail(CHECKS), Ok(Failed::Retry(4)));
        assert_eq!(run.fail(CHECKS), Ok(Failed::Final));
    }

    #[test]
    fn nothing_to_build_ends_the_run() {
        let plan = plan();
        let mut run = plan.start();

        let _ = run.advance();
        run.settle(WORKTREE, PASSED).unwrap();
        let _ = run.advance();
        run.settle(BUILDER, NOTHING).unwrap();

        assert_eq!(run.advance(), []);
        assert_eq!(run.state(), OVER);

        for step in [CHECKS, READY, MERGE] {
            assert_eq!(run.status(step), Some(Status::Skipped));
        }
    }

    #[test]
    fn five_red_rounds_block_the_run() {
        let plan = plan();
        let mut run = opened(&plan);

        for failed in 1..=4 {
            assert_eq!(run.fail(CHECKS), Ok(Failed::Retry(failed)));
        }

        assert_eq!(run.fail(CHECKS), Ok(Failed::Final));
        assert_eq!(run.status(CHECKS), Some(Status::Settled(FAILED)));
        assert_eq!(
            run.state(),
            State::Over {
                unhandled: Some(CHECKS)
            }
        );
        assert_eq!(
            run.fail(CHECKS),
            Err(RunError::WrongStatus {
                step: CHECKS,
                status: Status::Settled(FAILED)
            })
        );
    }
}

mod escalation {
    use super::*;

    const FAST: usize = 0;
    const SURE: usize = 1;
    const REVIEW: usize = 2;

    const DECIDED: Outcome = Outcome(0);
    const UNSURE: Outcome = Outcome(1);

    fn plan() -> Plan {
        Plan::new([
            Step::new(2),
            Step::new(2).after([Need::outcome(FAST, [UNSURE, FAILED])]),
            Step::new(2).after([Need::outcome(SURE, [UNSURE, FAILED])]),
        ])
        .unwrap()
    }

    #[test]
    fn a_sure_answer_ends_at_the_first_step() {
        let plan = plan();
        let mut run = plan.start();

        assert_eq!(run.advance(), [FAST]);
        run.settle(FAST, DECIDED).unwrap();

        assert_eq!(run.advance(), []);
        assert_eq!(run.status(SURE), Some(Status::Skipped));
        assert_eq!(run.status(REVIEW), Some(Status::Skipped));
        assert_eq!(run.state(), OVER);
    }

    #[test]
    fn unsure_escalates_once_per_step() {
        let plan = plan();
        let mut run = plan.start();

        let _ = run.advance();
        run.settle(FAST, UNSURE).unwrap();
        assert_eq!(run.advance(), [SURE]);
        run.settle(SURE, DECIDED).unwrap();

        assert_eq!(run.advance(), []);
        assert_eq!(run.state(), OVER);
    }

    #[test]
    fn a_failure_escalates_and_is_handled() {
        let plan = plan();
        let mut run = plan.start();

        let _ = run.advance();
        assert_eq!(run.fail(FAST), Ok(Failed::Final));
        assert_eq!(run.advance(), [SURE]);
        assert_eq!(run.fail(SURE), Ok(Failed::Final));
        assert_eq!(run.advance(), [REVIEW]);
        run.settle(REVIEW, UNSURE).unwrap();

        assert_eq!(run.state(), OVER);
    }

    #[test]
    fn the_last_failure_is_unhandled() {
        let plan = plan();
        let mut run = plan.start();

        let _ = run.advance();
        run.settle(FAST, UNSURE).unwrap();
        let _ = run.advance();
        run.settle(SURE, UNSURE).unwrap();
        let _ = run.advance();
        assert_eq!(run.fail(REVIEW), Ok(Failed::Final));

        assert_eq!(
            run.state(),
            State::Over {
                unhandled: Some(REVIEW)
            }
        );
    }

    #[test]
    fn the_deadline_skips_every_pending_step() {
        let plan = plan();
        let mut run = plan.start();

        let _ = run.advance();
        run.settle(FAST, UNSURE).unwrap();

        // Less than 500 ms is left: no escalation starts.
        for step in 0..plan.steps() {
            if run.status(step) == Some(Status::Pending) {
                run.skip(step).unwrap();
            }
        }

        assert_eq!(run.advance(), []);
        assert_eq!(run.state(), OVER);
        assert_eq!(run.status(SURE), Some(Status::Skipped));
        assert_eq!(run.status(REVIEW), Some(Status::Skipped));
    }
}

mod after {
    use super::*;

    #[test]
    fn a_settled_need_waits_for_a_skipped_step_and_runs() {
        let plan = Plan::new([
            Step::new(2),
            Step::new(1).after([Need::outcome(0, [Outcome(0)])]),
            Step::new(1).after([Need::settled(1)]),
        ])
        .unwrap();
        let mut run = plan.start();

        assert_eq!(run.advance(), [0]);
        run.settle(0, Outcome(1)).unwrap();

        assert_eq!(run.advance(), [2]);
        assert_eq!(run.status(1), Some(Status::Skipped));
    }

    #[test]
    fn an_outcome_need_never_matches_a_skipped_step() {
        let plan = Plan::new([
            Step::new(2),
            Step::new(2).after([Need::outcome(0, [Outcome(0)])]),
            Step::new(1).after([Need::outcome(1, [Outcome(0), Outcome(1), FAILED])]),
        ])
        .unwrap();
        let mut run = plan.start();

        let _ = run.advance();
        run.settle(0, Outcome(1)).unwrap();

        assert_eq!(run.advance(), []);
        assert_eq!(run.status(2), Some(Status::Skipped));
    }

    #[test]
    fn failed_matches_only_when_listed() {
        let plan = Plan::new([
            Step::new(2),
            Step::new(1).after([Need::outcome(0, [Outcome(0), Outcome(1)])]),
            Step::new(1).after([Need::outcome(0, [FAILED])]),
        ])
        .unwrap();
        let mut run = plan.start();

        let _ = run.advance();
        assert_eq!(run.fail(0), Ok(Failed::Final));

        assert_eq!(run.advance(), [2]);
        assert_eq!(run.status(1), Some(Status::Skipped));
    }

    fn both() -> Plan {
        Plan::new([
            Step::new(2),
            Step::new(2),
            Step::new(2),
            Step::new(1)
                .after([Need::outcome(0, [Outcome(0)])])
                .after_any([
                    Need::outcome(1, [Outcome(0)]),
                    Need::outcome(2, [Outcome(0)]),
                ]),
        ])
        .unwrap()
    }

    #[test]
    fn after_and_after_any_must_both_hold() {
        let cases = [
            ([0, 0, 1], true),
            ([0, 1, 0], true),
            ([0, 0, 0], true),
            ([0, 1, 1], false),
            ([1, 0, 0], false),
            ([1, 1, 1], false),
        ];

        for (outcomes, runs) in cases {
            let plan = both();
            let mut run = plan.start();

            assert_eq!(run.advance(), [0, 1, 2]);

            for (step, outcome) in outcomes.into_iter().enumerate() {
                run.settle(step, Outcome(outcome)).unwrap();
            }

            let started = run.advance();
            assert_eq!(started.contains(3), runs, "{outcomes:?}");
            assert_eq!(started.len(), usize::from(runs));
        }
    }

    #[test]
    fn a_step_waits_for_every_step_it_names() {
        let plan = both();
        let mut run = plan.start();

        let _ = run.advance();
        run.settle(0, Outcome(0)).unwrap();
        run.settle(1, Outcome(0)).unwrap();

        // Step 3's any already holds, but step 2 is still running.
        assert_eq!(run.advance(), []);
        assert_eq!(run.status(3), Some(Status::Pending));
        assert_eq!(run.state(), State::Running);
    }
}
