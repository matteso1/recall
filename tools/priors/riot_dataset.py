"""Freeze private Riot pair membership, raw hashes and the held-out seed cohort.

A dataset file is immutable input to backtest.py --source riot --dataset FILE.
Collection may continue alongside it without changing a comparison's observations.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path

from collect_matches import MATCH_ID, REGIONS, private_output, write_json


def inputs(root, dataset=None):
    root = Path(root).resolve()
    if dataset is None:
        seed_files = sorted((root/'seeds').glob('current-*.json'))
        if not seed_files:
            raise ValueError('Missing private current ladder cohort sidecars')
        cohort = [p for path in seed_files for p in json.loads(path.read_text())['cohort']]
        paths = sorted(root.glob('*/*/complete.json'))
        return cohort, [(p,json.loads(p.read_text())) for p in paths], seed_files+paths
    dataset = Path(dataset)
    doc = json.loads(dataset.read_text())
    if doc.get('schema') != 1:
        raise ValueError('Unsupported frozen dataset schema')
    records, seen = [], set()
    for pair in doc['pairs']:
        parts = Path(pair['folder']).parts
        if len(parts) != 2 or parts[0] not in REGIONS or not MATCH_ID.fullmatch(parts[1]):
            raise ValueError('Invalid frozen dataset folder')
        path = root/parts[0]/parts[1]/'complete.json'
        if not path.resolve().is_relative_to(root) or pair['folder'] in seen:
            raise ValueError('Invalid or duplicate frozen dataset folder')
        seen.add(pair['folder'])
        if set(pair.get('sha256',{})) != {'match.json','timeline.json'}:
            raise ValueError('Frozen pair must carry both response hashes')
        records.append((path,dict(patch=pair['patch'],sha256=pair['sha256'])))
    return doc['cohort'], records, [dataset]


def freeze(root, output, *, cohort=None, records=None):
    root, output = Path(root).resolve(), private_output(Path(output))
    if output.exists():
        raise ValueError('Dataset file already exists; choose a new snapshot name')
    if cohort is None or records is None:
        cohort, records, _ = inputs(root)
    if not records or not cohort:
        raise ValueError('A snapshot needs completed pairs and a seed cohort')
    pairs = []
    for path, metadata in records:
        folder = path.parent.relative_to(root).as_posix()
        for name, expected in metadata['sha256'].items():
            if name not in ('match.json','timeline.json'):
                raise ValueError('Unexpected raw response name')
            if hashlib.sha256((path.parent/name).read_bytes()).hexdigest() != expected:
                raise ValueError('Raw pair changed after collection')
        pairs.append(dict(folder=folder,patch=metadata['patch'],sha256=metadata['sha256']))
    write_json(output,dict(schema=1,created_at=datetime.now(timezone.utc).isoformat(),
                           source='Riot Match-v5 frozen pair membership and ladder cohort',
                           cohort=cohort,pairs=sorted(pairs,key=lambda p:p['folder'])))
    return dict(pairs=len(pairs),seed_players=len({p['puuid'] for p in cohort}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root',type=Path,default=Path.home()/'data/recall/riot')
    parser.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    report = freeze(args.root,args.output)
    print(f"Frozen {report['pairs']} pairs and {report['seed_players']} seed players; raw data remains private.")


if __name__ == '__main__':
    main()
