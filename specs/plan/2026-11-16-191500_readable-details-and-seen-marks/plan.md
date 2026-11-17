# Readable details, and NEEDS YOU rows marked once handled

Status: built and installed (2026-11-16, asked for by the user); live check done (formatting; no NEEDS YOU row yet for the marks)

## Why
The user found a NEEDS YOU item hard to understand: permission p234 showed
its arguments as one long line of JSON, and the leader's questions came as
long one-line paragraphs. They also wanted rows they had already opened or
answered to change colour.

## Changes
1. `details_view::text` formats the dialog text:
   - `Label: value` lines, with the label in bold cyan;
   - JSON objects and arrays laid out over indented lines, tinted (keys, strings,
     numbers), with short lists kept on one line;
   - inline `1) … 2) …` items on lines of their own;
   - a paragraph over 160 characters split into one sentence per line;
   - `- ` bullets shown as `•`, and `code` and **bold** marked.

   The dialog's sizing, scrolling and drawing all use it, so they agree.
2. `seen::Marks` records, in `board-seen.json` in the task folder, which
   NEEDS YOU rows were opened or told about. The key is the row id plus a hash
   of its text, so a changed row is new again. An opened row is grey instead of
   yellow, and a told row also shows `✉` in its tail.

## Tests
`details_view` (JSON, numbered items, sentences, bullets, labels), `seen`
(marks remembered after a restart; a changed row is new again; other rows
are never marked), and the drawing of new, opened and told rows.
