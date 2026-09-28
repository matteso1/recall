"""Ban suggestions for Recall's champion select, from the ranked-timeline player table (players.parquet).
For each champion and role: the enemy champions whose presence costs the most expected win rate,
presence x (base win rate - shrunk win rate against them). Banning removes that champion from the game,
so this is the expected win-rate gain of the ban (associational, Master+ 16.13-16.18).
Usage: export_bans.py <players.parquet> <champion.json> <out.json>"""
import json, sys, re
import pandas as pd
src, champion_json, out = sys.argv[1:4]
K = 30.0            # shrinkage toward the champion's base win rate, in games
MIN_GAMES = 200     # champion-role tables below this use the role table
MIN_SEEN = 15       # enemy must appear at least this often to be named
TOP = 8
_norm = lambda s: re.sub(r'[^a-z0-9]', '', str(s).lower())
ddragon = json.load(open(champion_json, encoding='utf-8'))['data']
KEY = {}
for c in ddragon.values():
    KEY[_norm(c['id'])] = int(c['key'])
    KEY[_norm(c['name'])] = int(c['key'])
ROLE = {'TOP': 'Top', 'JUNGLE': 'Jungle', 'MIDDLE': 'Mid', 'BOTTOM': 'ADC', 'UTILITY': 'Support'}
ENEMY = ['e_TOP', 'e_JUNGLE', 'e_MIDDLE', 'e_BOTTOM', 'e_UTILITY']
p = pd.read_parquet(src)
p['role'] = p.role.map(ROLE)
long = p.melt(id_vars=['smid', 'champ_id', 'role', 'win'], value_vars=ENEMY, value_name='enemy')
long['enemy_key'] = long.enemy.map(lambda n: KEY.get(_norm(n)))
missing = sorted(set(long[long.enemy_key.isna()].enemy))
if missing:
    print('unmapped enemies:', missing, file=sys.stderr)
long = long.dropna(subset=['enemy_key'])
long['enemy_key'] = long.enemy_key.astype(int)

def table(games, faced):
    n = len(games)
    base = games.win.mean()
    g = faced.groupby('enemy_key').win.agg(['size', 'sum'])
    g = g[g['size'] >= MIN_SEEN]
    wr = (g['sum'] + K * base) / (g['size'] + K)
    value = g['size'] / n * (base - wr)
    rows = []
    for key in value.sort_values(ascending=False).index[:TOP]:
        if value[key] <= 0:
            break
        rows.append({'id': int(key), 'seen': round(g['size'][key] / n, 4), 'win_vs': round(float(wr[key]), 4),
                     'gain': round(float(value[key]), 5)})
    return {'games': n, 'base': round(float(base), 4), 'bans': rows}

champions = {}
for (cid, role), g in p.groupby(['champ_id', 'role']):
    if len(g) < MIN_GAMES:
        continue
    champions[f'{cid}|{role}'] = table(g, long[(long.champ_id == cid) & (long.role == role)])
roles = {role: table(g, long[long.role == role]) for role, g in p.groupby('role')}
doc = {'source': 'Kaggle ranked-timeline, Master+ 16.13-16.18; tools/priors/export_bans.py',
       'champions': champions, 'roles': roles}
json.dump(doc, open(out, 'w'), separators=(',', ':'), sort_keys=True)
print(len(champions), 'champion-roles;', sum(len(c['bans']) for c in champions.values()), 'bans', file=sys.stderr)
