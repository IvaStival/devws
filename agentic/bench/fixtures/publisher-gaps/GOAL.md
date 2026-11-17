# Message gaps in the Flow queue details

This is the benchmark's fixed scope: give it to the leader unchanged on every
run.

In the **Flow** page's queue details panel (the drawer that lists the
messages waiting in a consumer's queue, next to their publish times), show how
long after the previous listed message each message was published:

- After a message's time, show `+<gap>`, the time since the previous message
  in the list, formatted with the project's existing duration formatter (for
  example `+1.5s`, `+250ms`).
- The first message shows no gap. A message without a publish time shows no
  gap, and neither does the message after it.
- Change nothing else: the Observe page's details drawer, which also has a
  queue panel, stays as it is.

The web lane only (`web/**`); no backend, docs or dependency changes.

Done means `cd web && npm run lint && npm run typecheck && npm test -- --configLoader runner`
passes, including the acceptance tests in
`web/src/components/Flow/FlowDiagram/__tests__/QueueDetails.gaps.test.tsx`,
which judge the work and must not be changed.
