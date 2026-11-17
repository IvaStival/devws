# The user's answers for a publisher-gaps run

Every run gets exactly these, so runs stay comparable.

1. **First message** to the leader, once its first turn is idle (after the
   folder-trust prompt), verbatim:

   > This is a fixed-scope task: GOAL.md at the repo root is the complete requirement. My standing answers for the interview: (1) the scope is exactly GOAL.md, nothing more; (2) one scope and one milestone, M01, version 1.1.0, with every item in it; (3) the web lane only (web/**); (4) the acceptance tests GOAL.md names already exist and must not be changed by the implementation; (5) the runner and the check profiles are already configured and verified: keep them as they are, do not reconfigure them; (6) for any other question, use your recommendation. When the baseline is ready, show it to me and I will approve that exact revision. When the milestone is validated, stop and tell me; do not open pull requests.

2. **Nudge**: the board sends `Check your inbox and continue.` by itself when
   the leader is idle with a new inbox message. Send it yourself only on a
   build older than that, exactly that text, and log it.
3. **Approval**: when the leader shows a baseline that implements exactly
   GOAL.md (one scope, milestone M01 1.1.0, web lane only, the acceptance tests
   unchanged), answer `I approve baseline revision <N>.` with its revision.
   Otherwise: `Use your recommendation, within GOAL.md.`
4. Anything else the leader asks: `Use your recommendation, within GOAL.md.`
5. Never approve pull requests, other repositories or runner changes.

Log every message sent, with the time, next to the run's report.
