import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import winenv  # noqa: E402


class LockfileTests(unittest.TestCase):
    def test_parse_lockfile(self):
        lf = winenv.parse_lockfile("LeagueClient:17896:56139:TmSecretPass_-xyz:https\n")
        self.assertEqual((lf.process, lf.pid, lf.port, lf.protocol), ("LeagueClient", 17896, 56139, "https"))
        self.assertEqual(lf.auth, ("riot", "TmSecretPass_-xyz"))
        self.assertNotIn("SecretPass", lf.masked())

    def test_parse_lockfile_rejects_garbage(self):
        with self.assertRaises(ValueError):
            winenv.parse_lockfile("not a lockfile")

    def test_parse_ux_commandline(self):
        cmd = ('"C:/Riot Games/League of Legends/LeagueClientUx.exe" --riotclient-auth-token=abc '
               '--app-port=56139 --remoting-auth-token=Tm_Secret-Token --region=NA')
        self.assertEqual(winenv.parse_ux_commandline(cmd), (56139, "Tm_Secret-Token"))
        self.assertIsNone(winenv.parse_ux_commandline(""))
        self.assertIsNone(winenv.parse_ux_commandline("--app-port=1"))


class InstallDiscoveryTests(unittest.TestCase):
    def test_parse_riot_client_installs(self):
        text = ('{"associated_client": {"C:/Riot Games/League of Legends/": "C:/Riot Games/Riot Client/RiotClientServices.exe",'
                ' "D:/Games/VALORANT/live/": "C:/Riot Games/Riot Client/RiotClientServices.exe"}, "rc_live": "x"}')
        self.assertEqual(winenv.parse_riot_client_installs(text), ["C:/Riot Games/League of Legends/"])
        self.assertEqual(winenv.parse_riot_client_installs("{bad json"), [])

    @unittest.skipIf(winenv.is_windows(), "path translation only applies off-Windows")
    def test_windows_to_local_path(self):
        self.assertEqual(str(winenv.windows_to_local_path("C:/Riot Games/League of Legends/")),
                         "/mnt/c/Riot Games/League of Legends")
        self.assertEqual(str(winenv.windows_to_local_path(r"D:\Games\x")), "/mnt/d/Games/x")


if __name__ == "__main__":
    unittest.main()
