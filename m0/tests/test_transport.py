import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import transport  # noqa: E402


class CurlParsingTests(unittest.TestCase):
    def test_body_and_status(self):
        r = transport.parse_curl_output(b'{"a": 1}\n200')
        self.assertEqual((r.status, r.json(), r.ok), (200, {"a": 1}, True))

    def test_empty_body(self):
        r = transport.parse_curl_output(b"\n204")
        self.assertEqual((r.status, r.body, r.json()), (204, b"", None))

    def test_multiline_body(self):
        r = transport.parse_curl_output(b"line1\nline2\n404")
        self.assertEqual((r.status, r.text()), (404, "line1\nline2"))

    def test_garbage_status(self):
        self.assertEqual(transport.parse_curl_output(b"oops").status, 0)


class BackendChoiceTests(unittest.TestCase):
    def test_forced(self):
        self.assertEqual(transport.choose_backend({"FEATHERSTORM_TRANSPORT": "curl"}), "curl")
        self.assertEqual(transport.choose_backend({"FEATHERSTORM_TRANSPORT": "direct"}), "direct")

    def test_auto_is_a_known_backend(self):
        self.assertIn(transport.choose_backend({}), ("curl", "direct"))


if __name__ == "__main__":
    unittest.main()
