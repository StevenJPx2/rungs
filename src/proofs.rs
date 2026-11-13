//! Kani harnesses: every plan of up to four steps, with up to three outcomes
//! and three attempts each, every progress it accepts and every call.
//!
//! Under Kani a plan has room for four steps and an outcome set is a byte
//! (`narrow.rs`), the sizes these plans need; the code is the same. Each
//! property has a harness for each plan size, so every loop has a fixed
//! bound.

use crate::need::{Bits, bit};
use crate::{Failed, MAX_STEPS, Need, Outcome, Plan, Progress, Run, State, Status, Step, Wants};

const STEPS: usize = 4;

type Statuses = [Status; MAX_STEPS];

/// The codes a need on a step with `outcomes` outcomes accepts, or none:
/// every code for a settled need, or a non-empty set of its outcomes and
/// `FAILED` for an outcome need.
fn any_accepts(outcomes: u8) -> Option<Bits> {
    let kind: u8 = kani::any();
    let mask: u8 = kani::any();

    let own = (mask & 0b111) & ((1 << outcomes) - 1);
    let failed = if mask & 0b1000 == 0 {
        0
    } else {
        bit(Outcome::FAILED.0).unwrap()
    };
    let set = own | failed;

    match kind {
        0 => None,
        1 => Some(Bits::MAX),
        _ => {
            kani::assume(set != 0);
            Some(set)
        }
    }
}

/// A plan of `count` steps, each with up to one need on each earlier step in
/// `after` and again in `after_any`, which is every plan of them up to the
/// merging of needs on one step.
fn any_plan(count: usize) -> Plan {
    let mut outcomes = [1_u8; STEPS];
    let mut steps = [Step::new(1); MAX_STEPS];

    for index in 0..count {
        let own: u8 = kani::any();
        let attempts: u8 = kani::any();
        kani::assume((1..=3).contains(&own) && (1..=3).contains(&attempts));
        outcomes[index] = own;

        let mut step = Step::new(own).attempts(attempts);

        for earlier in 0..index {
            for any in [false, true] {
                if let Some(accepts) = any_accepts(outcomes[earlier]) {
                    step = step.with_need(any, earlier, accepts);
                }
            }
        }

        steps[index] = step;
    }

    Plan::unchecked(steps, count)
}

/// A run of `plan` from any progress it accepts.
fn any_run(plan: &Plan) -> Run<'_> {
    let run = plan.resume(kani::any());
    kani::assume(run.is_ok());
    run.unwrap()
}

fn status(statuses: &Statuses, index: usize) -> Status {
    statuses[index]
}

fn holds(statuses: &Statuses, need: Need) -> bool {
    match (status(statuses, need.step()), need.wants()) {
        (Status::Settled(_) | Status::Skipped, Wants::Settled) => true,
        (Status::Settled(outcome), Wants::Outcome(set)) => set.contains(outcome),
        _ => false,
    }
}

/// Every step that step `index` names has finished.
fn named_finished(plan: &Plan, statuses: &Statuses, index: usize) -> bool {
    let finished = |need: Need| status(statuses, need.step()).is_finished();

    plan.after(index).unwrap().all(finished) && plan.after_any(index).unwrap().all(finished)
}

/// Every `after` need holds and, if there are `after_any` needs, one does.
fn rule(plan: &Plan, statuses: &Statuses, index: usize) -> bool {
    let all = plan.after(index).unwrap().all(|need| holds(statuses, need));
    let none = plan.after_any(index).unwrap().next().is_none();
    let any = plan
        .after_any(index)
        .unwrap()
        .any(|need| holds(statuses, need));

    all && (none || any)
}

/// Step `later` is `index` or names it, directly or through steps that do.
fn reached_by_restart(plan: &Plan, later: usize, index: usize) -> bool {
    let mut reached = [false; STEPS];

    for step in 0..plan.steps() {
        let names = plan
            .after(step)
            .unwrap()
            .chain(plan.after_any(step).unwrap())
            .any(|need| reached[need.step()]);
        reached[step] = step == index || names;
    }

    later < plan.steps() && reached[later]
}

