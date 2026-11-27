# Design references: the user approves what the interface should look like before a designer plans it

- **Date** 27-11-2026 · **Branch** `feat/design-references` (from `main` 91f51ef)
- **Status** Approved and built (27-11-2026), with tests. Not yet tried with the live team.
- **Built differently from the plan:**
  - Follow-up orders don't get the reference files in `read`: hammers and reviewers can't open image files in their containers. What reaches them is the plan's References section (each approved direction's note, the rejected ones, and the user's comments).
  - The designer's actions are worker tools (`references_status`, `references_add`, `references_submit`), the same way reviewed workers call `plan_write`. The user's images are copied into its `/scratch/references/` when it calls `references_status`.
  - On the board, Enter on the REFERENCES line opens the menu directly. Comments are given on the page or the CLI (`--comment-on`), not in the menu.
  - A designer waiting for the user's answer is not reported as stalled.
  - After the user's first look (27-11-2026): the page is always on the board (DESIGN REFERENCES), not only while a round waits; it takes several images at once, by drag and drop or a pasted screenshot (the page's one script, served as its own file under `script-src 'self'`), and an image can be removed. The leader is told of each image and sees it with the `design_references` MCP tool (an MCP image), and a designer gets each image's path on the machine so its program can open it.
- **Source of the references (user's choice: c):** both. The user gives
  inspiration images, and the designer shows 2–3 directions built from them as
  its own mockups.

## Context

The front's design keeps going wrong in feat-system-observer. The designer
(`.devws/roles/designer.md` in publisherProvider: a Codex senior) turns an
order straight into an execution plan with exact px, colours and layout. The
user sees the result only after hammers have built it and reviewers have
passed it. By then a wrong direction has cost a plan, several orders and their
reviews.

What exists that this builds on:

| Already there | Where |
|---|---|
| A senior writes its plan with `devagentics plan write`, for the step of its open order | `plans.rs:27-80` (`store`, `store_execution`) |
| A role's options in its file's front matter (`agent`, `group`, `approvals`…) | `roles.rs:149-170` |
| A forwarder on `127.0.0.1` that you reach over `ssh -L` | `preview.rs:378-460` |
| The board's NEEDS YOU rows (the plan approval) | `board.rs` `Approval`, `board_view.rs` `approval_entry` |
| Each worker has a `scratch/` folder it can write | `workers/<agent>/scratch` |
| A long-running dispatcher for the team | `watch.rs:687` (`run`) |
| `chafa` on this machine (draws images with text characters) | `/usr/bin/chafa` |

## In plain words: what changes

1. **A designer can't write a plan before you've approved its references.**
   A role gets a new option, `references = true` (the designer will have it).
   For an agent with that role, `devagentics plan write` is refused until the
   step has an approved set of references. Nothing else about plans changes.
2. **You can give the designer your own images.** On the references page, or
   with `devagentics references give <task> <image> --note "…"`. They are kept
   in the task, and every designer's order lists them, copied into its
   `scratch/references/`, so it can read them.
3. **The designer shows you 2–3 directions.** It builds each one as a static
   HTML mockup or an image in its scratch folder, with a note: what it takes
   from your images, and what it changes. Then it runs
   `devagentics references add <file> --note "…"` for each one, then
   `devagentics references submit`, and ends its turn. Its order stays open,
   the same way it does while it waits for an answer to a question.
4. **You see them on a page on your machine.** The board shows a NEEDS YOU
   line:
   `REFERENCES G9 dashboard · 3 to review · http://127.0.0.1:13100/r/k3f9…`
   The page shows each direction at full size. An HTML mockup is shown live
   in a locked frame: no scripts from outside, no network. You approve or
   reject each one and can add a comment. You can also upload more images
   there. One button sends your answer.
5. **Your answer goes to the designer.**
   - If you approved at least one, it gets "approved: 1 (comment); rejected:
     2, 3 (comments)", and writes its plan from the approved ones. The plan
     file gets a **References** section listing them, added automatically.
   - If you rejected all of them, it gets your comments and shows a new round.
   - The leader is told in its inbox. It is not reminded, and it can never
     approve for you: only the page, the board or the CLI can.
6. **The approved references go with the work.** Every order that follows that
   plan lists them in `read`, for the hammer and for the code reviewer, so the
   built screen is checked against what you approved, not only against the
   plan's numbers.
7. **The board can do it without a browser too.** Enter on the line opens a
   small menu: each reference drawn with `chafa` (rough, for recognising it,
   not for judging a design), its note, and approve or reject keys. HTML
   mockups show only their note there; open the page for those.

### Who can open the page

`127.0.0.1` is reachable by **every account on this machine**, not only
yours. There is another one: `aiplatform` (uid 1005) already holds port
18000. So the page's address carries a random key (`/r/<key>`), shown only on
your board and in `devagentics references url`. Without the key, every
request gets 404, and answers are accepted only with the key.

### An example

1. The leader orders designer-10: "plan G9, the publisher dashboard".
2. designer-10 reads your two images (`scratch/references/u1-linear.png`,
   `u2-stripe.png`). It builds `a-dense.html`, `b-cards.html` and
   `c-sidebar.html`, adds them with notes, submits, and stops.
3. Your board shows `REFERENCES G9 · 3 to review`. You open the link
   (with `ssh -L 13100:127.0.0.1:13100` if you're on SSH). You approve B
   with "less shadow", reject A and C, and click Send.
4. designer-10 gets your answer and writes the plan. The plan's References
   section links B and your comment. Hammer-61's order for G9.2 lists
   `references/G9/b-cards.html` in `read`.

## Implementation (`agentic/`, tests first)

### A. State and rules: `src/references.rs` (new)
- `state.rs`: `references: Vec<ReferenceSet>` (`#[serde(default)]`).
  - `ReferenceSet {step, round, status: Drafting|Waiting|Approved|Rejected, items: Vec<Reference>, sent_at}`
  - `Reference {id: "r1", file, kind: Image|Html, note, from: Designer|User, verdict: Option<Approved|Rejected>, comment}`
  - User images, kept for the whole task: `user_references: Vec<Reference>`.
- Files go in `task.dir()/references/<step>/<round>/` and
  `task.dir()/references/user/`. Only PNG, JPEG, WebP, SVG and HTML are
  accepted, checked by their content, at most 10 MB each and 8 to a round.
- Functions:
  - `add(task, agent, file, note)`: needs the agent's open order. It copies
    the file in, so the designer can't change it after you've seen it.
  - `submit(task, agent)`: Drafting → Waiting; needs at least one item; logs
    the event and tells the leader in its inbox.
  - `answer(task, step, verdicts, comments)`: Waiting → Approved if any is
    approved, otherwise Rejected. It types the answer into the designer with
    `devws agent send` and refuses while you're typing, the same as other
    messages.
  - `give(task, file, note)`: a user image.
  - `approved_for(state, step)`.
- `roles.rs`: front matter `references: bool` (default false).
- `plans.rs::store_execution`: if the agent's role has `references`, it is
  refused without `approved_for(step)`, with the message "show references
  first: devagentics references add/submit". On success the plan gets the
  References section.
- `orders.rs::send_order`: an order whose `plan` step has approved
  references gets their files in `read`.
- Orders: when a designer's order is delivered, the user images are copied
  into its `scratch/references/`, and the order text lists them.

### B. The CLI (`main.rs`)
- For workers: `devagentics references add <file> --note …`, `references
  submit`, `references show` (your past rounds and comments).
- For the user: `devagentics references give <task> <file> --note …`,
  `references url <task>`, `references approve|reject <task> <step> <id>
  [--comment …]`, `references send <task> <step>`.

### C. The page: `src/references_page.rs` (new)
- A small HTTP/1.1 server on `std::net::TcpListener`, bound to
  `127.0.0.1:<references_port>` (task.toml, default 13100). It runs as a
  thread of the dispatcher (`watch.rs`), so it lives as long as the team.
- The key: 32 random bytes in hex, kept in `task.dir()/references/key`
  (0600). Every path is under `/r/<key>/`; anything else gets 404.
- Routes:
  - `GET /` lists the waiting sets;
  - `GET /file/<step>/<round>/<name>` serves a file;
  - `POST /answer` takes the verdicts and comments;
  - `POST /upload` takes your images.
- Everything it serves carries `Content-Security-Policy: default-src 'self'
  data:; script-src 'none'` on the mockups. A mockup is shown in an `<iframe
  sandbox>`, so it can't run scripts or reach the network. The page itself
  uses plain forms, with no scripts.
- A port that's taken is logged, the board shows "page unavailable: port
  13100 in use", and the CLI way still works.

### D. Board (`board.rs`, `board_view.rs`, `board_tui.rs`)
- A NEEDS YOU line for each Waiting set, with the URL.
- Enter opens the menu: references listed with `chafa --size 60x20 --format
  symbols` when it's there, the note, keys `a`, `r`, `c` (comment) and `s`
  (send).

### E. Roles and docs
- `roles/samples/designer.md` (new, in devws): `references = true` and the
  references step before planning.
- `roles/leader.md`:
  - when a designer is waiting on references, tell the user the link;
  - never ask the user to approve design in chat;
  - give your inspiration images with `references give`.
- `task_status`: references sets with their status.
- Docs: `docs/user/devagentics-execution.md` ("Design references") and
  `agentic/docs/usage.md`.
- The publisherProvider designer (`.devws/roles/designer.md`) lives in that
  repo, so it changes only with the user's OK. This needs the same two lines
  as the sample.

## Tests

- `plan write` by a `references` role: refused before approval, accepted
  after, and the plan has its References section. A role without the option
  is unchanged.
- `add` refuses with no open order, a wrong type (a `.png` that is really a
  script), or more than 8 files. The stored copy doesn't change when the
  scratch file does.
- `answer`: one approved → Approved and the designer's message; all rejected
  → Rejected and a new round can be added; the leader can't call it (no MCP
  tool).
- A follow-up order gets the approved files in `read`.
- The page:
  - 404 without the key or with a wrong one;
  - a file outside the set's folder (`../`) gets 404;
  - an answer through `POST` gives the same result as the CLI;
  - the CSP and `sandbox` headers are there;
  - it binds to `127.0.0.1` only.
- Board: the NEEDS YOU line, and the menu keys answering a set
  (`board_view_tests.rs`, `board_tui` tests).

## Verification

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `scripts/check.sh`.
- Live: install, restart the team (pause, wait, restart, resume), give one
  image, order the designer to show references for the next front item, open
  the page over `ssh -L 13100:…`, approve one, and see the plan written with
  its References section.

## Out of scope

- Screenshots of the built screen compared with the reference (a later step:
  the reviewer could look at both).
- Searching the web for references: the team has no internet access.
