"""Run the production planner on player-held-out minute timelines.

First run trains isolated artifacts and builds the input cache. Subsequent runs reuse it.
  python tools/priors/backtest.py --output ~/data/recall/evaluation/before.json
  python tools/priors/backtest.py --baseline ~/data/recall/evaluation/before.json
Use --split test only after selecting changes on validation. Raw cases stay outside git.
"""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import duckdb
import pandas as pd

ROOT = Path(__file__).resolve().parents[2]
ROLE = {'TOP': 'Top', 'JUNGLE': 'Jungle', 'MIDDLE': 'Mid', 'BOTTOM': 'ADC', 'UTILITY': 'Support'}
SERDE_ROLE = dict(ROLE, BOTTOM='Adc')
ROLES = list(ROLE)
KINDS = ['buy', 'complete', 'complete_virtual']
SCHEMA = 1


def player_split(value):
    digest = hashlib.sha256(f'recall-engine-v1:{int(value)}'.encode()).digest()
    bucket = int.from_bytes(digest[:4], 'big') % 100
    return 'test' if bucket < 15 else 'validation' if bucket < 30 else 'train'


def split_players(players):
    """Exclude mixed-split matches entirely, including their training participants."""
    p = players.copy()
    p['split'] = p.sfk.map(player_split)
    mixed = p.groupby('mid')['split'].nunique()
    p = p[~p.mid.isin(mixed[mixed > 1].index)]
    return {s: p[p.split == s].copy() for s in ('train', 'validation', 'test')}


def picked(ids, count, total):
    return dict(ids=list(map(int, ids)), games=int(count), wins=0,
                pick_rate=float(count / max(total, 1)))


def training_aggregates(decisions):
    """Provider-shaped inputs trained on purchases, never on evaluation-game labels.

    This substitutes the historical provider inputs that the corpus does not contain.
    It is not used by the live app and makes no claims about runes or skill order.
    """
    result = {}
    valid = decisions.loc[decisions.kind.isin(KINDS), ['smid','champ_id','role','type','nth','item','minute']]
    for (champ, role), group in valid.groupby(['champ_id', 'role']):
        games = group.smid.nunique()
        leg = group[group.type == 'leg'].sort_values(['smid', 'minute', 'nth'])
        sequences = list(leg.groupby('smid').head(3).groupby('smid')['item'].agg(tuple))
        complete = [s for s in sequences if len(s) == 3]
        counts = collections.Counter(complete or sequences)
        if not counts:
            continue
        lines = [picked(ids, n, games) for ids, n in sorted(counts.items(), key=lambda x: (-x[1], x[0]))]
        boots = group[group.type == 'boots'].item.value_counts()
        boot_lines = [picked([i], n, boots.sum()) for i, n in boots.items()]
        late = leg.drop_duplicates(['smid', 'item']).item.value_counts()
        result[f'{int(champ)}|{ROLE[role]}'] = dict(
            champion_key=int(champ), position=SERDE_ROLE[role], requested_position=None,
            patch='16.19', region='training-only', tier='master_plus', cached_at='',
            games=int(games), win_rate=0.0, positions=[], spells=picked([], 0, games),
            runes=None, skill_order=[], skill_max=[], starters=picked([], 0, games),
            core=lines[0], core_most_picked=lines[0], core_lines=lines[:20],
            core_alternatives=[], boots=boot_lines[0] if boot_lines else None,
            boots_lines=boot_lines, late=[picked([i], n, games) for i, n in late.items()], counters=[])
    return result


def file_hash(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path, value):
    Path(path).write_text(json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False))


def execute(argv, **kwargs):
    subprocess.run(list(map(str, argv)), check=True, **kwargs)


