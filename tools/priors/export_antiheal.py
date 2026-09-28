"""How much each enemy champion makes Master+ players buy anti-heal, for Recall's anti-heal need.
Logistic regression over ~107k ranked Master+ player-games (Kaggle ranked-timeline, 16.13-16.18; games of 15+
minutes): P(the player owns a Grievous Wounds item at any minute) ~ the player's role + the five enemy
champions. Each champion's odds ratio is relative to the median champion, so co-occurring healers are separated
(Garen 0.8: nobody reacts to his regeneration; Soraka 6.8). weight = clamp((odds ratio - 1) / 2, 0, 1): 1 for
Soraka, Warwick and Aatrox, about 0.8 for Zac, 0 for Garen. Champions under 500 appearances are left out
(the engine falls back to the healing trait).
Usage: export_antiheal.py <MatchTimelineTbl.csv> <players.parquet> <out.json>"""
import json, re, sys
import duckdb, numpy as np, pandas as pd
from scipy import sparse
from sklearn.linear_model import LogisticRegression
timeline, players, out = sys.argv[1:4]
GW = (3011, 3033, 3123, 3165, 3916, 3075, 3076, 6609)  # every Summoner's Rift item applying Wounds on 16.19
MIN_SEEN, REF_ODDS = 500, 3.0
ENEMY = ['e_TOP', 'e_JUNGLE', 'e_MIDDLE', 'e_BOTTOM', 'e_UTILITY']
_norm = lambda s: re.sub(r'[^a-z0-9]', '', str(s).lower())
con = duckdb.connect()
owned = ' or '.join(f'Item{i} in {GW}' for i in range(7))
con.execute(f"create table gw as select distinct SummonerMatchFk smid from read_csv_auto('{timeline}') where {owned}")
df = con.execute(f"select p.*, gw.smid is not null as gw from read_parquet('{players}') p left join gw using (smid) "
                 f"where p.dur >= 900").df()
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
model = LogisticRegression(C=1.0, max_iter=2000).fit(X, df.gw.values)
coef = pd.Series(model.coef_[0][:len(champs)], index=champs)
odds = np.exp(coef - coef.median())
seen = pd.concat([df[c].map(_norm) for c in ENEMY]).value_counts()
champions = {c: {'odds': round(float(odds[c]), 3), 'seen': int(seen[c]),
                 'weight': round(float(min(1.0, max(0.0, (odds[c] - 1) / (REF_ODDS - 1)))), 3)}
             for c in champs if seen.get(c, 0) >= MIN_SEEN}
json.dump({'source': 'Kaggle ranked-timeline, Master+ 16.13-16.18; tools/priors/export_antiheal.py',
           'champions': champions}, open(out, 'w'), separators=(',', ':'), sort_keys=True)
print(len(champions), 'champions', file=sys.stderr)
