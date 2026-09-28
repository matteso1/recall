"""Pass 3: item events from per-minute inventory diffs, reconciled with gold spent per minute.
spent_obs = dTotalGold - dCurrentGold. Per-patch Data Dragon recipes/prices (16.10-16.18).
Removal classes: component (used by an added item), consumed (consumable), trinket_out, sold (refund = sell value),
undo (refund = full cost, bought <=2 min earlier), transform (no refund; id changed without an event: tier-3 boots,
Muramana/Seraph/Fimbulwinter, Shattered Armguard, bot-lane boots quest). Transformed ids stay usable as virtual
components (e.g. 3157 Zhonya from a transformed 2420; bot boots upgrade after the quest). Magical Footwear (rune 8304)
gives a virtual 1001. Control Ward may cost 40 (support). Positive leftovers are tested against hidden consumables
(bought and used inside the minute) and against a 'phantom' completed item that the 7-entry list dropped."""
import json, duckdb, itertools
from collections import Counter
import pandas as pd
con=duckdb.connect('rt.duckdb')
DD={}
for v in ['16.10','16.11','16.12','16.13','16.14','16.15','16.16','16.17','16.18']:
    DD[v]={int(k):x for k,x in json.load(open(f'ddragon/item-{v}.1.json',encoding='utf-8'))['data'].items()}
TR={3340,3363,3364,3330}
CONSUMABLE={2003,2055,2138,2139,2140,2150,2151,2152,2010}
TOL=10
def cost(p,i): d=DD[p].get(i); return d['gold']['total'] if d else 0
def sellv(p,i): d=DD[p].get(i); return d['gold']['sell'] if d else 0
def frm(p,i): d=DD[p].get(i); return [int(x) for x in d.get('from',[])] if d else []
def into(p,i): d=DD[p].get(i); return [int(x) for x in d.get('into',[])] if d else []
def consume(p,item,pools,used,depth=0):
    for c in frm(p,item):
        hit=False
        for name,pool in pools:
            if pool[c]>0:
                pool[c]-=1; used.append((name,c)); hit=True; break
        if not hit and depth<4:
            consume(p,c,pools,used,depth+1)
