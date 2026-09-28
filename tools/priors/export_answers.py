"""How much each enemy champion makes Master+ players buy an answer item, for Recall's need terms.
One logistic regression per answer over ~107k ranked Master+ player-games (Kaggle ranked-timeline, 16.13-16.18;
games of 15+ minutes): P(the player owns one of the answer's items at any minute) ~ the player's role + the five
enemy champions. Each champion's odds ratio is relative to the median champion, which separates co-occurring
threats. weight = clamp((odds - floor) / (reference - floor), 0, 1). This replaces hand-written trait tags, which were
wrong in both directions (Garen tagged a healer: 0.8x anti-heal; Lissandra untagged: 5.5x cleanse; AP burst
mages tagged burst: ~1.0x defensive items, against 1.75x for Zed). Champions under 500 appearances are left
out (the engine falls back to the trait tags).
Usage: export_answers.py <MatchTimelineTbl.csv> <players.parquet> <out.json>"""
import json, re, sys
import duckdb, numpy as np, pandas as pd
from scipy import sparse
from sklearn.linear_model import LogisticRegression
timeline, players, out = sys.argv[1:4]
# answer -> (Summoner's Rift items on 16.19, odds ratio that maps to weight 0, odds ratio that maps to weight 1).
# Cleanse items are rare (0.9% of player-games), so 1.5x against Sett is half a percentage point: below 1.5x is
# noise, not a reason.
ANSWERS = {
    'antiheal': ((3011, 3033, 3123, 3165, 3916, 3075, 3076, 6609), 1.0, 3.0),  # every item applying Wounds
    'cleanse': ((3139, 3140), 1.5, 5.0),                                          # Mercurial Scimitar, Quicksilver Sash
    'anti_burst': ((3026, 3157, 2420, 6673, 3814), 1.0, 1.75),                    # GA, Zhonya's, Armguard, Shieldbow, EoN
}
MIN_SEEN = 500
ENEMY = ['e_TOP', 'e_JUNGLE', 'e_MIDDLE', 'e_BOTTOM', 'e_UTILITY']
_norm = lambda s: re.sub(r'[^a-z0-9]', '', str(s).lower())
con = duckdb.connect()
con.execute(f"create table tl as select SummonerMatchFk smid, Item0, Item1, Item2, Item3, Item4, Item5, Item6 "
            f"from read_csv_auto('{timeline}')")
df = con.execute(f"select * from read_parquet('{players}') where dur >= 900").df()
champs = sorted({_norm(x) for c in ENEMY for x in df[c]})
ci = {c: i for i, c in enumerate(champs)}
roles = sorted(df.role.unique())
ri = {r: i for i, r in enumerate(roles)}
rows, cols = [], []
for k, r in enumerate(df[ENEMY + ['role']].itertuples(index=False)):
    for name in r[:5]:
        rows.append(k); cols.append(ci[_norm(name)])
    rows.append(k); cols.append(len(champs) + ri[r[5]])
X = sparse.csr_matrix((np.ones(len(rows)), (rows, cols)), shape=(len(df), len(champs) + len(roles)))
seen = pd.concat([df[c].map(_norm) for c in ENEMY]).value_counts()
doc = {'source': 'Kaggle ranked-timeline, Master+ 16.13-16.18; tools/priors/export_answers.py'}
for answer, (items, floor, reference) in ANSWERS.items():
    owned = ' or '.join(f'Item{i} in {items}' for i in range(7))
    has = set(con.execute(f"select distinct smid from tl where {owned}").df().smid)
    model = LogisticRegression(C=1.0, max_iter=3000).fit(X, df.smid.isin(has).values)
    coef = pd.Series(model.coef_[0][:len(champs)], index=champs)
    odds = np.exp(coef - coef.median())
    doc[answer] = {c: {'odds': round(float(odds[c]), 3), 'seen': int(seen[c]),
                       'weight': round(float(min(1.0, max(0.0, (odds[c] - floor) / (reference - floor)))), 3)}
                   for c in champs if seen.get(c, 0) >= MIN_SEEN}
    print(answer, len(doc[answer]), 'champions', file=sys.stderr)
json.dump(doc, open(out, 'w'), separators=(',', ':'), sort_keys=True)
