import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import updateguard


def unavailable():
    raise RuntimeError("local client API unavailable")


class UpdateGuardTests(unittest.TestCase):
    def test_api_failure_never_means_idle_while_league_is_running(self):
        for processes, expected in [({"League of Legends"}, "InProgress"),
                                    ({"LeagueClientUx"}, "Unknown"),
                                    (None, "Unknown"), (set(), "NoClient")]:
            with self.subTest(processes=processes):
                self.assertEqual(updateguard.phase(unavailable, lambda: processes), expected)

    def test_stale_lobby_phase_cannot_override_a_running_game(self):
        self.assertEqual(updateguard.phase(lambda: "Lobby", lambda: {"League of Legends"}),
                         "InProgress")

    def test_idle_phase_requires_successful_process_check(self):
        self.assertEqual(updateguard.phase(lambda: "Lobby", lambda: {"LeagueClientUx"}), "Lobby")
        self.assertEqual(updateguard.phase(lambda: "Lobby", unavailable), "Unknown")
        self.assertEqual(updateguard.phase(lambda: "NewUnknownPhase", lambda: {"LeagueClientUx"}),
                         "Unknown")

    def test_busy_client_phases_hold_without_process_lookup(self):
        for name in ["Matchmaking", "ReadyCheck", "ChampSelect", "GameStart", "InProgress",
                     "Reconnect", "WaitingForStats", "PreEndOfGame"]:
            with self.subTest(phase=name):
                self.assertEqual(updateguard.phase(lambda: name, unavailable), name)

    def test_process_lookup_distinguishes_empty_result_from_failure(self):
        for result, expected in [(SimpleNamespace(returncode=0, stdout="[]"), set()),
                                 (SimpleNamespace(returncode=1, stdout="[]"), None),
                                 (SimpleNamespace(returncode=0, stdout=""), None),
                                 (SimpleNamespace(returncode=0, stdout='["LeagueClientUx"]'),
                                  {"LeagueClientUx"})]:
            with self.subTest(result=result), patch("updateguard.shutil.which", return_value="powershell.exe"), \
                 patch("updateguard.subprocess.run", return_value=result):
                self.assertEqual(updateguard.league_processes(), expected)


if __name__ == "__main__":
    unittest.main()