def prepare(args):
    cache = args.cache
    cache.mkdir(parents=True, exist_ok=True)
    files = [args.data / 'derived/dec.parquet', args.data / 'derived/players.parquet',
             args.timeline, args.catalog / 'item.json', args.catalog / 'champion.json',
             ROOT / 'data/pack/champion_traits.json', Path(__file__),
             ROOT / 'tools/priors/export_next.py', ROOT / 'tools/priors/export_answers.py',
             ROOT / 'tools/priors/export_boots.py']
    signature = dict(schema=SCHEMA, files={str(p): file_hash(p) for p in files},
                     patches=args.patches, roles=args.roles, limit_games=args.limit_games)
    fingerprint = hashlib.sha256(json.dumps(signature, sort_keys=True).encode()).hexdigest()
    manifest_path = cache / 'manifest.json'
    if manifest_path.exists():
        manifest = json.loads(manifest_path.read_text())
        artifacts = manifest.get('artifacts', {})
        if (manifest.get('fingerprint') == fingerprint and artifacts and
                all((cache / p).exists() and file_hash(cache / p) == h for p, h in artifacts.items())):
            return manifest
    print('Preparing isolated training artifacts and minute observations...', flush=True)
    players = pd.read_parquet(args.data / 'derived/players.parquet')
    decisions = pd.read_parquet(args.data / 'derived/dec.parquet')
    parts = split_players(players)
    train = parts['train'].drop(columns='split')
    train_dec = decisions[decisions.smid.isin(train.smid)].copy()
    train.to_parquet(cache / 'train_players.parquet', index=False)
    train_dec.to_parquet(cache / 'train_dec.parquet', index=False)
    execute([sys.executable, ROOT / 'tools/priors/export_next.py', cache / 'train_dec.parquet', cache / 'next_items.json'])
    execute([sys.executable, ROOT / 'tools/priors/export_answers.py', args.timeline, cache / 'train_players.parquet', cache / 'answers.json'])
    execute([sys.executable, ROOT / 'tools/priors/export_boots.py', cache / 'train_dec.parquet',
             ROOT / 'data/pack/champion_traits.json', cache / 'boots.json'])
    aggregates = training_aggregates(train_dec)
    write_json(cache / 'aggregates.json', aggregates)
    selected = {}
    for name in ['validation', 'test']:
        p = parts[name]
        p = p[p.patch.isin(args.patches) & p.role.isin(args.roles)].sort_values('smid')
        if args.limit_games:
            p = p.head(args.limit_games)
        selected[name] = p
    needed = pd.concat(list(selected.values()))[['smid', 'mid']]
    # Available peers in the same match are observed data, never copied from future frames.
    peer_players = players[players.mid.isin(needed.mid)]
    con = duckdb.connect()
    con.execute('set threads=2')
    con.register('wanted', peer_players[['smid']])
    timeline = con.execute("""select SummonerMatchFk, Minute, CurrentGold, Level,
        MinionsKilled, JungleMinionsKilled, Kills, Deaths, Assists,
        Item0, Item1, Item2, Item3, Item4, Item5, Item6 from read_csv_auto(?) t
        join wanted w on w.smid=t.SummonerMatchFk order by SummonerMatchFk, Minute""",
        [str(args.timeline)]).df()
    con.close()
    frames = collections.defaultdict(dict)
    for f in timeline.itertuples():
        frames[int(f.SummonerMatchFk)][int(f.Minute)] = f
    peers = collections.defaultdict(list)
    for player in peer_players.itertuples():
        peers[player.mid].append(player)
    item_data = json.loads((args.catalog / 'item.json').read_text())['data']
    champion_data = json.loads((args.catalog / 'champion.json').read_text())['data']
    names = {int(c['key']): c['name'] for c in champion_data.values()}
    boots = {int(i) for i, v in item_data.items() if 'Boots' in v.get('tags', [])}
    reports = {}
    for name, group in selected.items():
        report = collections.Counter()
        labels = decisions.loc[decisions.smid.isin(group.smid) & decisions.kind.isin(KINDS),
                               ['smid','type','item','minute','s_min','p_min']]
        label_groups = collections.defaultdict(list)
        for label in labels.itertuples():
            label_groups[int(label.smid)].append(label)
        with (cache / f'{name}.jsonl').open('w') as out:
            for session, player in enumerate(group.itertuples(), 1):
                key = f'{player.champ_id}|{ROLE[player.role]}'
                if key not in aggregates or aggregates[key]['games'] < 30:
                    report['unsupported_games'] += 1
                    continue
                game_frames = frames.get(int(player.smid))
                targets = label_groups.get(int(player.smid))
                if game_frames is None or targets is None:
                    report['missing_timeline_games'] += 1
                    continue
                cases = []
                by_minute = collections.defaultdict(list)
                boot_completions = {}
                for label in targets:
                    if label.type == 'boots':
                        boot_completions[int(label.minute)] = int(label.item)
                    for horizon, minute in [('start', label.s_min), ('purchase', label.p_min)]:
                        by_minute[int(minute)].append(dict(horizon=horizon, item=int(label.item), kind=label.type))
                own_name = names.get(int(player.champ_id), player.champ)
                game_peers = peers[player.mid]
                previous_boot = None
                for minute, f in game_frames.items():
                    previous_boot = boot_completions.get(minute, previous_boot)
                    minute = int(f.Minute)
                    if minute < 6:
                        continue
                    try:
                        own = make_player(player, f, own_name, item_data)
                    except ValueError:
                        report['overfull_inventory_frames'] += 1
                        continue
                    # Past confirmed completions can restore quest-hidden boots, never future labels.
                    if player.role == 'BOTTOM' and previous_boot and not any(i['id'] in boots for i in own['items']):
                        own['items'].append(dict(id=previous_boot, name=item_data.get(str(previous_boot), {}).get('name', ''), count=1, slot=9))
                        report['boots_restored_frames'] += 1
                    allies, enemies = [], []
                    for peer in game_peers:
                        if peer.smid == player.smid:
                            continue
                        # The corpus tracks only some participants. Never pretend the rest have no items.
                        exact = frames.get(int(peer.smid), {}).get(minute)
                        if exact is None:
                            continue
                        try:
                            other = make_player(peer, exact, names.get(int(peer.champ_id), peer.champ), item_data)
                        except ValueError:
                            report['overfull_peer_frames'] += 1
                            continue
                        (allies if peer.blue == player.blue else enemies).append(other)
                    report['known_enemy_frames'] += len(enemies)
                    report['possible_enemy_frames'] += 5
                    snapshot = dict(game_time=minute*60.0, mode='CLASSIC', allies=allies, enemies=enemies,
                                    me=dict(player=own, gold=max(0.0, float(f.CurrentGold)),
                                            abilities=dict(q=0,w=0,e=0,r=0), rune_ids=None,
                                            spell_ids=[int(player.sp1), int(player.sp2)]))
                    cases.append(dict(snapshot=snapshot, labels=by_minute[minute]))
                if not cases:
                    report['empty_games'] += 1
                    continue
                enemy_names = [getattr(player, 'e_' + r) for r in ROLES]
                # Dataset uses champion identifiers; resolve display names through the current catalog.
                display = {c['id']: c['name'] for c in champion_data.values()}
                enemy_names = [display.get(n, n) for n in enemy_names]
                out.write(json.dumps(dict(session=session, champion=own_name, role=ROLE[player.role],
                                          aggregate=key, enemies=enemy_names, frames=cases),
                                     separators=(',', ':'), allow_nan=False) + '\n')
                report['games'] += 1
                report['frames'] += len(cases)
        reports[name] = dict(report)
        print(name, reports[name], flush=True)
    manifest = dict(signature=signature, fingerprint=fingerprint, schema=SCHEMA,
                    train_games=len(train), train_players=int(train.sfk.nunique()),
                    excluded_shared_match_games=len(players)-sum(map(len,parts.values())),
                    splits={s:dict(games=len(p), players=int(p.sfk.nunique())) for s,p in parts.items()},
                    cases=reports,
                    limitations=['Minute cadence, not overlay poll cadence.',
                                 'Most enemies have composition only; no inferred equipment or kill feed.',
                                 'Full rune pages and observed ability ranks are unavailable.',
                                 'Historical op.gg inputs replaced with training-only purchase aggregates.',
                                 'Agreement is purchase imitation, not causal win improvement.'])
    manifest['artifacts'] = {p: file_hash(cache / p) for p in
                             ['next_items.json', 'answers.json', 'boots.json', 'aggregates.json', 'validation.jsonl', 'test.jsonl']}
    write_json(manifest_path, manifest)
    return manifest


