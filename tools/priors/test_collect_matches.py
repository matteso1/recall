"""Collector contracts with fictional players and an injected HTTP boundary."""
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
from urllib.error import HTTPError

from collect_matches import ApiError, RiotClient, collect_pair, validate_pair, private_output
from audit_matches import audit


MATCH_ID = "EUW1_123"


def responses():
    identities = [f"fictional-player-{i}" for i in range(1, 11)]
    participants = [dict(participantId=i, puuid=identities[i-1],
                         teamId=100 if i <= 5 else 200, championId=i,
                         teamPosition="BOTTOM", summoner1Id=4, summoner2Id=7,
                         perks={"styles": []}) for i in range(1, 11)]
    match = dict(metadata=dict(matchId=MATCH_ID, participants=identities),
                 info=dict(gameVersion="16.19.1.1", queueId=420,
                           gameDuration=1200, participants=participants))
    player_frames = {str(i): dict(participantId=i, level=1, currentGold=500,
                                 totalGold=500, minionsKilled=0, jungleMinionsKilled=0,
                                 xp=0) for i in range(1, 11)}
    events = [dict(type="ITEM_PURCHASED", participantId=1, itemId=1001, timestamp=15000),
              dict(type="ITEM_UNDO", participantId=1, beforeId=1001, afterId=0,
                   goldGain=300, timestamp=16000)]
    timeline = dict(metadata=dict(matchId=MATCH_ID, participants=identities),
                    info=dict(frameInterval=60000,
                              participants=[dict(participantId=p["participantId"], puuid=p["puuid"])
                                            for p in participants],
                              frames=[dict(timestamp=t, participantFrames=copy.deepcopy(player_frames),
                                           events=events if t == 60000 else [])
                                      for t in range(0, 1200001, 60000)]))
    timeline["info"]["frames"][-1]["events"] = [dict(type="GAME_END", timestamp=1200000)]
    return match, timeline


class FakeClient:
    def __init__(self, match, timeline):
        self.match, self.timeline = match, timeline
        self.calls = []

    def get(self, path):
        self.calls.append(path)
        return copy.deepcopy(self.timeline if path.endswith("/timeline") else self.match)


class FakeResponse(io.BytesIO):
    headers = {}


