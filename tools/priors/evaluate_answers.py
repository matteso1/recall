"""Compare defensive-item ownership models on held-out legacy-corpus players.

Research only: no production export, no test-set selection, no claim of optimal
item choices. Model selection uses validation; bootstrap intervals are exploratory.
"""
import argparse
import json
from pathlib import Path

import duckdb
import numpy as np
import pandas as pd
from sklearn.feature_extraction import DictVectorizer
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import average_precision_score, brier_score_loss, log_loss

from backtest import file_hash, split_players
from collect_matches import private_output, write_json
from export_answers import ANSWERS, ENEMY, MIN_SEEN, _norm

VARIANTS = {
    'shared': {'role', 'enemy'},
    'champion_control': {'role', 'enemy', 'own', 'patch'},
    'role_interaction': {'role', 'enemy', 'own', 'patch', 'interaction'},
}
STRENGTHS = (0.1, 1.0, 10.0)


def read_labels(timeline, wanted):
    """Missing player histories stay missing; observed nonbuyers are negatives."""
    expressions = []
    for answer, (items, _, _) in ANSWERS.items():
        owns = ' or '.join(f'Item{i} in {items}' for i in range(7))
        expressions.append(f'bool_or({owns}) as {answer}')
    with duckdb.connect() as con:
        con.execute('set threads=2')
        con.register('wanted', wanted.drop_duplicates())
        return con.execute(
            'select t.SummonerMatchFk smid, ' + ','.join(expressions) +
            ' from read_csv_auto(?) t join wanted w on t.SummonerMatchFk=w.smid '
            'group by t.SummonerMatchFk', [str(timeline)]).df().set_index('smid')


def observed_rows(frame, labels):
    joined = frame.join(labels, on='smid', validate='one_to_one')
    known = joined[list(ANSWERS)].notna().all(axis=1)
    return joined[known].copy(), int((~known).sum())


def features(frame):
    result = []
    for row in frame.itertuples(index=False):
        values = {'role:'+row.role: 1, 'own:'+_norm(row.champ): 1, 'patch:'+row.patch: 1}
        for field in ENEMY:
            name = _norm(getattr(row, field))
            for key in ('enemy:'+name, 'interaction:'+row.role+':'+name):
                values[key] = values.get(key, 0) + 1
        result.append(values)
    return result


def cluster_interval(delta, groups):
    """Resample whole held-out players, never individual games independently."""
    sums = pd.DataFrame({'delta': delta, 'group': groups}).groupby('group').delta.agg(['sum','size'])
    losses, sizes = sums['sum'].to_numpy(), sums['size'].to_numpy()
    rng = np.random.default_rng(20260928)
    samples = []
    for _ in range(500):
        indexes = rng.integers(0, len(sums), len(sums))
        samples.append(float(losses[indexes].sum() / sizes[indexes].sum()))
    return list(map(float, np.quantile(samples, [.025,.975])))


def weights_from_coefficients(coefficients, seen, floor, reference):
    enemy = pd.Series({k[6:]:v for k,v in coefficients.items() if k.startswith('enemy:')})
    odds = np.exp(enemy - enemy.median())
    return {champ: dict(odds=round(float(odds[champ]),3), seen=int(seen[champ]),
                        weight=round(float(np.clip((odds[champ]-floor)/(reference-floor),0,1)),3))
            for champ in enemy.index if seen.get(champ,0) >= MIN_SEEN}


