"""Dataset membership and identity stay fixed while collection continues."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from riot_dataset import freeze, inputs
from riot_corpus import load_pair
from collect_matches import collect_pair
from test_collect_matches import FakeClient, MATCH_ID, responses


class DatasetContracts(unittest.TestCase):
    def test_snapshot_survives_new_seeds_matches_and_manifest_timestamps(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)/'riot'
            match,timeline = responses()
            collect_pair(FakeClient(match,timeline),root/'europe',MATCH_ID,{'16.19'})
            seeds = root/'seeds';seeds.mkdir()
            (seeds/'current-test.json').write_text(json.dumps({'cohort':[{'puuid':'fictional-player-1','tier':'master'}]}))
            target = Path(directory)/'frozen.json'
            freeze(root,target)
            before = target.read_bytes()
            (seeds/'current-added.json').write_text(json.dumps({'cohort':[{'puuid':'future-player','tier':'master'}]}))
            metadata_path = root/'europe'/MATCH_ID/'complete.json'
            metadata = json.loads(metadata_path.read_text());metadata['validated_at']='later'
            metadata_path.write_text(json.dumps(metadata))
            # Another completed folder would be visible to a fresh collection scan.
            folder = root/'europe'/'EUW1_456';folder.mkdir();(folder/'complete.json').write_text(json.dumps(metadata))
            cohort,records,files = inputs(root,target)
            self.assertEqual([p['puuid'] for p in cohort],['fictional-player-1'])
            self.assertEqual(len(records),1)
            self.assertEqual(files,[target])
            self.assertEqual(load_pair(*records[0])[0],'16.19')
            self.assertEqual(target.read_bytes(),before)
            with self.assertRaisesRegex(ValueError,'exists'):
                freeze(root,target)

            # Re-signing a changed raw response in complete.json cannot alter a frozen dataset.
            changed = root/'europe'/MATCH_ID/'match.json'
            changed.write_text('{}')
            metadata['sha256']['match.json']=hashlib.sha256(b'{}').hexdigest()
            metadata_path.write_text(json.dumps(metadata))
            with self.assertRaisesRegex(ValueError,'changed'):
                load_pair(*records[0])

    def test_snapshot_rejects_paths_outside_the_raw_store(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);target=root/'snapshot.json'
            target.write_text(json.dumps(dict(schema=1,cohort=[],pairs=[dict(folder='../private',patch='16.19',sha256={})])))
            with self.assertRaisesRegex(ValueError,'folder'):
                inputs(root,target)

    def test_pair_must_match_folder_identity_and_ranked_queue(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);match,timeline=responses()
            folder=root/'EUW1_456';folder.mkdir()
            for name,doc in [('match.json',match),('timeline.json',timeline)]:
                (folder/name).write_text(json.dumps(doc))
            metadata=dict(patch='16.19',sha256={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.iterdir()})
            with self.assertRaisesRegex(ValueError,'match ID'):
                load_pair(folder/'complete.json',metadata)
            corrected=root/MATCH_ID;folder.rename(corrected)
            match['info']['queueId']=400
            (corrected/'match.json').write_text(json.dumps(match))
            metadata['sha256']['match.json']=hashlib.sha256((corrected/'match.json').read_bytes()).hexdigest()
            with self.assertRaisesRegex(ValueError,'ranked'):
                load_pair(corrected/'complete.json',metadata)


if __name__ == '__main__':
    unittest.main()
