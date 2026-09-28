"""CS benchmarks for Recall's post-game recap, from the ranked-timeline tables (Kaggle, Master+ 16.13-16.18).
Per champion and role: median creep score (lane minions + jungle monsters, as the Live Client's creepScore counts
them) at 10 minutes and median CS per minute over the game, from games of at least 15 minutes. Keys are the Data
Dragon display name, lowercased to letters and digits, and the role label ("xayah|ADC"); champion-roles under
100 games are left to the role's value.
Usage: export_cs.py <MatchTimelineTbl.csv> <players.parquet> <champion.json> <out.json>"""
import json, re, sys
import duckdb
timeline, players, champion_json, out = sys.argv[1:5]
MIN_GAMES = 100
ROLE = {'TOP': 'Top', 'JUNGLE': 'Jungle', 'MIDDLE': 'Mid', 'BOTTOM': 'ADC', 'UTILITY': 'Support'}
_norm = lambda s: re.sub(r'[^a-z0-9]', '', str(s).lower())
NAME = {int(c['key']): _norm(c['name']) for c in json.load(open(champion_json, encoding='utf-8'))['data'].values()}
con = duckdb.connect()
con.execute(f"create table t as select SummonerMatchFk smid, Minute m, MinionsKilled + JungleMinionsKilled cs "
            f"from read_csv_auto('{timeline}')")
con.execute(f"create table p as select * from read_parquet('{players}')")
con.execute("""create table x as
    with last as (select smid, arg_max(cs, m) cs, max(m) lm from t group by smid),
         t10 as (select smid, cs cs10 from t where m = 10)
    select p.champ_id, p.role, t10.cs10, last.cs * 1.0 / last.lm cspm
    from p join t10 using (smid) join last using (smid) where p.dur >= 900""")
def row(n, cs10, cspm):
    return {'games': int(n), 'cs_at_10': round(float(cs10), 1), 'per_minute': round(float(cspm), 2)}
roles = {ROLE[r]: row(n, a, b) for r, n, a, b in
         con.execute("select role, count(*), median(cs10), median(cspm) from x group by role").fetchall()}
champions = {}
for cid, r, n, a, b in con.execute("select champ_id, role, count(*), median(cs10), median(cspm) from x "
                                   "group by champ_id, role").fetchall():
    if n >= MIN_GAMES and cid in NAME:
        champions[f'{NAME[cid]}|{ROLE[r]}'] = row(n, a, b)
json.dump({'source': 'Kaggle ranked-timeline, Master+ 16.13-16.18; tools/priors/export_cs.py',
           'champions': champions, 'roles': roles}, open(out, 'w'), separators=(',', ':'), sort_keys=True)
print(len(champions), 'champion-roles', roles, file=sys.stderr)
