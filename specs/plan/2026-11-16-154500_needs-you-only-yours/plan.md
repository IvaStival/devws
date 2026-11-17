# NEEDS YOU lists only what the user must act on

Status: built and installed (2026-11-16, approved by the user); live check done

## Why

On 2026-11-16 the board's NEEDS YOU listed 11 rows: one permission and ten
"leader hasn't reviewed …" steps. Only the permission was the user's. The
steps were the leader's backlog. Meanwhile the leader's own four questions to
the user weren't listed at all. The user decided: NEEDS YOU holds only what
they must act on, and the rest goes in another list.

## Changes
1. **NEEDS YOU** keeps: a dispatcher problem, the leader's open question (new),
   agents' questions, permissions, failed (exhausted) steps, and agents waiting
   or gone.
2. **The leader's open question:** while `leader_wait` is not `No`, the
   board shows the leader's latest escalation (from `log.jsonl`, excluding the
   "Permission pN requested" lines, which have their own rows) as the row
   `leader-asks`: `● leader asks: …`. Enter shows the whole text, and **m**
   answers it (About::General). Answering clears `leader_wait`, so the row goes away.
3. **LEADER BACKLOG card:** the steps the leader left (the former
   "leader hasn't reviewed" rows) sit in their own card after LEADER
   REVIEWING, under the CTO card. They keep their mark, id, title and wait
   time, and **m** still works on them.
4. The header's "N need you" counts the same rows as NEEDS YOU: the stale
   reviews are out, and the leader's question and failed steps are in.

## Tests first
- Stale reviews aren't under NEEDS YOU. They're in LEADER BACKLOG, after the CTO card.
- A leader question shows as the first NEEDS YOU row after a dispatcher
  problem, and `message_about("leader-asks")` is General.
- The latest question comes from the log, skipping permission lines. With no
  wait, there's no question.
- The header count matches the NEEDS YOU rows.

## Verification
`cargo test`, `cargo clippy --all-targets`, install, and restart the
feat-system-observer board. Live: NEEDS YOU shows p148 and the leader's
question, and the ten steps sit in LEADER BACKLOG.
