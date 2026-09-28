"""Next-legendary counts for Recall engine v3, from the ranked-timeline decision table (dec.parquet).
Counts, not probabilities: the engine smooths at runtime (owned set -> nth -> role backoff).
Weights: patches 16.17-16.18 (shop identical to 16.19) x1, 16.13-16.16 x0.5; each summoner's games on a
champion-role capped at 10 effective games (one-trick bias)."""
import json, sys, collections, zlib, re, os
import pandas as pd
TRAITS = json.load(open(os.path.join(os.path.dirname(os.path.abspath(sys.argv[2])) if 'data/pack' in sys.argv[2] else os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'data', 'pack'), 'champion_traits.json')))['champions']
_norm = lambda s: re.sub(r'[^a-z0-9]', '', str(s).lower())
_T = {_norm(k): v for k, v in TRAITS.items()}
# Enemy-composition features, defined exactly as the engine computes them (nextprior::Comp): any enemy
# with meaningful healing; three or more magic-damage enemies; two or more tanks.
def comp(row):
    ts = [_T.get(_norm(row[e]), {}) for e in ('e_TOP', 'e_JUNGLE', 'e_MIDDLE', 'e_BOTTOM', 'e_UTILITY')]
    return pd.Series({'heal': any(t.get('healing') for t in ts),
                      'magic': sum(t.get('damage') == 'ap' for t in ts) >= 3,
                      'tank': sum(bool(t.get('tank')) for t in ts) >= 2})
src, out, holdout = sys.argv[1], sys.argv[2], len(sys.argv) > 3
d = pd.read_parquet(src)
d = d[(d.type == 'leg') & d.kind.isin(['complete', 'complete_virtual', 'buy'])].copy()
ROLE = {'TOP': 'Top', 'JUNGLE': 'Jungle', 'MIDDLE': 'Mid', 'BOTTOM': 'ADC', 'UTILITY': 'Support'}
d['role'] = d.role.map(ROLE)
d['pw'] = d.patch.map(lambda p: 1.0 if p in ('16.17', '16.18') else 0.5)
games = d.groupby(['sfk', 'champ', 'role']).smid.nunique().rename('ng')
d = d.join(games, on=['sfk', 'champ', 'role'])
d['w'] = d.pw * (10.0 / d.ng).clip(upper=1.0)
d = d.join(d.drop_duplicates('smid').set_index('smid').apply(comp, axis=1), on='smid')
d['key'] = d.owned.fillna('').map(lambda s: ','.join(sorted(x for x in s.split() if x)))
if holdout:
    d['test'] = d.sfk.map(lambda s: zlib.crc32(str(s).encode()) % 100 < 15)
    test = d[d.test]; d = d[~d.test]
def table(g, keycol, minw=3.0, top=12):
    res = {}
    for k, gg in g.groupby(keycol):
        c = gg.groupby('item').w.sum().sort_values(ascending=False)
        if c.sum() < minw: continue
        res[str(k)] = {str(i): round(float(v), 2) for i, v in c.head(top).items()}
    return res
champs = {}
for (champ, role), g in d.groupby(['champ', 'role']):
    if g.smid.nunique() < 30: continue
    champs[f'{int(g.champ_id.iloc[0])}|{role}'] = {'name': champ, 'games': int(g.smid.nunique()), 'sets': table(g, 'key'), 'nth': table(g, 'nth')}
def lifts(g, k=100.0, lo=0.33, hi=3.5):
    # P(item | role, feature value) / P(item | role), shrunk toward 1 with k pseudo-completions, clipped.
    base = g.groupby('item').w.sum(); base = base / base.sum()
    out = {}
    for f in ('heal', 'magic', 'tank'):
        out[f] = {}
        for val, gg in g.groupby(f):
            c = gg.groupby('item').w.sum(); n = c.sum()
            res = {}
            for i, p in base.items():
                if p < 0.002: continue
                pf = (c.get(i, 0.0) + k * p) / (n + k)
                res[str(i)] = round(min(hi, max(lo, pf / p)), 3)
            out[f]['1' if val else '0'] = res
    return out
roles = {r: {'sets': table(g, 'key', minw=10), 'nth': table(g, 'nth', minw=10), 'lifts': lifts(g)} for r, g in d.groupby('role')}
doc = {'source': 'Kaggle nathansmallcalder/league-of-legends-ranked-post-match-and-timeline (MIT), Master+ ranked, patches 16.13-16.18',
       'unit': 'weighted completions of the next legendary item', 'champions': champs, 'roles': roles}
json.dump(doc, open(out, 'w'), separators=(',', ':'))
print('champion-roles', len(champs), 'bytes', len(json.dumps(doc, separators=(",", ":"))))
if holdout:
    # held-out summoners: smoothed owned-set model, as the engine will compute it
    A_SET, A_NTH, A_ROLE = 30.0, 100.0, 3000.0
    CR = {(v['name'], k.split('|')[1]): v for k, v in champs.items()}
    def dist(t, k):
        return t.get(str(k), {})
    def probs(champ, role, key, nth, static=False):
        cr = CR.get((champ, role)); ro = roles.get(role, {'sets': {}, 'nth': {}})
        base = collections.Counter({i: 1.0 for i in dist(ro['nth'], nth)})
        def smooth(counts, prior, a):
            n = sum(counts.values()); tot = sum(prior.values()) or 1.0
            items = set(counts) | set(prior)
            return {i: (counts.get(i, 0.0) + a * prior.get(i, 0.0) / tot) / (n + a) for i in items}
        p_role = smooth(dist(ro['nth'], nth), {}, 0.0) if dist(ro['nth'], nth) else {}
        p_rset = smooth(dist(ro['sets'], key), p_role, A_ROLE) if p_role else {}
        if not cr: return p_rset
        p_nth = smooth(dist(cr['nth'], nth), p_role if static else (p_rset or p_role), A_NTH)
        return p_nth if static else smooth(dist(cr['sets'], key), p_nth, A_SET)
    for static in (True, False):
     hit1 = hit3 = n = 0
     for r in test.itertuples():
        p = probs(r.champ, r.role, r.key, r.nth, static)
        if not p: continue
        # exclude owned items from candidates
        owned = set(r.key.split(',')) if r.key else set()
        ranked = [i for i, _ in sorted(p.items(), key=lambda x: -x[1]) if i not in owned]
        n += 1; hit1 += ranked[:1] == [str(r.item)]; hit3 += str(r.item) in ranked[:3]
     print(f"held-out summoners ({'static build order' if static else 'owned-set model'}): n={n} top1={hit1/n:.3f} top3={hit3/n:.3f}")