def make_player(player, frame, champion, item_data):
    ids = [int(getattr(frame, f'Item{i}')) for i in range(7)]
    ids = [3095 if i == 3097 else i for i in ids if i]
    items = []
    bag_slot = 0
    for item in ids:
        data = item_data.get(str(item), {})
        trinket = 'Trinket' in data.get('tags', []) or item in (3340,3363,3364,3330)
        if not trinket and bag_slot >= 6:
            # Seven compacted non-trinkets can include a consumed elixir/buff. Its
            # real inventory status is unavailable; omit and count the frame.
            raise ValueError('overfull corpus inventory')
        items.append(dict(id=item, name=data.get('name',''), count=1, slot=6 if trinket else bag_slot))
        bag_slot += not trinket
    return dict(name='', champion=champion, position=player.role, team='ORDER' if player.blue else 'CHAOS',
                level=int(frame.Level), items=items, kills=int(frame.Kills), deaths=int(frame.Deaths),
                assists=int(frame.Assists), cs=int(frame.MinionsKilled + frame.JungleMinionsKilled))


def compare_reports(before, after):
    if before['fingerprint'] != after['fingerprint'] or before['split'] != after['split']:
        raise ValueError('Comparisons require the same cases, training artifacts and split')
    print(f"{'Metric':48} {'Before':>10} {'After':>10} {'Delta':>10}")
    for group, metrics in sorted(after['groups'].items()):
        prior = before['groups'].get(group, {})
        for numerator, denominator in [('target_hits', 'decisions'), ('path_hits', 'legendary_decisions'),
                                       ('path_top3_hits', 'legendary_decisions'), ('boots_hits', 'boots_decisions'),
                                       ('flips', 'transitions'), ('repeat_flips', 'frames'),
                                       ('invalid_frames', 'frames'), ('missing_target_candidates', 'frames')]:
            if not metrics.get(denominator) or not prior.get(denominator):
                continue
            a, b = prior.get(numerator, 0)/prior[denominator], metrics.get(numerator, 0)/metrics[denominator]
            print(f'{group + "/" + numerator:48} {a:9.2%} {b:9.2%} {100*(b-a):+9.2f}pp')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--data', type=Path, default=Path.home()/'data/recall')
    parser.add_argument('--timeline', type=Path)
    parser.add_argument('--catalog', type=Path, default=Path('/mnt/c/Users/nilsm/AppData/Local/Recall/ddragon/16.19.1'))
    parser.add_argument('--cache', type=Path, default=Path.home()/'data/recall/evaluation/cache')
    parser.add_argument('--output', type=Path, default=Path.home()/'data/recall/evaluation/latest.json')
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--baseline-bin', type=Path, help='Run an archived evaluator on the identical prepared cases')
    parser.add_argument('--split', choices=['validation','test'], default='validation')
    parser.add_argument('--patches', nargs='+', default=['16.17','16.18'])
    parser.add_argument('--roles', nargs='+', choices=ROLES, default=ROLES)
    parser.add_argument('--limit-games', type=int, default=0, help='Debug only; 0 means all eligible games')
    parser.add_argument('--prepare-only', action='store_true')
    args = parser.parse_args()
    args.timeline = args.timeline or args.data/'kaggle/ranked-timeline/MatchTimelineTbl.csv'
    manifest = prepare(args)
    if args.prepare_only:
        return
    execute([Path.home()/'.cargo/bin/cargo', 'build', '--manifest-path', ROOT/'overlay/Cargo.toml',
             '--release','--locked','-p','recall-core','--features','evaluation','--bin','backtest','-j','2'])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    raw = subprocess.run([str(ROOT/'overlay/target/release/backtest'), str(args.cache),
                          args.split, str(args.catalog)], stdout=subprocess.PIPE, text=True)
    if raw.returncode not in (0, 1):
        raise RuntimeError(raw.stderr)
    report = json.loads(raw.stdout)
    report['data'] = manifest
    report['revision'] = subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    report['source_diff_sha256'] = hashlib.sha256(subprocess.check_output(['git','diff','HEAD'],cwd=ROOT)).hexdigest()
    write_json(args.output, report)
    if args.baseline_bin:
        before = subprocess.run([str(args.baseline_bin), str(args.cache), args.split, str(args.catalog)],
                                stdout=subprocess.PIPE, text=True)
        if before.returncode not in (0, 1):
            raise RuntimeError('Baseline evaluator failed')
        previous = json.loads(before.stdout)
        write_json(args.output.with_suffix('.baseline.json'), previous)
        compare_reports(previous, report)
    elif args.baseline:
        compare_reports(json.loads(args.baseline.read_text()), report)
    else:
        compare_reports(report, report)
    print(f'Report: {args.output}; split={args.split}; cases={manifest["cases"][args.split]}')
    if raw.stderr:
        print(raw.stderr, file=sys.stderr)
    raise SystemExit(raw.returncode)


if __name__ == '__main__':
    main()
