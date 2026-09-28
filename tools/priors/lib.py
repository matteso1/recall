"""Shared code for exp-prior: features, count backbone with hierarchical Dirichlet backoff, log-linear
correction layer (conditional logit with offset), metrics. Everything here maps 1:1 to what the Rust side
would compute at 1 Hz (see REPORT.md, export format)."""
import os, json, zlib
import numpy as np, pandas as pd
from scipy.optimize import minimize
H = os.path.dirname(os.path.abspath(__file__))
ROLES = ['TOP', 'JUNGLE', 'MIDDLE', 'BOTTOM', 'UTILITY']
DD = json.load(open('/mnt/c/Users/nilsm/AppData/Local/Recall/ddragon/16.19.1/item.json'))['data']
EPS = 1e-12


def fold_of(sfk, k=5):
    return np.array([zlib.crc32(str(int(s)).encode()) % k for s in sfk])


# ---------------------------------------------------------------- features
def load(kind='leg'):
    D = pd.read_parquet(H + '/data/dec.parquet')
    D = D[D.type == kind].reset_index(drop=True)
    P = pd.read_parquet(H + '/data/champ_profile.parquet').set_index('champ')
    D['cls'] = D.champ.map(P.cls).fillna('Fighter')
    D['nb'] = D.nth.clip(upper=4)
    D['fold'] = fold_of(D.sfk)
    D['owned'] = D.owned.map(lambda o: ' '.join(sorted(o.split())) if o else '')
    return D, P


def comp_features(D, P):
    """Enemy/ally composition features from champion identities only (live: from allPlayers championName)."""
    X = pd.DataFrame(index=D.index)
    get = lambda col, f: D[col].map(P[f]).astype(float)
    E = ['e_' + r for r in ROLES]
    dpm = np.stack([get(c, 'dpm').fillna(P.dpm.mean()) for c in E], 1)
    mag = np.stack([get(c, 'magic').fillna(P.magic.mean()) for c in E], 1)
    tru = np.stack([get(c, 'true').fillna(P['true'].mean()) for c in E], 1)
    X['e_magic'] = (dpm * mag).sum(1) / dpm.sum(1)
    X['e_true'] = (dpm * tru).sum(1) / dpm.sum(1)
    for f, name in [('tank', 'e_tank'), ('heal', 'e_heal'), ('shield_pm_z', 'e_shield'), ('cc_pm_z', 'e_cc'), ('ranged', 'e_ranged')]:
        X[name] = np.nansum(np.stack([get(c, f) for c in E], 1), 1)
    tags = P.tags.str.split('|')
    X['e_assassin'] = sum(D[c].map(tags.map(lambda t: 'Assassin' in t)).fillna(False).astype(float) for c in E)
    X['e_marksman'] = sum(D[c].map(tags.map(lambda t: 'Marksman' in t)).fillna(False).astype(float) for c in E)
    X['opp_magic'] = D.opp.map(P.magic).fillna(P.magic.mean())
    X['opp_tank'] = D.opp.map(P.tank).fillna(0)
    X['opp_heal'] = D.opp.map(P.heal).fillna(0)
    X['opp_ranged'] = D.opp.map(P.ranged).fillna(0.5)
    A = ['a_' + r for r in ROLES]
    adpm = np.stack([get(c, 'dpm').fillna(P.dpm.mean()) for c in A], 1)
    amag = np.stack([get(c, 'magic').fillna(P.magic.mean()) for c in A], 1)
    self_ = np.stack([(D[c] == D.champ).values for c in A], 1)
    adpm = np.where(self_, 0, adpm)
    X['a_magic'] = (adpm * amag).sum(1) / adpm.sum(1)
    return X


ECON_MED = None


