"""Answer-model research must not turn missing histories into negative labels."""
import csv
from contextlib import redirect_stdout
import io
from pathlib import Path
import tempfile
import unittest

import pandas as pd

from evaluate_answers import evaluate, features, observed_rows, read_labels, weights_from_coefficients
from export_answers import ANSWERS, ENEMY


class AnswerEvaluationContracts(unittest.TestCase):
    def test_absent_timeline_is_excluded_but_observed_nonbuyer_is_negative(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'timeline.csv'
            with path.open('w') as stream:
                writer = csv.writer(stream)
                writer.writerow(['SummonerMatchFk', *[f'Item{i}' for i in range(7)]])
                writer.writerows([[1, *[0]*7], [2, 3123, *[0]*6], [2, *[0]*7]])
            frame = pd.DataFrame({'smid':[1,2,3]})
            labels = read_labels(path, frame[['smid']])
            rows, missing = observed_rows(frame, labels)
            self.assertEqual(missing, 1)
            self.assertEqual(rows.smid.tolist(), [1,2])
            self.assertEqual(rows.antiheal.tolist(), [False,True])
            self.assertFalse(rows.cleanse.any())
            self.assertEqual(set(labels.columns), set(ANSWERS))

    def test_predictors_ignore_identity_outcomes_and_eventual_build(self):
        row = dict(role='BOTTOM',champ='Xayah',patch='16.19',
                   **dict(zip(ENEMY,['A','B','C','D','E'])))
        first = row | dict(smid=1,sfk=2,win=True,dur=1800,antiheal=True,item=3123)
        second = row | dict(smid=9,sfk=8,win=False,dur=2100,antiheal=False,item=0)
        a,b = features(pd.DataFrame([first,second]))
        self.assertEqual(a,b)
        self.assertEqual(a['own:xayah'],1)
        changed = row | dict(champ='Nilah')
        self.assertNotEqual(a,features(pd.DataFrame([changed]))[0])

    def test_control_coefficients_do_not_change_enemy_normalization(self):
        enemy = {'enemy:ashe':0., 'enemy:zed':1., 'enemy:soraka':2.}
        seen = pd.Series({'ashe':600,'zed':600,'soraka':400})
        a = weights_from_coefficients(enemy,seen,1.,3.)
        b = weights_from_coefficients(enemy | {'own:xayah':100.,'role:BOTTOM':200.},seen,1.,3.)
        self.assertEqual(a,b)
        self.assertEqual(a['zed']['odds'],1.)
        self.assertNotIn('soraka',a,'Sparse enemies retain the existing fallback')

    def test_fitting_returns_selected_candidate_and_comparisons_together(self):
        rows = [dict(role='BOTTOM',champ='Xayah',patch='16.18',sfk=i,
                     **dict(zip(ENEMY,['A','B','C','D','E'])),
                     **{answer:bool(i%2) for answer in ANSWERS}) for i in range(20)]
        with redirect_stdout(io.StringIO()):
            report, candidate = evaluate(pd.DataFrame(rows[:16]),pd.DataFrame(rows[16:]))
        self.assertEqual(set(candidate),{'source',*ANSWERS})
        self.assertEqual(set(report['comparisons']),set(ANSWERS))
        self.assertEqual(candidate['cleanse'],{},'Tiny samples must not create enemy weights')


if __name__ == '__main__':
    unittest.main()
