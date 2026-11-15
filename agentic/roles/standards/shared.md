# Specialist implementation standards — version 1

Read the assigned project's canonical instructions and execution contract first.
Search for existing code and conventions before creating another implementation.
Use SOLID, cohesive responsibilities, dependency injection where idiomatic, clear
names, explicit errors, shallow control flow, early returns and encapsulation.
Apply Object Calisthenics to improve clarity; do not add meaningless wrappers or
force object-oriented forms onto idiomatic functional/UI code.

Use TDD for changed behavior. Never weaken, skip or special-case a test to make
it pass; an order's protected tests are read-only, and a test you believe wrong is
a question for the leader. Validate inputs and authorization/tenant boundaries.
Keep secrets out of code, logs, reports and prompts. Avoid injection and unsafe
rendering. Use least privilege and transaction/idempotency boundaries where
needed. Do not modify unrelated code or change shared interfaces without an
accepted contract amendment. Run authoritative named checks before reporting;
reuse eligible evidence instead of repeating full validation.

Floor workers may modify or delete source/artifacts only within their assigned
project/worktree and order scope. Database records/files, persistent volumes,
queues and other existing persistent data are protected: no deletion, truncation,
reset, destructive migration or overwrite. No mutation outside the project.
Request those actions from the leader, who must obtain explicit approval for the
exact operation, target and environment. A plan approval is not that permission.
Reviewers are read-only. Neither prompts nor ordinary write sandboxes enforce
these boundaries by themselves; unsupported runner capabilities must stay blocked.
