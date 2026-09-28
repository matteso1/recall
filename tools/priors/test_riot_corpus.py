"""Synthetic split, observation, and chronological label contracts."""
import unittest
from collections import Counter
from riot_corpus import assign_match, item_kinds, labels_after, make_cases, player_split, snapshot_player


class CorpusContracts(unittest.TestCase):
    def test_cheap_finished_items_are_not_counted_as_components(self):
        catalog = {'100':dict(gold=dict(total=350,base=350,purchasable=True),maps={'11':True},into=['200']),
                   '200':dict(gold=dict(total=1500,base=1150,purchasable=True),maps={'11':True},**{'from':['100']})}
        self.assertEqual(item_kinds(catalog),{100:'component',200:'finished'})

    def test_held_out_seed_excludes_whole_match_even_as_peer(self):
        seeds = {f"anonymous{i}": player_split(f"anonymous{i}") for i in range(50)}
        val = next(p for p,s in seeds.items() if s == "validation")
        test = next(p for p,s in seeds.items() if s == "test")
        train = next(p for p,s in seeds.items() if s == "train")
        self.assertEqual(assign_match([train, val, "peer"], seeds), "validation")
        self.assertEqual(assign_match([test, val], seeds), "excluded")
        self.assertEqual(assign_match([train, "peer"], seeds), "train")

    def test_labels_are_strictly_future_and_undo_is_excluded(self):
        purchases = [dict(timestamp=t, item=i, retained=ok) for t,i,ok in
                     [(60000,100,True),(61000,200,False),(62000,300,True),(121000,400,True)]]
        labels = labels_after(purchases, 60000, 120000, {300: "leg",400: "boots"})
        self.assertEqual(labels, [dict(horizon="purchase", item=300, kind="leg"),
                                  dict(horizon="shop", item=300, kind="buy", component=False)])

    def test_same_timestamp_shopping_labels_remain_ambiguous(self):
        purchases = [dict(timestamp=61000,item=i,retained=True) for i in (100,200)]
        self.assertEqual(len([l for l in labels_after(purchases,60000,120000,{}) if l['kind']=='buy']), 2)

    def test_snapshot_does_not_expose_enemy_gold_stats_or_position(self):
        player = dict(participantId=2,teamPosition="TOP",teamId=200,championName="Fixture")
        frame = dict(level=4,minionsKilled=20,jungleMinionsKilled=0,currentGold=9999,
                     position=dict(x=123,y=456),damageStats=dict(totalDamageDone=999),championStats=dict(armor=500))
        result = snapshot_player(player,frame,dict(items=[100,100],role_slot_boots=None),[1,2,3],
                                 {"100":dict(name="Part")},{"Fixture":"Fixture"})
        self.assertEqual(len(result['items']),2)
        self.assertEqual(result['position'],'TOP')
        self.assertNotIn('currentGold',result)
        self.assertNotIn('championStats',result)
        self.assertEqual(set(result),{'name','champion','team','position','level','items','kills','deaths','assists','cs'})

    def test_future_kills_skills_and_final_stats_do_not_fill_earlier_frames(self):
        players = [dict(participantId=p,teamId=100 if p<=5 else 200,teamPosition='TOP',
                        championName=f'Champion{p}',summoner1Id=4,summoner2Id=12,kills=99)
                   for p in range(1,11)]
        frames = [dict(timestamp=t,participantFrames={str(p):dict(level=6,currentGold=500,
                                                                 minionsKilled=30,jungleMinionsKilled=0)
                                                       for p in range(1,11)},events=[])
                  for t in (360000,420000)]
        frames[1]['events'] = [dict(type='CHAMPION_KILL',timestamp=400000,killerId=6,victimId=1,
                                   assistingParticipantIds=[7]),
                               dict(type='SKILL_LEVEL_UP',timestamp=400001,participantId=1,skillSlot=1)]
        history = {p:dict(issues=[],reconciled=True,purchases=[],
                          frames=[dict(items=[],role_slot_boots=None,exact=True) for _ in frames])
                   for p in range(1,11)}
        result = make_cases(dict(info=dict(participants=players)),dict(info=dict(frames=frames)),
                            dict(players=history),players[0],{}, {}, {}, Counter())
        self.assertEqual(result[0]['snapshot']['me']['player']['kills'],0)
        self.assertEqual(result[0]['snapshot']['me']['abilities']['q'],0)
        self.assertEqual(result[0]['snapshot']['my_deaths'],[])
        self.assertEqual(result[1]['snapshot']['me']['player']['deaths'],1)
        self.assertEqual(result[1]['snapshot']['me']['abilities']['q'],1)
        self.assertEqual(result[1]['snapshot']['my_deaths'][0]['killer'],'Champion6')
        history[10]['frames'][0]['exact'] = False
        result = make_cases(dict(info=dict(participants=players)),dict(info=dict(frames=frames)),
                            dict(players=history),players[0],{}, {}, {}, Counter())
        self.assertEqual(len(result),1)
        report = Counter()
        result = make_cases(dict(info=dict(participants=players)),dict(info=dict(frames=frames)),
                            dict(players=history),players[0],{}, {}, {}, report, 'known-peers')
        self.assertEqual(len(result),2)
        self.assertEqual(len(result[0]['snapshot']['enemies']),4)
        self.assertEqual(report['known_enemy_frames'],9)
        self.assertEqual(report['possible_enemy_frames'],10)
        self.assertEqual(report['complete_team_frames'],1)
        history[1]['frames'][0]['exact'] = False
        result = make_cases(dict(info=dict(participants=players)),dict(info=dict(frames=frames)),
                            dict(players=history),players[0],{}, {}, {}, Counter(), 'known-peers')
        self.assertEqual(len(result),1, 'Partial peers must never permit an unknown own inventory')


if __name__ == '__main__':
    unittest.main()
