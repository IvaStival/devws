# A submitted review tells the leader

Status: built and installed (2026-11-17, approved by the user); live check done (r325 submitted 08:47:19, leader woken 08:47:21)

## Context
On `feat-system-observer`, code-reviewer-6 submitted review r312 (B1-2) at
08:19:33. The leader had ended its turn at 08:18, waiting for that review,
and it stayed idle until the user asked "you are waiting for what?". It acted
at 08:23.

Cause: `reviews::submit` (`agentic/src/reviews.rs`) records the verdict and
logs "review r312 submitted", but it never puts a message in the leader's
inbox. The other ways a worker finishes already do:
- a report (`orders.rs`, `tx.state.say(LEADER, …)`);
- a plan proposal and a planner question (`worker_tools.rs`);
- an answer (`questions.rs`);
- a failed candidate check (`reviews.rs`, INTEGRATION FAILED).

The dispatcher delivers inbox messages to an idle leader, so a message here
starts the leader's next turn.

## Change
In `reviews::submit`, inside the transaction that stores the verdict:
- Look up the review's order to get its step.
- Call `tx.state.say(LEADER, …)` with a message from a small pure function
  `fn submitted_message(review_id, reviewer, step, verdict, findings, summary) -> String`.
  It gives:
  - the reviewer, the review and the step;
  - the verdict and how many blocking and other findings;
  - the summary (cut to about 600 characters; the full text is in
    `application_workflow status` section `reviews`);
  - what to do next:
    - approved: `accept_step <step>`;
    - changes requested or blocked: order the correction, or answer the
      reviewer, then request a fresh review.
- Pass the step to `tx.log`, so the log line names B1-2 as well.

## Tests (TDD)
- A unit test of `submitted_message`:
  - approved → names `accept_step B1-2`;
  - changes requested with one blocking finding → names the correction and
    the fresh review;
  - a long summary is cut.
- In the Docker reviewed-flow test (`runner.rs`
  `reviewed_source_checks_independent_review_merge_and_milestone`), after the
  approved submit, the leader's queue holds a message naming the review and
  `accept_step`.
- `cargo test`, the Docker test with `DEVAGENTICS_TEST_IMAGE`,
  `cargo clippy --all-targets`, then `cargo install --path agentic --locked`.
  The running leader's MCP relay picks up the new binary.

## Live check
The next `review_submit` on `feat-system-observer` shows up in the leader's
inbox, and the leader acts without being asked.

Saved as `specs/plan/2026-11-17-*_review-tells-the-leader/plan.md` with its
status. The autonomous-leader plan (ask only about the project's edges and
the plan) stays aside until the user brings it back.
