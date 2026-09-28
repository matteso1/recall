import json, duckdb
d=json.load(open('/mnt/c/Users/nilsm/AppData/Local/Recall/ddragon/16.19.1/item.json',encoding='utf-8'))['data']
con=duckdb.connect('rt.duckdb')
con.execute("create or replace table dd(id bigint, name varchar, gold_total int, gold_base int, purchasable bool, sell int, frm varchar[], into_ varchar[], tags varchar[], map11 bool, consumed bool, depth int, required_champion varchar, in_store bool, special_recipe int)")
rows=[]
for k,v in d.items():
    rows.append((int(k), v['name'], v['gold']['total'], v['gold']['base'], v['gold']['purchasable'], v['gold']['sell'], v.get('from',[]), v.get('into',[]), v.get('tags',[]), v['maps'].get('11',False), v.get('consumed',False), v.get('depth'), v.get('requiredChampion'), v.get('inStore',True), v.get('specialRecipe')))
con.executemany("insert into dd values (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", rows)
print(con.execute("select count(*) from dd").fetchone())
# long timeline items
con.execute("""create or replace table tl_items as
select SummonerMatchFk, Minute, unnest([Item0,Item1,Item2,Item3,Item4,Item5,Item6]) item, unnest([0,1,2,3,4,5,6]) slot from MatchTimelineTbl""")
print(con.execute("select count(*), count(*) filter (where item<>0) from tl_items").fetchone())
