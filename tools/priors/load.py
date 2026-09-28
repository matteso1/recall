import duckdb, time
D='/home/nilsm/data/recall/kaggle/ranked-timeline/'
con=duckdb.connect('rt.duckdb')
for t in ['ChampionTbl','ItemTbl','MatchStatsTbl','MatchTbl','MatchTimelineTbl','RankTbl','SummonerMatchTbl','TeamMatchTbl']:
    t0=time.time()
    con.execute(f"create or replace table {t} as select * from read_csv('{D}{t}.csv', header=true, sample_size=-1)")
    n=con.execute(f"select count(*) from {t}").fetchone()[0]
    print(t,n,round(time.time()-t0,1))
    print(con.execute(f"describe {t}").fetchall())
