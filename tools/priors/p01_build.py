"""Build the decision table from ranked-timeline (rt.duckdb, derived events ev3).

One row per completed item of a tracked player-game (legendary, or the first tier-2 boots), patches 16.13-16.18,
games >= 15 min, known lane, Viego excluded (possession artifacts). For each row:
  - target item (normalized: 3097->3095 Stormrazor; tear transforms stay as their base id)
  - nth legendary, owned legendary set before (sold/undone removed), boots state before
  - decision-start state (frame at the previous completion minute; for the first completion min(6, m-1))
    and at-purchase state (frame m-1); only Live-Client-computable fields plus oracle gold diffs for ablation
  - enemy/ally composition by role and lane opponent (from TeamMatchTbl), keystone
Output: data/dec.parquet, data/players.parquet"""
import json, os, collections
import numpy as np, pandas as pd, duckdb
H = os.path.dirname(os.path.abspath(__file__)); V3 = os.path.dirname(H)
con = duckdb.connect(V3 + '/rt.duckdb', read_only=True)
PATCHES = ('16.13', '16.14', '16.15', '16.16', '16.17', '16.18')
ROLES = ['TOP', 'JUNGLE', 'MIDDLE', 'BOTTOM', 'UTILITY']
NORM = {3097: 3095, 3042: 3004, 3040: 3003, 3121: 3119, 2530: 2526}
TRINKET = {3340, 3363, 3364, 3330}
XP = [0, 280, 660, 1140, 1720, 2400, 3180, 4060, 5040, 6120, 7300, 8580, 9960, 11440, 13020, 14700, 16480, 18360]
def lvl(xp): return int(np.searchsorted(XP, xp, side='right'))

pl = con.execute(f"""
select pm.smid, pm.mid, pm.sfk, pm.cname champ, pm.lane as "role", pm.patch, pm.dur, pm.win,
  ms.PrimaryKeyStone ks, ms.SummonerSpell1 sp1, ms.SummonerSpell2 sp2,
  tm.B1Champ,tm.B2Champ,tm.B3Champ,tm.B4Champ,tm.B5Champ,tm.R1Champ,tm.R2Champ,tm.R3Champ,tm.R4Champ,tm.R5Champ, pm.champ champ_id
from pm join MatchStatsTbl ms on ms.SummonerMatchFk=pm.smid join TeamMatchTbl tm on tm.MatchFk=pm.mid
where pm.patch in {PATCHES} and pm.dur>=900 and pm.lane in ('TOP','JUNGLE','MIDDLE','BOTTOM','UTILITY') and pm.cname<>'Viego'
""").df()
cname = dict(con.execute("select ChampionId, ChampionName from ChampionTbl").fetchall())
B = pl[[f'B{i}Champ' for i in range(1, 6)]].values; R = pl[[f'R{i}Champ' for i in range(1, 6)]].values
blue = (B == pl.champ_id.values[:, None]).any(1)
ally = np.where(blue[:, None], B, R); enemy = np.where(blue[:, None], R, B)
for i, r in enumerate(ROLES):
    pl['e_' + r] = [cname.get(x, '') for x in enemy[:, i]]
    pl['a_' + r] = [cname.get(x, '') for x in ally[:, i]]
pl['blue'] = blue.astype(int)
pl['opp'] = [pl.at[k, 'e_' + r] for k, r in zip(pl.index, pl.role)]
pl = pl.drop(columns=[f'B{i}Champ' for i in range(1, 6)] + [f'R{i}Champ' for i in range(1, 6)])
print('players', len(pl), 'summoners', pl.sfk.nunique())

cls = con.execute(f"select patch, item, cls, total from itemcls where patch in {PATCHES}").df()
CLS = {(p, i): c for p, i, c in zip(cls.patch, cls.item, cls.cls)}
PRICE = {(p, i): t for p, i, t in zip(cls.patch, cls.item, cls.total)}
ev = con.execute(f"""
select e.smid, e.minute, e.kind, e.item, e.net_cost from ev3 e join pm on pm.smid=e.smid
where pm.patch in {PATCHES} and pm.dur>=900 and e.kind in ('buy','complete','complete_virtual','phantom_complete','sold','undo')
order by e.smid, e.minute""").df()
ev['patch'] = ev.smid.map(dict(zip(pl.smid, pl.patch)))
ev = ev[ev.patch.notna()]
ev['cls'] = [CLS.get((p, i), 'other') if i < 10000 else 'other' for p, i in zip(ev.patch, ev.item)]
ev = ev[ev.cls.isin(['legendary', 'boots2'])]
ev['item'] = ev.item.map(lambda i: NORM.get(i, i))