/// What `resume` should accept, written from the plan's rules.
fn reachable(plan: &Plan, progress: &Progress) -> bool {
    (0..MAX_STEPS).all(|index| {
        let status = progress.status[index];
        let failed = progress.failed[index];

        let Some(attempts) = plan.attempts(index) else {
            return status == Status::Pending && failed == 0;
        };

        let counts = match status {
            Status::Settled(Outcome::FAILED) => failed == attempts,
            Status::Settled(outcome) => outcome.0 < plan.outcomes(index).unwrap(),
            _ => true,
        };
        let started = matches!(status, Status::Running | Status::Settled(_));
        let allowed =
            named_finished(plan, &progress.status, index) && rule(plan, &progress.status, index);

        failed <= attempts && counts && (!started || allowed)
    })
}

fn advance_starts_a_step_exactly_when_its_needs_hold(count: usize) {
    let plan = any_plan(count);
    let before = any_run(&plan);

    let mut run = before;
    let started = run.advance();
    let after = run.progress().status;

    for index in 0..count {
        let was = before.status(index).unwrap();
        let now = run.status(index).unwrap();

        if was != Status::Pending {
            assert_eq!(now, was);
            assert!(!started.contains(index));
            continue;
        }

        assert_eq!(started.contains(index), now == Status::Running);

        let decided = named_finished(&plan, &after, index);
        let rule = rule(&plan, &after, index);

        match now {
            Status::Running => assert!(decided && rule),
            Status::Skipped => assert!(decided && !rule),
            Status::Pending => assert!(!decided),
            Status::Settled(_) => panic!("advance settles nothing"),
        }
    }

    assert!(same_failed(&run, &before));

    let once = run;
    assert!(run.advance().is_empty());
    assert!(same(&run, &once));
}

fn a_run_never_stalls(count: usize) {
    let plan = any_plan(count);
    let mut run = any_run(&plan);

    let state = run.state();
    let _started = run.advance();

    let running = (0..count).any(|index| run.status(index) == Some(Status::Running));
    let pending = (0..count).any(|index| run.status(index) == Some(Status::Pending));

    assert!(running || !pending);
    assert_eq!(state == State::Running, running);
    assert_eq!(run.state() == State::Running, running);
}

/// Each step's level: one more than the deepest level among the steps it
/// names, so a step with no needs is level 1.
fn levels(plan: &Plan) -> [usize; MAX_STEPS] {
    let mut levels = [0; MAX_STEPS];

    for index in 0..plan.steps() {
        let mut deepest = 0;

        for need in plan.after(index).unwrap() {
            deepest = deepest.max(levels[need.step()]);
        }

        for need in plan.after_any(index).unwrap() {
            deepest = deepest.max(levels[need.step()]);
        }

        levels[index] = deepest + 1;
    }

    levels
}

/// Without retries or restarts, the passes go level by level: once every
/// step up to level `p` has finished and none runs, one pass of `advance`,
/// with every started step settled, finishes every step up to `p + 1`. So
/// `depth()` passes finish the run.
fn a_pass_finishes_the_next_level(count: usize) {
    let plan = any_plan(count);
    let levels = levels(&plan);
    let mut run = any_run(&plan);
    let done: usize = kani::any();
    kani::assume(done < plan.depth());

    assert_eq!(plan.depth(), levels.into_iter().max().unwrap());

    for index in 0..count {
        let status = run.status(index).unwrap();
        kani::assume(status != Status::Running);
        kani::assume(levels[index] > done || status.is_finished());
    }

    for index in run.advance() {
        let outcome: u8 = kani::any();

        if outcome == u8::MAX {
            kani::assume(plan.attempts(index) == Some(1));
            assert_eq!(run.fail(index), Ok(Failed::Final));
        } else {
            kani::assume(outcome < plan.outcomes(index).unwrap());
            assert!(run.settle(index, Outcome(outcome)).is_ok());
        }
    }

    for index in 0..count {
        if levels[index] <= done + 1 {
            assert!(run.status(index).unwrap().is_finished());
        }
    }
}

fn resume_accepts_exactly_the_reachable_progress(count: usize) {
    let plan = any_plan(count);
    let progress: Progress = kani::any();

    assert_eq!(plan.resume(progress).is_ok(), reachable(&plan, &progress));
}

