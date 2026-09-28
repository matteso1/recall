"""Private, strict-inventory Match-v5 cases for the assembled planner benchmark."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import sys

import pandas as pd

from collect_matches import private_output, validate_pair
from riot_inventory import reconstruct, runes

ROLES = ['TOP', 'JUNGLE', 'MIDDLE', 'BOTTOM', 'UTILITY']
ROLE = dict(zip(ROLES, ['Top', 'Jungle', 'Mid', 'ADC', 'Support']))
NORMALIZE = {3097: 3095, 3042: 3004, 3040: 3003, 3121: 3119, 2530: 2526}


def player_split(puuid):
    bucket = int.from_bytes(hashlib.sha256(('recall-riot-v1:' + puuid).encode()).digest()[:4], 'big') % 100
    return 'test' if bucket < 15 else 'validation' if bucket < 30 else 'train'


def assign_match(players, seeds):
    heldout = {seeds[p] for p in players if seeds.get(p) in ('validation', 'test')}
    return 'excluded' if len(heldout) > 1 else next(iter(heldout), 'train')


def labels_after(purchases, timestamp, next_time, kinds):
    future = [p for p in purchases if p['retained'] and timestamp < p['timestamp'] <= next_time]
    labels = [dict(horizon='purchase', item=NORMALIZE.get(p['item'], p['item']), kind=kinds[p['item']])
              for p in future if kinds.get(p['item']) in ('leg', 'boots')]
    if future:
        first = min(p['timestamp'] for p in future)
        labels += [dict(horizon='shop', item=NORMALIZE.get(p['item'], p['item']), kind='buy',
                        component=kinds.get(p['item']) == 'component')
                   for p in future if p['timestamp'] == first]
    return labels


def snapshot_player(player, frame, inventory, kda, items, names):
    bag, slot = [], 0
    for item, count in sorted(Counter(inventory['items']).items()):
        data = items[str(item)]
        bound = item == inventory.get('role_slot_boots')
        stackable = data.get('stacks', 1) > 1
        for _ in range(1 if stackable else count):
            if slot >= 6 and not bound:
                raise ValueError('overfull inventory')
            bag.append(dict(id=item, name=data.get('name', ''), count=count if stackable else 1,
                            slot=9 if bound else slot))
            slot += not bound
    return dict(name=f"participant-{player['participantId']}",
                champion=names.get(player['championName'], player['championName']),
                team='ORDER' if player['teamId'] == 100 else 'CHAOS', position=player['teamPosition'],
                level=frame['level'], items=bag, kills=kda[0], deaths=kda[1], assists=kda[2],
                cs=frame['minionsKilled'] + frame['jungleMinionsKilled'])


def item_kinds(items):
    """Same SR finished-item boundary as nextprior; transformed IDs normalize at labels."""
    result = {}
    for key, data in items.items():
        item = int(key)
        if not data.get('maps', {}).get('11') or not data.get('gold', {}).get('purchasable'):
            continue
        boots = 'Boots' in data.get('tags', []) or item == 3172
        if boots:
            if item not in (1001, 2422):
                result[item] = 'boots'
            continue
        if data.get('consumed') or 'Consumable' in data.get('tags', []):
            result[item] = 'consumable'
            continue
        children = [items.get(i, {}) for i in data.get('into', [])]
        finished = bool(data.get('from') or data.get('depth', 0) > 1 or data.get('specialRecipe')) and not any(
                           c.get('maps', {}).get('11') and c.get('gold', {}).get('purchasable')
                           and c.get('inStore', True) and c['gold'].get('base') != 0
                           and not c.get('requiredAlly') and not c.get('requiredChampion')
                           for c in children)
        if finished and data['gold']['total'] >= 2200 and not data.get('consumed'):
            result[item] = 'leg'
        else:
            result[item] = 'finished' if finished else 'component'
    return result


def make_cases(match, timeline, reconstruction, target, items, names, kinds, report, policy='exact-team'):
    players = {p['participantId']: p for p in match['info']['participants']}
    pid = target['participantId']
    histories = reconstruction['players']
    # A final mismatch or unexplained event disqualifies the whole history. Closed,
    # explicitly uncertain transformations instead disqualify just affected frames.
    if histories[pid]['issues'] or not histories[pid]['reconciled']:
        report['unreconciled_target_games'] += 1
        return []
    if policy == 'exact-team' and any(p['issues'] or not p['reconciled'] for p in histories.values()):
        report['unreconciled_team_games'] += 1
        return []
    kda = {p: [0, 0, 0] for p in players}
    abilities = dict(q=0, w=0, e=0, r=0)
    deaths, result = [], []
    frames = timeline['info']['frames']
    for index, frame in enumerate(frames):
        timestamp = frame['timestamp']
        for event in frame['events']:
            if event['type'] == 'CHAMPION_KILL':
                killer, victim = event['killerId'], event['victimId']
                assists = event.get('assistingParticipantIds', [])
                if killer in kda:
                    kda[killer][0] += 1
                if victim in kda:
                    kda[victim][1] += 1
                for p in assists:
                    if p in kda:
                        kda[p][2] += 1
                if victim == pid:
                    opponent = lambda p: p in players and players[p]['teamId'] != target['teamId']
                    deaths.append(dict(time=event['timestamp']/1000,
                                       killer=names.get(players[killer]['championName'], players[killer]['championName']) if opponent(killer) else '',
                                       assisters=[names.get(players[p]['championName'], players[p]['championName']) for p in assists if opponent(p)]))
            elif event['type'] == 'SKILL_LEVEL_UP' and event['participantId'] == pid and event['skillSlot'] in (1,2,3,4):
                abilities['qwer'[event['skillSlot']-1]] += 1
        if timestamp < 360000:
            continue
        report['candidate_frames'] += 1
        if policy == 'exact-team' and not all(p['frames'][index]['exact'] for p in histories.values()):
            report['uncertain_inventory_frames'] += 1
            continue
        if not histories[pid]['frames'][index]['exact']:
            report['uncertain_target_inventory_frames'] += 1
            continue
        visible, missing = {}, Counter()
        for p, player in players.items():
            hist = histories[p]
            if hist['issues'] or not hist['reconciled']:
                missing['unreconciled_peer_player_frames'] += 1
                continue
            if not hist['frames'][index]['exact']:
                missing['uncertain_peer_player_frames'] += 1
                continue
            try:
                visible[p] = snapshot_player(player,frame['participantFrames'][str(p)],hist['frames'][index],kda[p],items,names)
            except ValueError:
                missing['overfull_peer_player_frames'] += p != pid
        if pid not in visible or (policy == 'exact-team' and len(visible) != 10):
            report['overfull_inventory_frames'] += 1
            continue
        report.update(missing)
        report['known_enemy_frames'] += sum(players[p]['teamId'] != target['teamId'] for p in visible)
        report['possible_enemy_frames'] += 5
        report['complete_team_frames'] += len(visible) == 10
        snapshot = dict(game_time=timestamp/1000, mode='CLASSIC',
                        me=dict(player=visible[pid], gold=max(0,frame['participantFrames'][str(pid)]['currentGold']),
                                abilities=abilities.copy(), rune_ids=sorted(runes(target) | set(target.get('perks',{}).get('statPerks',{}).values())),
                                spell_ids=[target['summoner1Id'], target['summoner2Id']]),
                        allies=[visible[p] for p in visible if p != pid and players[p]['teamId'] == target['teamId']],
                        enemies=[visible[p] for p in visible if players[p]['teamId'] != target['teamId']],
                        my_deaths=deaths.copy())
        next_time = frames[index+1]['timestamp'] if index+1 < len(frames) else timestamp
        labels = labels_after(histories[pid]['purchases'], timestamp, next_time, kinds)
        shop_items = [NORMALIZE.get(p['item'],p['item']) for p in histories[pid]['purchases']
                      if p['retained'] and timestamp < p['timestamp'] <= next_time]
        result.append(dict(snapshot=snapshot, labels=labels, shop_items=shop_items))
    return result


def prepare(args):
    from backtest import ROOT, execute, file_hash, training_aggregates, write_json
    from export_answers import ANSWERS, fit_answers

    cache = private_output(args.cache)
    cache.mkdir(parents=True, exist_ok=True)
    cache.chmod(0o700)
    root = args.data / 'riot'
    seed_files = sorted((root / 'seeds').glob('current-*.json'))
    if not seed_files:
        raise ValueError('Missing private current ladder cohort sidecars')
    seeds = {p['puuid']: player_split(p['puuid']) for path in seed_files
             for p in json.loads(path.read_text())['cohort']}
    manifests = sorted(root.glob('*/*/complete.json'))
    sources = seed_files + manifests
    code = [Path(__file__), ROOT/'tools/priors/riot_inventory.py', ROOT/'tools/priors/backtest.py',
            ROOT/'tools/priors/collect_matches.py', ROOT/'tools/priors/export_next.py',
            ROOT/'tools/priors/export_answers.py', ROOT/'tools/priors/export_boots.py',
            ROOT/'data/pack/champion_traits.json']
    catalogs = {}
    for path in manifests:
        patch = json.loads(path.read_text())['patch']
        folder = args.catalog.parent / (patch + '.1')
        catalogs[patch] = folder
    sources += [folder/name for folder in catalogs.values() for name in ('item.json','champion.json')]
    signature = dict(protocol='riot-exact-inventory-v1', patches=args.patches, roles=args.roles,
                     inventory_policy=args.inventory_policy,
                     limit_games=args.limit_games, minimum_training_games=30,
                     files={str(p):file_hash(p) for p in sources+code})
    fingerprint = hashlib.sha256(json.dumps(signature,sort_keys=True).encode()).hexdigest()
    # Always verify the raw responses, even on a cache hit.
    for path in manifests:
        manifest = json.loads(path.read_text())
        for name in ('match.json','timeline.json'):
            raw = (path.parent/name).read_bytes()
            if hashlib.sha256(raw).hexdigest() != manifest['sha256'][name]:
                raise ValueError('Raw pair changed after collection')
    manifest_path = cache/'manifest.json'
    if manifest_path.exists():
        previous = json.loads(manifest_path.read_text())
        if (previous.get('fingerprint') == fingerprint and previous.get('artifacts') and
                all((cache/p).exists() and file_hash(cache/p)==h for p,h in previous['artifacts'].items())):
            return previous
    print('Reconstructing paired timelines; unsupported inventory states will be counted and excluded.', flush=True)
    catalog_data = {p:json.loads((f/'item.json').read_text())['data'] for p,f in catalogs.items()}
    current_patch = '.'.join(json.loads((args.catalog/'item.json').read_text())['version'].split('.')[:2])
    if args.patches != [current_patch]:
        raise ValueError('Riot evaluation requires one exact patch matching --catalog')
    champion_data = json.loads((args.catalog/'champion.json').read_text())['data']
    names = {c['id']:c['name'] for c in champion_data.values()}
    items = catalog_data[current_patch]
    kinds = item_kinds(items)
    audit, split_counts, issues = Counter(), Counter(), Counter()
    training, train_players, train_decisions, evaluations = set(), [], [], []
    ownership = {answer:set() for answer in ANSWERS}
    for session,path in enumerate(manifests,1):
        patch, match, timeline = load_pair(path)
        reconstruction = reconstruct(match,timeline,catalog_data[patch])
        audit['matches'] += 1
        for p in reconstruction['players'].values():
            audit['player_histories'] += 1
            audit['final_compatible'] += p['reconciled']
            audit['histories_without_unexplained_events'] += not p['issues']
            audit['exact_final_histories'] += p['valid']
            audit['player_frames'] += len(p['frames'])
            audit['causally_exact_player_frames'] += sum(f['exact'] for f in p['frames'])
            issues.update(i['kind'] for i in p['issues'])
        if patch not in args.patches:
            continue
        players = match['info']['participants']
        split = assign_match([p['puuid'] for p in players],seeds)
        split_counts[split+'_matches'] += 1
        if split == 'excluded':
            continue
        if split != 'train':
            evaluations.append((session, split, path))
            continue
        for p in players:
            hist = reconstruction['players'][p['participantId']]
            if hist['issues'] or not hist['reconciled'] or p['teamPosition'] not in ROLES:
                split_counts['excluded_training_players'] += 1
                continue
            training.add(p['puuid'])
            smid = session*10 + p['participantId']
            enemies = {'e_'+other['teamPosition']:other['championName'] for other in players
                       if other['teamId'] != p['teamId']}
            if set(enemies) != {'e_'+r for r in ROLES}:
                split_counts['unknown_training_roles'] += 1
                continue
            row = dict(smid=smid, sfk=p['puuid'], champ=p['championName'], champ_id=p['championId'],
                       role=p['teamPosition'], patch=patch, dur=match['info']['gameDuration'], **enemies)
            train_players.append(row)
            bought = {x['item'] for x in hist['purchases'] if x['retained']}
            for answer,(answer_items,_,_) in ANSWERS.items():
                if bought & set(answer_items):
                    ownership[answer].add(smid)
            for purchase in hist['purchases']:
                i = purchase['item']
                if not purchase['retained'] or kinds.get(i) not in ('leg', 'boots'):
                    continue
                owned = sorted({NORMALIZE.get(o,o) for o in purchase['owned'] if kinds.get(NORMALIZE.get(o,o))=='leg'})
                train_decisions.append(dict(row, item=NORMALIZE.get(i,i), type=kinds[i], kind='buy',
                                            minute=purchase['timestamp']/60000, nth=len(owned)+1,
                                            owned=' '.join(map(str,owned))))
    heldout = {p for p,s in seeds.items() if s in ('validation','test')}
    if training & heldout:
        raise ValueError('Held-out seed player leaked into training')
    if not train_decisions:
        raise ValueError('No reconciled training purchases')
    df = pd.DataFrame(train_decisions)
    df.to_parquet(cache/'train_dec.parquet',index=False)
    players_df = pd.DataFrame(train_players)
    players_df.to_parquet(cache/'train_players.parquet',index=False)
    execute([sys.executable, ROOT/'tools/priors/export_next.py', cache/'train_dec.parquet',cache/'next_items.json'])
    execute([sys.executable, ROOT/'tools/priors/export_boots.py',cache/'train_dec.parquet',ROOT/'data/pack/champion_traits.json',cache/'boots.json'])
    source = 'Official Riot Match-v5, current Master+ ladder seeded cohort, '+current_patch+'; training partition only'
    write_json(cache/'answers.json',fit_answers(players_df[players_df.dur>=900],ownership,source))
    for name in ('next_items.json','boots.json'):
        doc = json.loads((cache/name).read_text()); doc['source'] = source; write_json(cache/name,doc)
    aggregates = training_aggregates(df)
    write_json(cache/'aggregates.json',aggregates)
    reports = {}
    for split in ('validation','test'):
        report = Counter()
        with (cache/(split+'.jsonl')).open('w') as output:
            for session,s,path in evaluations:
                if s != split:
                    continue
                _, match, timeline = load_pair(path)
                reconstruction = reconstruct(match, timeline, items)
                for target in match['info']['participants']:
                    if seeds.get(target['puuid']) != split or target['teamPosition'] not in args.roles:
                        continue
                    report['target_games'] += 1
                    key = f"{target['championId']}|{ROLE[target['teamPosition']]}"
                    if key not in aggregates or aggregates[key]['games'] < 30:
                        report['unsupported_champion_role_games'] += 1
                        continue
                    if args.limit_games and report['games'] >= args.limit_games:
                        continue
                    frames = make_cases(match,timeline,reconstruction,target,items,names,kinds,report,args.inventory_policy)
                    if not frames:
                        report['empty_games'] += 1
                        continue
                    case = dict(session=session*10+target['participantId'], champion=names.get(target['championName'],target['championName']),
                                role=ROLE[target['teamPosition']], aggregate=key,
                                enemies=[names.get(p['championName'],p['championName']) for p in match['info']['participants'] if p['teamId']!=target['teamId']],frames=frames)
                    output.write(json.dumps(case,separators=(',',':'),allow_nan=False)+'\n')
                    report['games'] += 1
                    report['frames'] += len(frames)
        reports[split] = dict(report)
        print(split,dict(report),flush=True)
    manifest = dict(schema=1, protocol=signature['protocol'], signature=signature,fingerprint=fingerprint,
                    train_players=len(training),train_games=len(train_players),train_decisions=len(df),
                    seed_splits=dict(Counter(seeds.values())),splits=dict(split_counts),cases=reports,
                    inventory_audit=dict(audit), inventory_issues=dict(issues),
                    inventory_policy=args.inventory_policy, limitations=[
                        'Evaluation target players and matches excluded from training; contextual non-seed peers may recur across groups.',
                        'Exact own combat inventory always required; exact-team also requires all peers. known-peers omits entire unresolved peer inventories and counts coverage.',
                        'Trinkets excluded, final stack quantities unavailable. Physical slot order is compacted; overfull bags excluded.',
                        'Unknown support choices and tear transforms exclude affected frames, creating strong early-game selection bias.',
                        'Minute cadence and later purchases do not establish exact shop gold, optimal timing, or win improvement.',
                        'Ranks verified only for ladder seed cohort at collection; contextual training players have unverified ranks.',
                        'Training-only historical purchase aggregates replace unavailable provider inputs; sparse coverage.',
                        'No production artifacts changed.'])
    artifact_names = ['next_items.json','boots.json','answers.json','aggregates.json','train_dec.parquet','train_players.parquet','validation.jsonl','test.jsonl']
    manifest['artifacts'] = {p:file_hash(cache/p) for p in artifact_names}
    write_json(manifest_path,manifest)
    write_json(cache/'inventory-audit.json',dict(counts=dict(audit),issues=dict(issues),cases=reports,limitations=manifest['limitations']))
    return manifest


def load_pair(path):
    manifest = json.loads(path.read_text())
    pair = {}
    for name in ('match.json', 'timeline.json'):
        raw = (path.parent/name).read_bytes()
        if hashlib.sha256(raw).hexdigest() != manifest['sha256'][name]:
            raise ValueError('Raw pair changed after collection')
        pair[name] = json.loads(raw)
    match, timeline = pair['match.json'], pair['timeline.json']
    validate_pair(match, timeline, match['metadata']['matchId'])
    return manifest['patch'], match, timeline
