"""The scope's acceptance: the client and the API, together over HTTP."""
import threading
import unittest

from api.notes_api import serve
from client.notes_client import NotesClient, NotesError


class NotesAcceptance(unittest.TestCase):
    def setUp(self):
        self.server = serve(0)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        host, port = self.server.server_address[:2]
        self.client = NotesClient(f"http://{host}:{port}")

    def test_a_note_is_created_listed_read_and_deleted(self):
        first = self.client.create("Groceries", "milk")
        second = self.client.create("Ideas", "")
        self.assertEqual(first["title"], "Groceries")
        self.assertEqual([note["id"] for note in self.client.list()], [first["id"], second["id"]])
        self.assertEqual(self.client.get(second["id"])["body"], "")
        self.client.delete(first["id"])
        self.assertEqual([note["id"] for note in self.client.list()], [second["id"]])

    def test_an_unknown_note_is_not_found(self):
        with self.assertRaises(NotesError) as raised:
            self.client.get(999)
        self.assertEqual((raised.exception.status, raised.exception.code), (404, "not_found"))
        with self.assertRaises(NotesError):
            self.client.delete(999)

    def test_a_note_needs_a_title(self):
        for title in ["", None]:
            with self.assertRaises(NotesError) as raised:
                self.client.create(title, "body")
            self.assertEqual((raised.exception.status, raised.exception.code), (400, "invalid_note"))


if __name__ == "__main__":
    unittest.main()
