# Notes service

Build a small notes service in two parts that agree on one contract. This is
the benchmark's fixed scope: give it to the leader unchanged on every run.

- **API** (`api/notes_api.py`): an HTTP JSON server on the standard library
  (`http.server`), notes kept in memory.
  - `POST /notes` with `{"title", "body"}` creates a note: `201` and the note.
  - `GET /notes` lists the notes, oldest first: `200` and `{"notes": [...]}`.
  - `GET /notes/<id>` returns one note, `DELETE /notes/<id>` removes it
    (`204`). An unknown id is `404` with `{"error": "not_found"}`.
  - A missing or empty `title` is `400` with `{"error": "invalid_note"}`.
  - `serve(port)` starts it and returns the server; port 0 picks a free one.
- **Client** (`client/notes_client.py`): `NotesClient(base_url)` with
  `create(title, body)`, `list()`, `get(note_id)` and `delete(note_id)`,
  using `urllib`. API errors raise `NotesError` carrying the HTTP status and
  the error code.
- **Contract** (`contract/notes.json`): the note's fields and the error codes.
  Both parts follow it; change it only as a plan amendment.

Done means `python3 -m unittest discover -s tests -t .` passes, including the
acceptance tests in `tests/acceptance/`, which judge the work and are not
changed by the implementation.
