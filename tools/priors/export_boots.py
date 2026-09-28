"""Boots choice for Recall, from the ranked-timeline decision table (dec.parquet): the first tier-2 boots each Master+
player completed, per champion and role, conditioned on how many enemies deal magic damage (0-4+, counted from
data/pack/champion_traits.json exactly as the engine counts them). Master+ players react strongly: top laners buy
Mercury's Treads in 2% of games against no magic damage and 45% against four sources; ADCs 2% and 11%. Poke does
not move ADC boots (Gluttonous Greaves 23% at any poke count), so it is not a feature.
P(boots | champion, role, n_ap) = normalize(P(boots | champion, role) x P(boots | role, n_ap) / P(boots | role)),
the champion's shares smoothed toward the role's with 20 pseudo-games. Champion-roles under 30 games are left out
(the engine keeps op.gg's boots).
Usage: export_boots.py <dec.parquet> <champion_traits.json> <out.json>"""
import json, re, sys
import pandas as pd
src, traits_path, out = sys.argv[1:4]
A = 20.0
MIN_GAMES = 30
MIN_SHARE = 0.03
ROLE = {'TOP': 'Top', 'JUNGLE': 'Jungle', 'MIDDLE': 'Mid', 'BOTTOM': 'ADC', 'UTILITY': 'Support'}
ENEMY = ['e_TOP', 'e_JUNGLE', 'e_MIDDLE', 'e_BOTTOM', 'e_UTILITY']
_norm = lambda s: re.sub(r'[^a-z0-9]', '', str(s).lower())
T = {_norm(k): v for k, v in json.load(open(traits_path))['champions'].items()}
d = pd.read_parquet(src)
b = d[(d.type == 'boots') & d.kind.isin(['buy', 'complete', 'complete_virtual'])].sort_values(['smid', 'minute']).drop_duplicates('smid').copy()
b['role'] = b.role.map(ROLE)
b['nap'] = b[ENEMY].apply(lambda r: min(4, sum(T.get(_norm(x), {}).get('damage') == 'ap' for x in r)), axis=1)
doc = {'source': 'Kaggle ranked-timeline, Master+ 16.13-16.18; tools/priors/export_boots.py', 'champions': {}}
for role, rb in b.groupby('role'):
    p_role = rb.item.value_counts(normalize=True)
    # Sparse composition buckets must not make an item impossible because it has zero observations.
    # Use the same 100-completion shrinkage as the legendary composition export.
    lift = {nap: ((g.item.value_counts().reindex(p_role.index, fill_value=0) + 100.0*p_role)
                  / (len(g)+100.0) / p_role) for nap, g in rb.groupby('nap')}
    for (cid, _), g in rb.groupby(['champ_id', 'role']):
        if len(g) < MIN_GAMES:
            continue
        counts = g.item.value_counts()
        p_champ = (counts.reindex(p_role.index, fill_value=0) + A * p_role) / (len(g) + A)
        table = {}
        for nap in range(5):
            p = p_champ * lift.get(nap, pd.Series(1.0, index=p_role.index)).reindex(p_role.index, fill_value=1.0)
            p = p / p.sum()
            table[str(nap)] = {str(int(i)): round(float(v), 4) for i, v in p[p >= MIN_SHARE].sort_values(ascending=False).items()}
        doc['champions'][f'{cid}|{role}'] = {'games': int(len(g)), 'by_magic_enemies': table}
json.dump(doc, open(out, 'w'), separators=(',', ':'), sort_keys=True)
print(len(doc['champions']), 'champion-roles', file=sys.stderr)