def live_features(D, suf='_d'):
    """Live-Client-computable own/lobby state at the decision state. econ = visible item value + current gold."""
    global ECON_MED
    X = pd.DataFrame(index=D.index)
    m = D['s_min' if suf == '_d' else 'p_min'].clip(upper=40)
    econ = D['item_val' + suf] + D['CurrentGold' + suf]
    key = D.role + '_' + m.astype(str)
    if ECON_MED is None:
        tr = D.patch < '16.18'
        ECON_MED = pd.DataFrame({'k': key[tr], 'e': econ[tr], 'c': D['cs' + suf][tr]}).groupby('k').median()
    X['minute'] = m
    X['econ_rel'] = (econ - key.map(ECON_MED.e)) / 1000.0
    X['cs_rel'] = (D['cs' + suf] - key.map(ECON_MED.c)) / 50.0
    X['cur_gold'] = D['CurrentGold' + suf] / 1000.0
    X['lvl_diff'] = (D['Level' + suf] - D['opp_level' + suf]).clip(-4, 4)
    X['kills'] = np.log1p(D['Kills' + suf]); X['deaths'] = np.log1p(D['Deaths' + suf])
    X['deaths5'] = D['deaths5' + suf].clip(upper=3)
    X['team_kd'] = (D['TeamKills' + suf] - D['EnemyKills' + suf]).clip(-20, 20) / 5.0
    X['tower_diff'] = (D['TeamTowers' + suf] - D['EnemyTowers' + suf]).clip(-6, 6)
    return X.fillna(0)


def oracle_features(D, suf='_d'):
    X = pd.DataFrame(index=D.index)
    X['gold_diff'] = (D['GoldDiff' + suf] / 1000.0).clip(-8, 8)
    X['team_gold_diff'] = (D['TeamGoldDiff' + suf] / 1000.0).clip(-20, 20)
    return X.fillna(0)


def keystone_features(D, top=None):
    if top is None:
        top = D.ks.value_counts().index[:17].tolist()
    X = pd.DataFrame({f'ks_{k}': (D.ks == k).astype(float) for k in top}, index=D.index)
    return X, top


# ---------------------------------------------------------------- items
def item_index(D):
    items = sorted(D.item.unique().tolist())
    return items, {it: k for k, it in enumerate(items)}


def owned_matrix(D, idx):
    O = np.zeros((len(D), len(idx)), dtype=bool)
    for r, s in enumerate(D.owned.values):
        if s:
            for t in s.split():
                k = idx.get(int(t))
                if k is not None: O[r, k] = True
    return O


# ---------------------------------------------------------------- count backbone
LEVELS = {
    'role': ['role', 'nb'],
    'cls': ['cls', 'role', 'nb'],
    'champ': ['champ', 'nb'],
    'cr': ['champ', 'role', 'nb'],
    'last': ['champ', 'role', 'nb', 'last'],
    'set': ['champ', 'role', 'owned'],
    'iv': ['champ', 'role', 'nb'],  # intervals-2026 counts for the same key, supplied externally (Backbone.ext)
}


def make_codes(D, levels):
    codes = {}
    for lv in levels:
        cols = LEVELS[lv]
        codes[lv] = D.groupby(cols, sort=False, dropna=False).ngroup().values
    return codes


class Backbone:
    """Chain of Dirichlet-smoothed count tables: p_L = (c_L + a_L * p_{L-1}) / (N_L + a_L), from uniform."""
    def __init__(self, levels, alphas, n_items, ext=None):
        self.levels, self.alphas, self.I = levels, alphas, n_items
        self.ext = ext or {}  # level -> fixed count matrix over that level's code space (e.g. intervals prior)

    def fit(self, codes, y, w, rows):
        self.M = {}
        for lv in self.levels:
            if lv in self.ext:
                self.M[lv] = self.ext[lv]; continue
            c = codes[lv]
            M = np.zeros((c.max() + 1, self.I), dtype=np.float64)
            np.add.at(M, (c[rows], y[rows]), w[rows])
            self.M[lv] = M
        return self

    def predict(self, codes, rows, O=None, upto=None):
        p = np.full((len(rows), self.I), 1.0 / self.I)
        for lv in self.levels:
            if upto is not None and lv == upto: break
            a = self.alphas[lv]
            C = self.M[lv][codes[lv][rows]]
            N = C.sum(1, keepdims=True)
            p = (C + a * p) / (N + a)
        if O is not None:
            p = np.where(O[rows], 0.0, p)
            p /= p.sum(1, keepdims=True)
        return p


def player_weights(D, rows, cap):
    """w = min(1, cap / games of this player on this champion-role in the training rows)."""
    w = np.zeros(len(D))
    if cap is None or cap <= 0:
        w[rows] = 1.0; return w
    sub = D.iloc[rows]
    g = sub.groupby(['sfk', 'champ', 'role']).smid.transform('nunique').values
    w[rows] = np.minimum(1.0, cap / g)
    return w


