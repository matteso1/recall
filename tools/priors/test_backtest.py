"""Contracts for the corpus evaluator, using small anonymous fixtures."""
import unittest
from types import SimpleNamespace
import pandas as pd

from backtest import split_players, training_aggregates, compare_reports, make_player


class EvaluationContracts(unittest.TestCase):
    def test_overfull_inventory_is_rejected_instead_of_hiding_real_equipment(self):
        player = SimpleNamespace(role='JUNGLE', blue=True)
        frame = SimpleNamespace(Level=18, Kills=0, Deaths=0, Assists=0,
                                MinionsKilled=100, JungleMinionsKilled=100,
                                **{f'Item{i}': n for i,n in enumerate([2520,6699,3158,6695,3814,2140,3033])})
        with self.assertRaisesRegex(ValueError, 'overfull'):
            make_player(player, frame, 'Qiyana', {})

    def test_compacted_corpus_inventory_keeps_the_sixth_real_item(self):
        player = SimpleNamespace(role='BOTTOM', blue=True)
        frame = SimpleNamespace(Level=12, Kills=0, Deaths=0, Assists=0,
                                MinionsKilled=100, JungleMinionsKilled=0,
                                **{f'Item{i}': n for i,n in enumerate([1086,3032,3008,3363,3046,3031,3036])})
        result = make_player(player, frame, 'Jinx', {'3363': {'tags':['Trinket']}})
        by_id = {i['id']: i['slot'] for i in result['items']}
        self.assertEqual(by_id[3363], 6)
        self.assertLess(by_id[3036], 6, 'compacted position 6 is not necessarily the trinket slot')

    def test_players_and_matches_cannot_cross_training_and_evaluation(self):
        players = pd.DataFrame([
            dict(sfk=i, smid=i, mid=i // 2) for i in range(500)
        ])
        parts = split_players(players)
        for name, left in parts.items():
            for other, right in parts.items():
                if name != other:
                    self.assertFalse(set(left.sfk) & set(right.sfk))
                    self.assertFalse(set(left.mid) & set(right.mid))
        self.assertGreater(len(parts['train']), 200)
        self.assertGreater(len(parts['validation']), 0)
        self.assertGreater(len(parts['test']), 0)

    def test_aggregate_build_uses_joint_observed_order_not_independent_slot_modes(self):
        rows = []
        for game, sequence in enumerate([[100, 200, 300], [100, 200, 300], [400, 500, 600]]):
            for nth, item in enumerate(sequence, 1):
                rows.append(dict(smid=game, champ_id=498, role='BOTTOM', type='leg',
                                 nth=nth, item=item, kind='complete', minute=10*nth))
        aggregate = training_aggregates(pd.DataFrame(rows))['498|ADC']
        self.assertEqual(aggregate['core']['ids'], [100, 200, 300])
        self.assertEqual(aggregate['core']['games'], 2)

    def test_comparison_rejects_different_cases_or_training(self):
        baseline = dict(fingerprint='a', split='validation', groups={})
        with self.assertRaisesRegex(ValueError, 'same'):
            compare_reports(baseline, dict(baseline, fingerprint='b'))


if __name__ == '__main__':
    unittest.main()