HIDDEN=sorted({a*50+b*40+c*75+d*500 for a in range(4) for b in range(3) for c in range(3) for d in range(2)} - {0})
mf=set(r[0] for r in con.execute("select SummonerMatchFk from MatchStatsTbl where 8304 in (PrimaryKeyStone,PrimarySlot1,PrimarySlot2,PrimarySlot3,SecondarySlot1,SecondarySlot2)").fetchall())
rows=con.execute("""
select t.SummonerMatchFk, t.Minute, t.TotalGold, t.CurrentGold, [Item0,Item1,Item2,Item3,Item4,Item5,Item6],
 split_part(m.Patch,'.',1)||'.'||split_part(m.Patch,'.',2) patch
from MatchTimelineTbl t join SummonerMatchTbl s on s.SummonerMatchId=t.SummonerMatchFk join MatchTbl m on m.MatchId=s.MatchFk
order by t.SummonerMatchFk, t.Minute""").fetchall()
def solve(patch, A, R, V, obs, minute, lastbuy, allow_virtual):
    Rw=Counter(R); Vw=Counter(V)
    out=[]; costs=0
    for x in sorted(A.elements(), key=lambda i:-cost(patch,i)):
        used=[]
        pools=[('real',Rw)]+([('virtual',Vw)] if allow_virtual else [])
        consume(patch,x,pools,used)
        c=cost(patch,x)-sum(cost(patch,u) for _,u in used)
        kind='trinket' if x in TR else ('complete' if used else 'buy')
        if any(n=='virtual' for n,_ in used): kind+='_virtual'
        out.append([kind,x,[u for _,u in used],c]); costs+=c
    fixed=[]; other=[]
    for r in Rw.elements():
        if r in TR: fixed.append(['trinket_out',r,[],0])
        elif r in CONSUMABLE: fixed.append(['consumed',r,[],0])
        else: other.append(r)
    r0=obs-costs
    n_cw=sum(1 for k,x,_,_ in out if x==2055)
    def best_combo(others, r0):
        opts=[]
        for r in others:
            o=[('transform',0),('sold',sellv(patch,r))]
            if lastbuy.get(r,-9)>=minute-2: o.append(('undo',cost(patch,r)))
            opts.append(o)
        best=None
        it=itertools.product(*opts) if len(others)<=7 else [tuple(('sold',sellv(patch,r)) for r in others)]
        for combo in it:
            for k_cw in range(n_cw+1):
                res=r0+sum(v for _,v in combo)+35*k_cw
                key=(abs(res)>TOL, sum(1 for k,_ in combo if k=='transform'), k_cw, abs(res))
                if best is None or key<best[0]: best=(key,combo,res,k_cw)
        return best
    best=best_combo(other, r0); phantom=None
    if best[0][0] and other:
        # list overflow: a completed item was bought but never entered the 7-entry list; its components still left
        cands=set()
        for r in other: cands.update(into(patch,r))
        for X in sorted(cands):
            pool=Counter(other); used=[]
            consume(patch,X,[('rem',pool)],used)
            if not used: continue
            c=cost(patch,X)-sum(cost(patch,u) for _,u in used)
            left=list(pool.elements())
            b2=best_combo(left, r0-c)
            if not b2[0][0]:
                best=b2; phantom=(X,[u for _,u in used],c); other=left; break
    _,combo,res,k_cw=best
    if phantom:
        X,used,c=phantom
        out.append(['phantom_complete',X,used,c])
        for u in used: fixed.append(['component',u,[],0])
    for r,(k,v) in zip(other,combo):
        fixed.append([k,r,[],-v])
        if k=='transform': Vw[r]+=1
    if k_cw:
        n=0
        for o in out:
            if o[1]==2055 and n<k_cw: o[3]=40; n+=1
    note=''
    if res>TOL:
        for h in HIDDEN:
            if abs(res-h)<=TOL: note='hidden_consumable'; res=0; break
    return out, fixed, res, Vw, note
ev=[]; mins=[]
prev=None
for smid, minute, tg, cg, items, patch in rows:
    if patch not in DD: prev=None; continue
    if prev is None or prev[0]!=smid:
        prev=(smid, minute, tg, cg, items); V=Counter(); lastbuy={}
        if smid in mf: V[1001]+=1
        continue
    _, pm, ptg, pcg, pitems = prev
    prev=(smid, minute, tg, cg, items)
    a=Counter(i for i in items if i); b=Counter(i for i in pitems if i)
    A=a-b; R=b-a
    obs=(tg-ptg)-(cg-pcg)
    prev_n=sum(1 for i in pitems if i)
    if not A and not R:
        res=obs; note=''
        if res>TOL:
            for h in HIDDEN:
                if abs(res-h)<=TOL: note='hidden_consumable'; res=0; break
        mins.append((smid, minute, obs, res, 0, 0, prev_n, note)); continue
    s1=solve(patch,A,R,V,obs,minute,lastbuy,False)
    s2=solve(patch,A,R,V,obs,minute,lastbuy,True) if V else s1
    out,fixed,res,Vn,note = s1 if abs(s1[2])<=abs(s2[2]) else s2
    V=Vn
    for k,x,used,c in out:
        lastbuy[x]=minute
    for k,x,used,c in out+fixed:
        ev.append((smid, minute, k, x, used, c, prev_n))
    mins.append((smid, minute, obs, res, sum(A.values()), sum(R.values()), prev_n, note))
df=pd.DataFrame(ev, columns=['smid','minute','kind','item','used','net_cost','prev_n'])
con.execute("create or replace table ev3 as select * from df")
dm=pd.DataFrame(mins, columns=['smid','minute','spent_obs','resid','n_added','n_removed','prev_n','note'])
con.execute("create or replace table gold3 as select * from dm")
print(con.execute("select kind, count(*) from ev3 group by 1 order by 2 desc").fetchall())
print(con.execute("select (n_added+n_removed>0) ch, count(*), avg((abs(resid)<=10)::int), avg((abs(resid)<=50)::int) from gold3 group by 1").fetchall())