fn every_call_keeps_the_progress_reachable(count: usize) {
    let plan = any_plan(count);

    assert!(reachable(&plan, &plan.start().progress()));

    let mut run = any_run(&plan);
    let call: u8 = kani::any();
    let index: usize = kani::any();
    let outcome: Outcome = kani::any();

    let _ = match call {
        0 => {
            let _started = run.advance();
            Ok(())
        }
        1 => run.settle(index, outcome),
        2 => run.fail(index).map(|_| ()),
        3 => run.skip(index),
        _ => run.restart(index).map(|_| ()),
    };

    assert!(reachable(&plan, &run.progress()));
}

fn a_call_changes_only_its_step_and_only_from_the_status_it_expects(count: usize) {
    let plan = any_plan(count);
    let before = any_run(&plan);
    let mut run = before;

    let call: u8 = kani::any();
    let index: usize = kani::any();
    let outcome: Outcome = kani::any();
    kani::assume(call <= 2 && index <= MAX_STEPS);

    let was = before.status(index);
    let failed = before.progress().failed;

    let result = match call {
        0 => run.settle(index, outcome).map(|()| None),
        1 => run.fail(index).map(Some),
        _ => run.skip(index).map(|()| None),
    };

    let Ok(failure) = result else {
        assert!(same(&run, &before));
        return;
    };

    for other in (0..MAX_STEPS).filter(|other| *other != index) {
        assert_eq!(
            run.progress().status[other],
            before.progress().status[other]
        );
        assert_eq!(run.progress().failed[other], failed[other]);
    }

    let now = run.status(index).unwrap();
    let spent = run.progress().failed[index];
    let attempts = plan.attempts(index).unwrap();

    match (call, failure) {
        (0, _) => {
            assert_eq!(was, Some(Status::Running));
            assert_eq!(now, Status::Settled(outcome));
            assert!(outcome.0 < plan.outcomes(index).unwrap());
            assert_eq!(spent, failed[index]);
        }
        (1, Some(Failed::Retry(so_far))) => {
            assert_eq!(was, Some(Status::Running));
            assert_eq!(now, Status::Running);
            assert!(spent == so_far && spent == failed[index] + 1 && spent < attempts);
        }
        (1, _) => {
            assert_eq!(was, Some(Status::Running));
            assert_eq!(now, Status::Settled(Outcome::FAILED));
            assert_eq!(spent, attempts);
        }
        _ => {
            assert_eq!(was, Some(Status::Pending));
            assert_eq!(now, Status::Skipped);
            assert_eq!(spent, failed[index]);
        }
    }
}

fn restart_moves_only_the_step_and_its_dependents_back(count: usize) {
    let plan = any_plan(count);
    let before = any_run(&plan);
    let mut run = before;
    let index: usize = kani::any();
    kani::assume(index <= MAX_STEPS);

    let Ok(moved) = run.restart(index) else {
        assert!(same(&run, &before));
        assert!(index >= count || before.status(index) == Some(Status::Pending));
        return;
    };

    assert_ne!(before.status(index), Some(Status::Pending));
    assert!(moved.contains(index));
    assert!(same_failed(&run, &before));

    for step in 0..MAX_STEPS {
        let was = before.progress().status[step];
        let now = run.progress().status[step];
        let reached = reached_by_restart(&plan, step, index);

        assert_eq!(moved.contains(step), was != now);

        if reached {
            assert_eq!(now, Status::Pending);
        } else {
            assert_eq!(now, was);
        }
    }
}

/// Two runs' progress is the same, compared step by step: a whole-array
/// comparison is a `memcmp`, whose loop outruns the unwinding bound.
fn same(a: &Run<'_>, b: &Run<'_>) -> bool {
    let (a, b) = (a.progress(), b.progress());

    (0..MAX_STEPS).all(|index| a.status[index] == b.status[index]) && same_failed_in(&a, &b)
}

fn same_failed(a: &Run<'_>, b: &Run<'_>) -> bool {
    same_failed_in(&a.progress(), &b.progress())
}

fn same_failed_in(a: &Progress, b: &Progress) -> bool {
    (0..MAX_STEPS).all(|index| a.failed[index] == b.failed[index])
}