# ---------------------------------------------------------------- log-linear correction layer
class LogLinear:
    """logit_i = tau * log p_backbone_i + sum_j owned_j W[i,j] + sum_k x_k W[i, I+k] + sum_{a,l} A[i,a] V[a,l] xa_l
    (owned items masked). X columns get item-specific weights; Xa columns get item-ATTRIBUTE weights through the
    fixed item-attribute matrix A (I x a), which transfers across patches via Data Dragon stats.
    Fitted per role by L-BFGS with L2 penalty lam on W and lam_attr on V (tau unpenalized)."""
    def __init__(self, lam=1.0, use_pairs=True, attr=None, lam_attr=None):
        self.lam, self.use_pairs, self.attr = lam, use_pairs, attr
        self.lam_attr = lam if lam_attr is None else lam_attr

    def _design(self, O, X):
        parts = []
        if self.use_pairs: parts.append(O.astype(np.float32))
        if X is not None and X.shape[1]: parts.append(X.astype(np.float32))
        return np.hstack(parts) if parts else np.zeros((O.shape[0], 0), np.float32)

    def fit(self, logp, O, X, y, sw=None, maxiter=400, Xa=None):
        n, I = logp.shape
        Z = self._design(O, X); F = Z.shape[1]
        self.I, self.F = I, F
        Y = np.zeros((n, I), np.float32); Y[np.arange(n), y] = 1
        sw = np.ones(n, np.float32) if sw is None else sw.astype(np.float32)
        lp = np.where(O, 0.0, logp).astype(np.float32)
        mask = O
        A = self.attr if Xa is not None else None
        na = 0 if A is None else A.shape[1]; nx = 0 if Xa is None else Xa.shape[1]

        def unpack(th):
            tau = th[0]; W = th[1:1 + I * F].reshape(I, F)
            V = th[1 + I * F:].reshape(na, nx) if na else None
            return tau, W, V

        def f(th):
            tau, W, V = unpack(th)
            L = tau * lp + Z @ W.T
            if na: L = L + (Xa @ V.T) @ A.T
            L = np.where(mask, -1e4, L)
            L -= L.max(1, keepdims=True)
            E = np.exp(L); S = E.sum(1, keepdims=True); Pm = E / S
            ll = (np.log(Pm[np.arange(n), y] + 1e-30) * sw).sum()
            G = (Pm - Y) * sw[:, None]
            gW = G.T @ Z + self.lam * W
            gt = (G * lp).sum()
            obj = -ll + 0.5 * self.lam * (W * W).sum()
            grads = [np.array([gt]), gW.ravel()]
            if na:
                gV = (A.T @ G.T) @ Xa + self.lam_attr * V
                obj += 0.5 * self.lam_attr * (V * V).sum(); grads.append(gV.ravel())
            return obj / n, np.concatenate(grads).astype(np.float64) / n

        th0 = np.zeros(1 + I * F + na * nx); th0[0] = 1.0
        r = minimize(f, th0, jac=True, method='L-BFGS-B', options=dict(maxiter=maxiter))
        self.tau, self.W, self.V = unpack(r.x)
        self.opt = r
        return self

    def logits(self, logp, O, X, Xa=None):
        Z = self._design(O, X)
        L = self.tau * np.where(O, 0.0, logp) + Z @ self.W.T
        if Xa is not None and self.V is not None: L = L + (Xa @ self.V.T) @ self.attr.T
        return np.where(O, -1e4, L)

    def predict(self, logp, O, X, Xa=None):
        L = self.logits(logp, O, X, Xa)
        L -= L.max(1, keepdims=True); E = np.exp(L)
        return E / E.sum(1, keepdims=True)


