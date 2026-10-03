"""Held-out comparison of the next-legendary smoothing in overlay/core/src/nextprior.rs.

Rebuilds the champion and role count tables from 85% of the players in dec.parquet (as export_next.py
does) and scores each variant on the other 15%: how often the item a Master+ player completed next is
the model's first choice, among its first three, and the mean log loss.

  previous  role backoff unrestricted, A_NTH 100 (engine v3 as shipped on 2026-09-27)
  support   the role backoff restricted to items the champion builds
  subsets   + the smaller-set view in the prior of an owned set (two items or more)
  current   + the build-step table down-weights items never built with an owned item; A_NTH 50

The held-out players are split in two by hash: constants were chosen on `tune`, `report` is the half
the choice did not see. Usage: eval_next.py <dec.parquet> [tune|report|all]"""
import collections, math, sys, zlib
import pandas as pd

src = sys.argv[1]
part = sys.argv[2] if len(sys.argv) > 2 else 'report'
A_SET, A_ROLE = 30.0, 3000.0
SUBSET_SHARE, NOT_BUILT_WITH = 0.5, 0.3
VARIANTS = {  # name -> (support mask, subset backoff, co-build weight, A_NTH)
    'previous': (False, False, False, 100.0),
    'support': (True, False, False, 100.0),
    'subsets': (True, True, False, 100.0),
    'current': (True, True, True, 50.0),
}
ROLE = {'TOP': 'Top', 'JUNGLE': 'Jungle', 'MIDDLE': 'Mid', 'BOTTOM': 'ADC', 'UTILITY': 'Support'}

d = pd.read_parquet(src)
d = d[(d.type == 'leg') & d.kind.isin(['complete', 'complete_virtual', 'buy'])].copy()
d['role'] = d.role.map(ROLE)
d['pw'] = d.patch.map(lambda p: 1.0 if p in ('16.17', '16.18') else 0.5)
games = d.groupby(['sfk', 'champ', 'role']).smid.nunique().rename('ng')
d = d.join(games, on=['sfk', 'champ', 'role'])
d['w'] = d.pw * (10.0 / d.ng).clip(upper=1.0)
d['key'] = d.owned.fillna('').map(lambda s: ','.join(sorted(x for x in s.split() if x)))
d['h'] = d.sfk.map(lambda s: zlib.crc32(str(s).encode()) % 100)
train, test = d[d.h >= 15], d[d.h < 15]
if part == 'tune':
    test = test[test.h < 7]
elif part == 'report':
    test = test[test.h >= 7]


def table(g, keycol, minw=3.0, top=12):
    res = {}
    for k, gg in g.groupby(keycol):
        c = gg.groupby('item').w.sum().sort_values(ascending=False)
        if c.sum() >= minw:
            res[str(k)] = {str(i): float(v) for i, v in c.head(top).items()}
    return res


champs = {(champ, role): {'sets': table(g, 'key'), 'nth': table(g, 'nth')}
          for (champ, role), g in train.groupby(['champ', 'role']) if g.smid.nunique() >= 30}
roles = {r: {'sets': table(g, 'key', minw=10), 'nth': table(g, 'nth', minw=10)} for r, g in train.groupby('role')}


def norm(p):
    total = sum(p.values())
    return {i: v / total for i, v in p.items()} if total > 0 else {}


def smooth(counts, prior, a):
    n = sum(counts.values())
    if n + a <= 0:
        return {}
    return {i: (counts.get(i, 0.0) + a * prior.get(i, 0.0)) / (n + a) for i in set(counts) | set(prior)}


SUPPORT, BUILT_WITH = {}, {}
for ck, cr in champs.items():
    support, built = set(), collections.defaultdict(set)
    for t in cr['nth'].values():
        support |= set(t)
    for key, t in cr['sets'].items():
        owned = {x for x in key.split(',') if x}
        support |= owned | set(t)
        for a in owned:
            built[a] |= owned | set(t)
        for b in t:
            built[b] |= owned
    SUPPORT[ck], BUILT_WITH[ck] = support, built