class CollectorContracts(unittest.TestCase):
    def test_audit_only_exports_aggregate_counts_from_complete_pairs(self):
        match, timeline = responses()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            collect_pair(FakeClient(match, timeline), root / "europe", MATCH_ID, {"16.19"})
            incomplete = root / "europe" / "EUW1_999"
            incomplete.mkdir()
            (incomplete / "match.json").write_text(json.dumps(match))
            report = audit(root)
            self.assertEqual(report["unique_players"], 10)
            group = report["groups"]["europe/16.19"]
            self.assertEqual(group["matches"], 1)
            self.assertEqual(group["player_frames"], 210)
            self.assertNotIn("fictional-player", json.dumps(report))
            self.assertNotIn(MATCH_ID, json.dumps(report))
            (root / "europe" / MATCH_ID / "timeline.json").write_text("{}")
            with self.assertRaisesRegex(ValueError, "changed after validation"):
                audit(root)

    def test_raw_output_cannot_be_inside_a_git_checkout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertEqual(private_output(root / "raw"), root / "raw")
            (root / ".git").mkdir()
            with self.assertRaisesRegex(ValueError, "outside Git"):
                private_output(root / "nested" / "raw")

    def test_partial_pair_resumes_and_preserves_undo_events_exactly(self):
        match, timeline = responses()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            folder = root / MATCH_ID
            folder.mkdir()
            (folder / "match.json").write_text(json.dumps(match))
            client = FakeClient(match, timeline)
            result = collect_pair(client, root, MATCH_ID, {"16.19"})
            self.assertEqual(client.calls, [f"/lol/match/v5/matches/{MATCH_ID}/timeline"])
            self.assertEqual(result["status"], "complete")
            self.assertEqual(json.loads((folder / "timeline.json").read_text()), timeline)
            self.assertTrue((folder / "complete.json").exists())
            client.calls.clear()
            self.assertEqual(collect_pair(client, root, MATCH_ID, {"16.19"})["status"], "cached")
            self.assertEqual(client.calls, [])

    def test_mismatched_timeline_is_not_saved_or_marked_complete(self):
        match, timeline = responses()
        timeline["metadata"]["matchId"] = "EUW1_456"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, "match"):
                collect_pair(FakeClient(match, timeline), root, MATCH_ID, {"16.19"})
            self.assertTrue((root / MATCH_ID / "match.json").exists())
            self.assertFalse((root / MATCH_ID / "timeline.json").exists())
            self.assertFalse((root / MATCH_ID / "complete.json").exists())

    def test_missing_player_or_swapped_identity_is_rejected(self):
        for change in ["frame", "identity", "chronology", "truncated", "gap", "missing_start"]:
            match, timeline = responses()
            if change == "frame":
                del timeline["info"]["frames"][1]["participantFrames"]["10"]
            elif change == "identity":
                timeline["info"]["participants"][0]["puuid"] = "different-fictional-player"
            elif change == "chronology":
                timeline["info"]["frames"][1]["timestamp"] = 0
            elif change == "truncated":
                timeline["info"]["frames"].pop()
            elif change == "gap":
                del timeline["info"]["frames"][1]
            else:
                del timeline["info"]["frames"][0]
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_pair(match, timeline, MATCH_ID)

    def test_patch_and_queue_filters_run_before_timeline_download(self):
        for field, value in [("gameVersion", "16.3.1"), ("queueId", 450)]:
            match, timeline = responses()
            match["info"][field] = value
            client = FakeClient(match, timeline)
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                result = collect_pair(client, Path(directory), MATCH_ID, {"16.19"})
                self.assertEqual(result["status"], "excluded")
                self.assertEqual(len(client.calls), 1)

    def test_cached_pair_does_not_bypass_new_patch_filter(self):
        match, timeline = responses()
        client = FakeClient(match, timeline)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            collect_pair(client, root, MATCH_ID, {"16.19"})
            client.calls.clear()
            self.assertEqual(collect_pair(client, root, MATCH_ID, {"16.20"})["status"], "excluded")
            self.assertEqual(client.calls, [])

    def test_invalid_match_id_cannot_escape_cache_directory(self):
        client = FakeClient(*responses())
        with tempfile.TemporaryDirectory() as directory, self.assertRaises(ValueError):
            collect_pair(client, Path(directory), "../outside", {"16.19"})
        self.assertEqual(client.calls, [])

    def test_throttling_respects_retry_after_and_uses_header_credentials(self):
        clock = [0.0]
        times, requests = [], []

        def sleep(seconds):
            clock[0] += seconds

        def opener(request, timeout):
            times.append(clock[0])
            requests.append(request)
            if len(times) == 1:
                raise HTTPError(request.full_url, 429, "rate limit", {"Retry-After": "7"}, io.BytesIO())
            return FakeResponse(b'{"ok": true}')

        client = RiotClient("fictional-test-key", "europe", opener=opener,
                            clock=lambda: clock[0], sleep=sleep)
        self.assertEqual(client.get("/lol/match/v5/matches/EUW1_123"), {"ok": True})
        client.get("/lol/match/v5/matches/EUW1_124")
        self.assertGreaterEqual(times[1] - times[0], 7)
        self.assertGreaterEqual(times[2] - times[1], 1.25)
        self.assertEqual(requests[0].get_header("X-riot-token"), "fictional-test-key")
        self.assertNotIn("fictional-test-key", requests[0].full_url)

    def test_authentication_failure_stops_without_response_or_key_disclosure(self):
        calls = []

        def opener(request, timeout):
            calls.append(request)
            raise HTTPError(request.full_url, 403, "fictional-secret-value", {},
                            io.BytesIO(b'fictional-secret-value'))

        client = RiotClient("fictional-secret-value", "europe", opener=opener)
        with self.assertRaises(ApiError) as caught:
            client.get("/lol/match/v5/matches/EUW1_123")
        self.assertNotIn("fictional-secret-value", str(caught.exception))
        self.assertEqual(len(calls), 1)

    def test_official_ladder_and_history_routes_use_separate_hosts(self):
        requests = []

        def opener(request, timeout):
            requests.append(request)
            return FakeResponse(b'[]')

        RiotClient("test-key", "euw1", opener=opener).get(
            "/lol/league/v4/masterleagues/by-queue/RANKED_SOLO_5x5")
        RiotClient("test-key", "europe", opener=opener).get(
            "/lol/match/v5/matches/by-puuid/fictional-player/ids?queue=420&count=20&startTime=1")
        self.assertTrue(requests[0].full_url.startswith("https://euw1.api.riotgames.com/"))
        self.assertTrue(requests[1].full_url.startswith("https://europe.api.riotgames.com/"))
        with self.assertRaises(ValueError):
            RiotClient("test-key", "europe", opener=opener).get("https://example.com/collect")


if __name__ == "__main__":
    unittest.main()