rows = []
for smid, g in ev.groupby('smid', sort=False):
    owned = []; boots = None; nth = 0; prev_m = None; nb = 0
    recs = list(zip(g.minute, g.kind, g.item, g.cls, g.net_cost))
    # drop completions undone within 2 minutes
    undone = set()
    for k, (m, kind, it, c, cost) in enumerate(recs):
        if kind == 'undo':
            for j in range(k - 1, -1, -1):
                mj, kj, ij, _, _ = recs[j]
                if ij == it and kj in ('buy', 'complete', 'complete_virtual', 'phantom_complete') and j not in undone and m - mj <= 2:
                    undone.add(j); break
    # sells first within a minute
    order = sorted(range(len(recs)), key=lambda k: (recs[k][0], 0 if recs[k][1] in ('sold', 'undo') else 1, -recs[k][4]))
    for k in order:
        m, kind, it, c, cost = recs[k]
        if kind in ('sold', 'undo'):
            if c == 'legendary' and it in owned: owned.remove(it)
            if c == 'boots2' and boots == it: boots = None
            continue
        if k in undone: continue
        if c == 'boots2':
            if boots is not None or nb > 0:  # only the first tier-2 boots is a decision target
                boots = it; continue
            nb += 1
            rows.append((smid, m, 'boots', it, nth, tuple(owned), None, prev_m, kind))
            boots = it; prev_m = m
            continue
        if it in owned:  # duplicate legendary (rare, e.g. list artifacts) -> skip as target
            continue
        nth += 1
        rows.append((smid, m, 'leg', it, nth, tuple(owned), boots, prev_m, kind))
        owned.append(it); prev_m = m
D = pd.DataFrame(rows, columns=['smid', 'minute', 'type', 'item', 'nth', 'owned', 'boots_before', 'prev_m', 'kind'])
# boots_before for boots rows is None by construction; nleg_before for all
D['nleg_before'] = D.owned.map(len)
D['last'] = D.owned.map(lambda o: o[-1] if o else 0)
D['s_min'] = [int(p) if p == p and p is not None else int(min(6, m - 1)) for p, m in zip(D.prev_m, D.minute)]
D['s_min'] = np.minimum(D.s_min, D.minute - 1).clip(lower=1)
D['p_min'] = (D.minute - 1).clip(lower=1)
print('decision rows', len(D), D.type.value_counts().to_dict())

# frames
need = set(zip(D.smid, D.s_min)) | set(zip(D.smid, D.p_min)) | set(zip(D.smid, (D.s_min - 5).clip(lower=0))) | set(zip(D.smid, (D.p_min - 5).clip(lower=0)))
fr = con.execute(f"""select t.* exclude (TimelineId, Win) from MatchTimelineTbl t join pm on pm.smid=t.SummonerMatchFk
  where pm.patch in {PATCHES} and pm.dur>=900""").df()
fr = fr.rename(columns={'SummonerMatchFk': 'smid', 'Minute': 'min'})
fr['patch'] = fr.smid.map(dict(zip(pl.smid, pl.patch)))
fr = fr[fr.patch.notna()]
val = np.zeros(len(fr))
for c in [f'Item{i}' for i in range(7)]:
    val += np.array([0 if (i == 0 or i in TRINKET) else PRICE.get((p, i), 0) if CLS.get((p, i)) != 'consumable' else 0
                     for p, i in zip(fr.patch, fr[c])])
fr['item_val'] = val
fr['nleg_vis'] = sum(np.array([CLS.get((p, i)) == 'legendary' for p, i in zip(fr.patch, fr[c])]) for c in [f'Item{i}' for i in range(7)])
fr['cs'] = fr.MinionsKilled + fr.JungleMinionsKilled
fr['opp_level'] = [lvl(x) for x in (fr.Xp - fr.XpDiff).clip(lower=0)]
keep = ['smid', 'min', 'Level', 'opp_level', 'CurrentGold', 'TotalGold', 'item_val', 'cs', 'Kills', 'Deaths', 'Assists',
        'TeamKills', 'EnemyKills', 'TeamTowers', 'EnemyTowers', 'TeamDragons', 'EnemyDragons', 'GoldDiff', 'XpDiff', 'TeamGoldDiff',
        'DmgDealtToChampions', 'DmgTaken']
fr = fr[keep].set_index(['smid', 'min'])
def attach(df, mcol, suf):
    x = fr.reindex(pd.MultiIndex.from_arrays([df.smid, df[mcol]])).reset_index(drop=True)
    x5 = fr.reindex(pd.MultiIndex.from_arrays([df.smid, (df[mcol] - 5).clip(lower=0)]))[['Deaths', 'Kills', 'DmgTaken']].reset_index(drop=True)
    x['deaths5'] = x.Deaths - x5.Deaths; x['kills5'] = x.Kills - x5.Kills; x['taken5'] = x.DmgTaken - x5.DmgTaken
    x.columns = [c + suf for c in x.columns]
    return x
D = pd.concat([D.reset_index(drop=True), attach(D, 's_min', '_d'), attach(D, 'p_min', '_p')], axis=1)
D = D.merge(pl, on='smid', how='inner')
D['owned'] = D.owned.map(lambda o: ' '.join(map(str, o)))
D.to_parquet(H + '/data/dec.parquet')
pl.to_parquet(H + '/data/players.parquet')
print(D.groupby(['type', 'role']).size())
print(D[D.type == 'leg'].groupby('patch').size())