def item_attrs(items, dd_items):
    """Item attribute matrix used by the factorized context terms (computed from Data Dragon of the patch)."""
    names = ['MR', 'armor', 'HP', 'AP', 'AD', 'AS', 'crit', 'armor_pen', 'magic_pen', 'antiheal', 'lifesteal', 'haste']
    A = np.zeros((len(items), len(names)))
    for k, it in enumerate(items):
        d = dd_items.get(str(it), {}); s = d.get('stats', {}); t = set(d.get('tags', [])); desc = d.get('description', '')
        A[k] = [s.get('FlatSpellBlockMod', 0) > 0, s.get('FlatArmorMod', 0) > 0, s.get('FlatHPPoolMod', 0) > 0,
                s.get('FlatMagicDamageMod', 0) > 0, s.get('FlatPhysicalDamageMod', 0) > 0, s.get('PercentAttackSpeedMod', 0) > 0,
                s.get('FlatCritChanceMod', 0) > 0, 'ArmorPenetration' in t, 'MagicPenetration' in t,
                is_antiheal(desc), bool({'LifeSteal', 'SpellVamp'} & t), 'AbilityHaste' in t or 'CooldownReduction' in t]
    return A.astype(np.float32), names


# ---------------------------------------------------------------- metrics
def metrics(P, y, idx=None):
    if idx is not None: P, y = P[idx], y[idx]
    n = len(y)
    py = P[np.arange(n), y]
    rank = (P > py[:, None]).sum(1)
    return dict(n=n, logloss=float(-np.log(py + EPS).mean()), top1=float((rank == 0).mean()), top3=float((rank < 3).mean()))


def ece(P, y, bins=10):
    n = len(y)
    conf = P.max(1); hit = (P.argmax(1) == y).astype(float)
    b = np.minimum((conf * bins).astype(int), bins - 1)
    e = 0.0; rel = []
    for k in range(bins):
        m = b == k
        if m.sum():
            e += m.sum() / n * abs(conf[m].mean() - hit[m].mean())
            rel.append((k, int(m.sum()), round(float(conf[m].mean()), 3), round(float(hit[m].mean()), 3)))
    return float(e), rel


def ece_all(P, y, bins=15):
    """Calibration over all candidate probabilities (every item of every row), equal-width bins on log scale."""
    n, I = P.shape
    Y = np.zeros_like(P, dtype=bool); Y[np.arange(n), y] = True
    p = P.ravel(); t = Y.ravel(); keep = p > 1e-6
    p, t = p[keep], t[keep]
    edges = np.array([0, .005, .01, .02, .05, .1, .2, .3, .4, .5, .6, .7, .8, .9, 1.0001])
    b = np.digitize(p, edges) - 1
    e = 0.0; rel = []
    for k in range(len(edges) - 1):
        m = b == k
        if m.sum():
            e += m.sum() / len(p) * abs(p[m].mean() - t[m].mean())
            rel.append((f'{edges[k]:.3f}-{edges[k+1]:.3f}', int(m.sum()), round(float(p[m].mean()), 4), round(float(t[m].mean()), 4)))
    return float(e), rel


def cluster_boot(vals, groups, B=500, seed=0):
    """Summoner-cluster bootstrap CI of a per-row mean (vals may be a difference)."""
    rng = np.random.default_rng(seed)
    g = pd.Series(vals).groupby(groups)
    s = g.sum().values; c = g.size().values; k = len(s)
    est = s.sum() / c.sum()
    bs = []
    for _ in range(B):
        ix = rng.integers(0, k, k)
        bs.append(s[ix].sum() / c[ix].sum())
    return float(est), float(np.percentile(bs, 2.5)), float(np.percentile(bs, 97.5))


def is_antiheal(desc):
    return ('Wounds</keyword>' in desc) or ('Grievous Wounds</status>' in desc)


def intervals_matrix(D, codes, items, lam):
    """Intervals-2026 (16.1/16.2) next-legendary counts keyed like codes['iv'] (champ, role, nb), scaled by lam.
    Items outside the ranked-timeline universe are dropped; 3097 (old Stormrazor id) -> 3095."""
    IV = pd.read_parquet(H + '/data/iv_dec.parquet')
    IV['item'] = IV.item.replace({3097: 3095})
    idx = {it: k for k, it in enumerate(items)}
    IV = IV[IV.item.isin(idx)]
    IV['nb'] = IV.nth.clip(upper=4)
    key = dict(zip(zip(D.champ, D.role, D.nb), codes['iv']))
    M = np.zeros((codes['iv'].max() + 1, len(items)))
    for c, r, n, it in zip(IV.champ, IV.role, IV.nb, IV.item):
        k = key.get((c, r, n))
        if k is not None: M[k, idx[it]] += lam
    return M