/// The starts step `index` has left before a restart: from Pending, its
/// first start and its retries; when running, its retries; none once it
/// finished.
fn budget(run: &Run<'_>, index: usize) -> u8 {
    let attempts = run.plan().attempts(index).unwrap();
    let failed = run.progress().failed[index];

    match run.status(index).unwrap() {
        Status::Pending => attempts.saturating_sub(failed).max(1),
        Status::Running => attempts.saturating_sub(failed).saturating_sub(1),
        Status::Settled(_) | Status::Skipped => 0,
    }
}

/// Every call but `restart` pays for each start it makes, from `advance`
/// or a retry, out of the step's budget, and a budget is at most the step's
/// attempts. So between two restarts a step starts at most `attempts`
/// times, and a run at most the sum of them.
fn a_start_spends_its_steps_budget(count: usize) {
    let plan = any_plan(count);
    let before = any_run(&plan);
    let mut run = before;

    let call: u8 = kani::any();
    let index: usize = kani::any();
    let outcome: Outcome = kani::any();
    kani::assume(index < count);

    let mut starts = [0_u8; MAX_STEPS];

    match call {
        0 => {
            for started in run.advance() {
                starts[started] += 1;
            }
        }
        1 => {
            let _ = run.settle(index, outcome);
        }
        2 => {
            if let Ok(Failed::Retry(_)) = run.fail(index) {
                starts[index] += 1;
            }
        }
        _ => {
            let _ = run.skip(index);
        }
    }

    for step in 0..count {
        assert!(budget(&before, step) <= plan.attempts(step).unwrap());
        assert!(starts[step] + budget(&run, step) <= budget(&before, step));
    }
}

/// One harness for each plan size, so each one's loops have fixed bounds.
macro_rules! for_each_size {
    ($($property:ident: $one:ident $two:ident $three:ident $four:ident;)*) => {$(
        #[kani::proof]
        #[kani::unwind(6)]
        fn $one() {
            $property(1);
        }

        #[kani::proof]
        #[kani::unwind(6)]
        fn $two() {
            $property(2);
        }

        #[kani::proof]
        #[kani::unwind(6)]
        fn $three() {
            $property(3);
        }

        #[kani::proof]
        #[kani::unwind(6)]
        fn $four() {
            $property(4);
        }
    )*};
}

for_each_size! {
    advance_starts_a_step_exactly_when_its_needs_hold: advance_starts_a_step_exactly_when_its_needs_hold_1 advance_starts_a_step_exactly_when_its_needs_hold_2 advance_starts_a_step_exactly_when_its_needs_hold_3 advance_starts_a_step_exactly_when_its_needs_hold_4;
    a_run_never_stalls: a_run_never_stalls_1 a_run_never_stalls_2 a_run_never_stalls_3 a_run_never_stalls_4;
    a_pass_finishes_the_next_level: a_pass_finishes_the_next_level_1 a_pass_finishes_the_next_level_2 a_pass_finishes_the_next_level_3 a_pass_finishes_the_next_level_4;
    a_start_spends_its_steps_budget: a_start_spends_its_steps_budget_1 a_start_spends_its_steps_budget_2 a_start_spends_its_steps_budget_3 a_start_spends_its_steps_budget_4;
    resume_accepts_exactly_the_reachable_progress: resume_accepts_exactly_the_reachable_progress_1 resume_accepts_exactly_the_reachable_progress_2 resume_accepts_exactly_the_reachable_progress_3 resume_accepts_exactly_the_reachable_progress_4;
    every_call_keeps_the_progress_reachable: every_call_keeps_the_progress_reachable_1 every_call_keeps_the_progress_reachable_2 every_call_keeps_the_progress_reachable_3 every_call_keeps_the_progress_reachable_4;
    a_call_changes_only_its_step_and_only_from_the_status_it_expects: a_call_changes_only_its_step_and_only_from_the_status_it_expects_1 a_call_changes_only_its_step_and_only_from_the_status_it_expects_2 a_call_changes_only_its_step_and_only_from_the_status_it_expects_3 a_call_changes_only_its_step_and_only_from_the_status_it_expects_4;
    restart_moves_only_the_step_and_its_dependents_back: restart_moves_only_the_step_and_its_dependents_back_1 restart_moves_only_the_step_and_its_dependents_back_2 restart_moves_only_the_step_and_its_dependents_back_3 restart_moves_only_the_step_and_its_dependents_back_4;
}
