# Identical-state planner stability — September 28, 2026

The planner used its displayed path as memory for near-ties in the conditional item model.
Defense reordering and buy-now promotion could therefore change the next model sequence even
when the observation had not changed. A synthetic Lulu regression reproduced a different third
legendary on the second call. The fix retains the model sequence separately in `last_chain`;
presentation still uses `last_path`. No scoring weights or trained artifacts changed.

Compared with `bc554a9`, using identical before/after input fingerprints:

| Metric | Validation | Separate test |
|---|---|---|
| Games / observations | 3,924 / 88,959 | 4,046 / 92,147 |
| Identical-state target changes | 11 → 0 | 7 → 0 |
| Identical-state path changes | 172 → 0 | 206 → 0 |
| Invalid recommendations | 0 → 0 | 0 → 0 |
| Purchase target agreement | 50.44% → 50.50% | 51.88% → 51.92% |
| First legendary agreement | 58.80% → 58.87% | 61.23% → 61.26% |
| Boots agreement | 67.41% → 67.41% | 65.34% → 65.34% |

This removes 18 target changes and 378 path changes caused by repeating an observation. Minute
transitions can still change legitimately as gold, enemies or inventories change. Agreement is
imitation, not evidence of increased wins. The baseline archived binary was rerun on the same
inputs after the reporting script changed its preparation fingerprint; all regenerated case and
model artifacts were also checked for byte equality with the earlier inputs.

[Aggregate report](planner-stability-evaluation.json) includes every role, Xayah, raw numerators
and denominators, and model hashes. Detailed diagnostic examples stay outside Git. The evaluator
now prints path-repeat rates alongside target-repeat rates and saves capped reproduction samples.

Verification: the regression failed before the fix; 310 core tests with the evaluation feature,
39 runtime tests, core/runtime Clippy and Windows release type-check pass. The current recorded
games replay without shop-legality violations. Sparse legacy recordings can still differ from
live hidden-boot inference; replay legality does not establish exact reproduction of live advice.

Reproduce with the persistent environment described in the priors README:

```bash
~/data/recall/.venv/bin/python tools/priors/backtest.py \
  --baseline ~/data/recall/evaluation/stability-chain.baseline.json \
  --output ~/data/recall/evaluation/latest.json
```
