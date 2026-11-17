"""The notes API's client: see GOAL.md and contract/notes.json."""


class NotesError(Exception):
    pass


class NotesClient:
    def __init__(self, base_url):
        raise NotImplementedError("the client is not built yet")