def evaluate(train, validation):
    rows = features(pd.concat([train, validation], ignore_index=True))
    report = {'models': {}, 'comparisons': {}}
    predictions, coefficients = {}, {}
    for variant, allowed in VARIANTS.items():
        filtered = [{k:v for k,v in row.items() if k.split(':',1)[0] in allowed} for row in rows]
        vectorizer = DictVectorizer()
        x_train = vectorizer.fit_transform(filtered[:len(train)])
        x_validation = vectorizer.transform(filtered[len(train):])
        for strength in ((1.0,) if variant == 'shared' else STRENGTHS):
            key = f'{variant}/C={strength}'
            scores = {}
            for answer in ANSWERS:
                y_train = train[answer].astype(int).to_numpy()
                y_validation = validation[answer].astype(int).to_numpy()
                if len(np.unique(y_train)) < 2 or len(np.unique(y_validation)) < 2:
                    raise ValueError('Each answer needs positive and negative training/validation examples')
                model = LogisticRegression(C=strength, max_iter=3000).fit(x_train, y_train)
                prediction = model.predict_proba(x_validation)[:,1]
                predictions[key,answer] = prediction
                coefficients[key,answer] = dict(zip(vectorizer.get_feature_names_out(),model.coef_[0]))
                scores[answer] = {
                    'positives': int(y_validation.sum()),
                    'log_loss': float(log_loss(y_validation, prediction)),
                    'brier': float(brier_score_loss(y_validation, prediction)),
                    'average_precision': float(average_precision_score(y_validation, prediction)),
                }
                print(f'{key:29} {answer:11} loss={scores[answer]["log_loss"]:.6f} '
                      f'AP={scores[answer]["average_precision"]:.4f}', flush=True)
            report['models'][key] = scores
    candidate_pack = {'source': 'Training-only legacy corpus, own-champion and patch controls; C chosen on validation. Research only.'}
    seen = pd.concat([train[c].map(_norm) for c in ENEMY]).value_counts()
    for answer, (_,floor,reference) in ANSWERS.items():
        best = {variant: min((key for key in report['models'] if key.startswith(variant+'/')),
                             key=lambda key: report['models'][key][answer]['log_loss'])
                for variant in VARIANTS}
        y = validation[answer].astype(int).to_numpy()

        def losses(key):
            p = np.clip(predictions[key,answer], 1e-12, 1-1e-12)
            return -(y*np.log(p) + (1-y)*np.log(1-p))

        comparisons = {}
        for baseline, candidate in [('shared','champion_control'), ('champion_control','role_interaction')]:
            delta = losses(best[candidate]) - losses(best[baseline])
            comparisons[candidate] = dict(
                baseline=best[baseline], candidate=best[candidate],
                loss_delta=float(delta.mean()), interval_95=cluster_interval(delta, validation.sfk.to_numpy()))
        report['comparisons'][answer] = comparisons
        candidate_pack[answer] = weights_from_coefficients(coefficients[best['champion_control'],answer],seen,floor,reference)
    return report, candidate_pack


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data', type=Path, default=Path.home()/'data/recall')
    parser.add_argument('--output', type=Path, default=Path.home()/'data/recall/evaluation/answer-models.json')
    parser.add_argument('--candidate-pack', type=Path, help='Optional training-only experimental answer pack, outside Git')
    args = parser.parse_args()
    players = args.data/'derived/players.parquet'
    timeline = args.data/'kaggle/ranked-timeline/MatchTimelineTbl.csv'
    parts = split_players(pd.read_parquet(players))
    train = parts['train']; train = train[train.dur >= 900]
    validation = parts['validation']
    validation = validation[(validation.dur >= 900) & validation.patch.isin(['16.17','16.18'])]
    labels = read_labels(timeline, pd.concat([train[['smid']],validation[['smid']]]))
    train, train_missing = observed_rows(train, labels)
    validation, validation_missing = observed_rows(validation, labels)
    if set(train.sfk) & set(validation.sfk) or set(train.mid) & set(validation.mid):
        raise ValueError('Training and validation overlap')
    report, candidate = evaluate(train, validation)
    report.update(
        schema=1, protocol='legacy-answer-ownership-validation-v1',
        train_games=len(train), validation_games=len(validation),
        train_players=int(train.sfk.nunique()), validation_players=int(validation.sfk.nunique()),
        excluded_missing_timeline=dict(train=train_missing, validation=validation_missing),
        source_hashes={p.name:file_hash(p) for p in (players,timeline)},
        code_hashes={p.name:file_hash(p) for p in (Path(__file__),Path(__file__).with_name('backtest.py'),
                                                Path(__file__).with_name('export_answers.py'))},
        selection=dict(strengths=list(STRENGTHS), metric='validation log loss',
                       bootstrap_replicates=500, bootstrap_unit='player', seed=20260928),
        limitations=[
            'Ever owns an answer, not purchase timing, causality, or win improvement.',
            'Own-champion and patch controls are added together; their effects are not separated.',
            'Strength selection and bootstrap comparisons reuse validation: exploratory intervals, not final test evidence.',
            'No production export or test split scoring. Planner behavior requires a separate same-case comparison.',
        ])
    if args.candidate_pack:
        target = private_output(args.candidate_pack)
        write_json(target, candidate)
        report['candidate_pack_sha256'] = file_hash(target)
    write_json(args.output, report)
    print(json.dumps(report['comparisons'], indent=2))


if __name__ == '__main__':
    main()