memo = {}


def predict(ck, owned, variant):
    """The smoothed distribution for a sorted tuple of owned ids, owned items not yet removed."""
    if (ck, owned, variant) in memo:
        return memo[(ck, owned, variant)]
    mask, subsets, cobuilt, a_nth = VARIANTS[variant]
    cr, ro = champs[ck], roles.get(ck[1], {'sets': {}, 'nth': {}})
    key, nth = ','.join(owned), str(len(owned) + 1)
    p_role = norm(ro['nth'].get(nth, {}))
    p_role = smooth(ro['sets'].get(key, {}), p_role, A_ROLE) if p_role else {}
    if mask:
        p_role = norm({i: v for i, v in p_role.items() if i in SUPPORT[ck]})
    p_nth = smooth(cr['nth'].get(nth, {}), p_role, a_nth)
    if cobuilt and owned:
        with_owned = set().union(*[BUILT_WITH[ck].get(a, set()) for a in owned])
        p_nth = {i: v if i in with_owned else v * NOT_BUILT_WITH for i, v in p_nth.items()}
    prior = p_nth
    if subsets and len(owned) >= 2:
        smaller, sets = collections.Counter(), 0
        for skip in owned:
            p = norm({i: v for i, v in predict(ck, tuple(x for x in owned if x != skip), variant).items()
                      if i not in owned})
            if p:
                sets += 1
                smaller.update(p)
        if sets:
            step = norm({i: v for i, v in p_nth.items() if i not in owned})
            prior = {i: SUBSET_SHARE * smaller.get(i, 0.0) / sets + (1 - SUBSET_SHARE) * step.get(i, 0.0)
                     for i in set(smaller) | set(step)}
    memo[(ck, owned, variant)] = p = smooth(cr['sets'].get(key, {}), prior, A_SET)
    return p


score = collections.defaultdict(collections.Counter)
off = collections.Counter()
for r in test.itertuples():
    ck = (r.champ, r.role)
    if ck not in champs:
        continue
    owned = tuple(sorted(x for x in r.key.split(',') if x))
    groups = ('all', 'set in the champion table' if r.key in champs[ck]['sets'] else 'set not in the table',
              f'item {min(r.nth, 5)}')
    for variant in VARIANTS:
        p = norm({i: v for i, v in predict(ck, owned, variant).items() if i not in owned and v > 0})
        ranked = [i for i, _ in sorted(p.items(), key=lambda x: (-x[1], x[0]))]
        item = str(r.item)
        for group in groups:
            c = score[(variant, group)]
            c['n'] += 1
            c['top1'] += ranked[:1] == [item]
            c['top3'] += item in ranked[:3]
            c['loss'] += -math.log(max(p.get(item, 0.0), 1e-4))
        if variant == 'previous':
            off['n'] += 1
            off['mass'] += sum(v for i, v in p.items() if i not in SUPPORT[ck])
            off['top1'] += bool(ranked) and ranked[0] not in SUPPORT[ck]

n = score[('previous', 'all')]['n']
print(f'{part}: {len(champs)} champion-roles, {n} held-out decisions')
print(f"previous model: {off['mass'] / off['n']:.1%} of the probability on items the champion never builds; "
      f"first choice is such an item in {off['top1'] / off['n']:.1%} of decisions")
for group in ('all', 'set in the champion table', 'set not in the table', 'item 1', 'item 2', 'item 3', 'item 4', 'item 5'):
    print(f"\n{group} (n={score[('previous', group)]['n']})")
    for variant in VARIANTS:
        c = score[(variant, group)]
        print(f"  {variant:9s} top-1 {c['top1'] / c['n']:.3f}  top-3 {c['top3'] / c['n']:.3f}  log loss {c['loss'] / c['n']:.3f}")
