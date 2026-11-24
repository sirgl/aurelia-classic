"""Variant MB of the army net (M-HANDOFF.md, Roman's line B + memory): M plus B's hex-conv map encoder, encoded ONCE
per turn (from the turn's first moment; no updates within the turn) at MAP_CH channels over the 24 x 24 window.

The encoder is B's (`model.py`: 1 x 1 projection, 4 residual blocks of two hex convolutions, dilations as B); its
output enters as in B: a token adds m_tok(M[its tile]), a candidate m_cand(M[its tile]), the objective
m_pool(mean M). The crop to the army's area of the MB design is not built here: the Rust kernels belong to the
`map-encoder` lane (vectorised conv, incremental updates); MB changes only the configuration.

`crates/qa/src/army_net/model.rs` is the same arithmetic in Rust.
"""
import mlx.core as mx
import mlx.nn as nn

from .model import DIL, M_IN, Block, map_planes
from .model_m import PLANS, MemNet

MAP_CH = 32


class MBNet(MemNet):
    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=MAP_CH):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos)
        self.config['map_ch'] = map_ch
        self.config['map'] = {'ch': map_ch, 'once_per_turn': True}
        self.has_map = True
        self.m_in = nn.Linear(M_IN, map_ch)
        self.blocks = [Block(map_ch, d1, d2) for d1, d2 in DIL]
        self.m_tok = nn.Linear(map_ch, d)
        self.m_cand = nn.Linear(map_ch, d)
        self.m_pool = nn.Linear(map_ch, d)

    def encode_turn(self, statics, tok, pos):
        """The turn's map [B, W*W, C] from its first moment."""
        return self.encode(map_planes(statics, tok, pos))

    def body_mbg(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean):
        """M's body with the map read at the tokens' tiles (gtok [B, T, C], tok_ok) and its mean (mmean [B, C])."""
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        pooled = self.m_pool(mmean)
        h = mx.concatenate([h[:, :1] + pooled[:, None, :], h[:, 1:]], axis=1)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        for li, layer in enumerate(self.layers):
            x = self.layer_m(layer, x, dpad, mask, pair_plan, self.plan_bias[li])
        return self.norm(x)

    def action_logits_g(self, h, unit, cand, promo, cmask, gcand, cand_ok):
        b = h.shape[0]
        hu = h[mx.arange(b), unit]
        h0 = h[:, 0]
        pre = self.c1(cand) + self.promo[promo] + mx.where(cand_ok[..., None], self.m_cand(gcand), 0.0)
        c = self.c2(nn.relu(pre))
        hu_b = mx.broadcast_to(hu[:, None, :], c.shape)
        h0_b = mx.broadcast_to(h0[:, None, :], c.shape)
        joint = mx.concatenate([hu_b, h0_b, c, hu_b * c], axis=-1)
        s = self.s2(nn.relu(self.s1(joint)))[..., 0]
        from .model import NEG
        return mx.where(cmask, s, NEG)


class MBANet(MBNet):
    """MB with the price input and the auxiliary heads (M-HANDOFF.md, backlog 2 and addition 8). Starts equal to MB:
    the price's projection is zero; the heads read the body and do not change the play.
    - price (3: worth in units / 8, time pressure, given) -> d, added to the objective token (`w_price`, zero init);
    - danger after layer 1 (`ge1`, `ge2`: the hit points each of our units loses in the enemy's next turn), beside
      MB's danger head on the last layer;
    - the strike's expected dealt / taken (the rules' preview, candidate columns 30, 31) from the action scorer's
      hidden state of that candidate (`xs`);
    - value on [H_0, P_0] (`va1`, `va2`): the win (logit), the capture (logit), Phi's change over the next 3 turns.
    The war target and stage on the plan tokens are M's auxiliary head already (`aux_target`, the stage)."""

    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=MAP_CH):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
        self.config['aux_heads'] = True
        self.w_price = mx.zeros((3, d))
        self.ge1 = nn.Linear(d, d)
        self.ge2 = nn.Linear(d, 1)
        self.xs = nn.Linear(d, 2)
        self.va1 = nn.Linear(2 * d, d)
        self.va2 = nn.Linear(d, 3)

    def body_b(self, b, um, pm):
        """The body on a batch dict: (H, the stream after layer 1)."""
        return self.body_mba(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['gtok'], b['tok_ok'], b['mmean'], b['price'])

    def body_mba(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean, price):
        """`body_mbg` with the price on the objective token; also returns the residual stream after layer 1."""
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        top = self.m_pool(mmean) + price @ self.w_price
        h = mx.concatenate([h[:, :1] + top[:, None, :], h[:, 1:]], axis=1)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        h1 = None
        for li, layer in enumerate(self.layers):
            x = self.layer_m(layer, x, dpad, mask, pair_plan, self.plan_bias[li])
            if li == 0:
                h1 = x
        return self.norm(x), h1

    def action_logits_a(self, h, unit, cand, promo, cmask, gcand, cand_ok):
        """`action_logits_g` and the scorer's hidden state per candidate [B, C, d]."""
        b = h.shape[0]
        hu = h[mx.arange(b), unit]
        h0 = h[:, 0]
        pre = self.c1(cand) + self.promo[promo] + mx.where(cand_ok[..., None], self.m_cand(gcand), 0.0)
        c = self.c2(nn.relu(pre))
        hu_b = mx.broadcast_to(hu[:, None, :], c.shape)
        h0_b = mx.broadcast_to(h0[:, None, :], c.shape)
        joint = mx.concatenate([hu_b, h0_b, c, hu_b * c], axis=-1)
        hid = nn.relu(self.s1(joint))
        s = self.s2(hid)[..., 0]
        from .model import NEG
        return mx.where(cmask, s, NEG), hid

    def danger_early(self, h1):
        return self.ge2(nn.relu(self.ge1(h1)))[..., 0]

    def value_a(self, h, p0):
        return self.va2(nn.relu(self.va1(mx.concatenate([h[:, 0], p0], axis=-1))))


# MBH: the history's parameters (their own lr group in train_m: `--new-lr-mult`).
HX_PARAMS = ('hx1', 'hx2', 'hxp', 'hx_null', 'hx_same', 'hx_slope', 'ho')
# The fact's own age number (history column 10, age / 3): left out of MBH's embedding (age enters as a bias only).
HX_AGE_COL = 10


class MBHNet(MBANet):
    """MBA plus the history of facts (M-HANDOFF.md, «MBH0 spec», 19:45). Starts equal to MBA: the history's output
    projections `ho` are zero.
    - A fact's embedding: hx2(relu(hx1(fact with its age number zeroed))) + hxp(P_0 of the fact's turn).
    - Per layer a SEPARATE cross-attention of the tokens and plan tokens (queries: the layer's q on norm1 of the
      stream) to the facts (keys / values: the layer's k / v on norm1 of the embedding), its own softmax with one
      always-present null key (logit hx_null[l, h], value 0), plus per head: -hx_slope[l, h] x age (ALiBi-style; age
      is not in the keys, so a fact's keys / values stay fixed while it lives) and hx_same[l, h] for a fact of the
      query's own unit. Its context enters the residual through ho[l] (d x d, zero at the start), beside the
      self-attention's output, before the feed-forward.
    - The memory update is MB's (`MemNet.step`)."""

    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=MAP_CH):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
        from .data import F_HIST
        from .model_mh import H_MAX
        self.config['xhist'] = {'f_hist': F_HIST, 'h_max': H_MAX, 'null_key': True, 'age_col_zeroed': HX_AGE_COL}
        self.hx1 = nn.Linear(F_HIST, d)
        self.hx2 = nn.Linear(d, d)
        self.hxp = nn.Linear(d, d)
        self.hxp.weight = self.hxp.weight * 0.1
        self.hx_null = mx.zeros((layers, heads))
        self.hx_same = mx.full((layers, heads), 1.0)
        self.hx_slope = mx.full((layers, heads), 0.5)
        self.ho = [nn.Linear(d, d) for _ in range(layers)]
        for o in self.ho:
            o.weight = mx.zeros((d, d))
            o.bias = mx.zeros((d,))

    def hx_embed(self, hist, hplan):
        keep = (mx.arange(hist.shape[-1]) != HX_AGE_COL).astype(hist.dtype)
        return self.hx2(nn.relu(self.hx1(hist * keep))) + self.hxp(hplan)

    def body_b(self, b, um, pm):
        return self.body_mbh(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['gtok'], b['tok_ok'], b['mmean'], b['price'],
                             b['hist'], b['hmask'], b['hslot'], b['hage'], b['hplan'])

    def body_mbh(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean, price, hist, hmask, hslot, hage, hplan):
        """`body_mba` with the history's cross-attention in every layer; returns (H, the stream after layer 1)."""
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        top = self.m_pool(mmean) + price @ self.w_price
        h = mx.concatenate([h[:, :1] + top[:, None, :], h[:, 1:]], axis=1)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        qslot = mx.concatenate([slot, mx.full((b, PLANS), -1, dtype=slot.dtype)], axis=1)
        he = self.hx_embed(hist, hplan)
        same = ((qslot[:, :, None] == hslot[:, None, :]) & (hslot[:, None, :] >= 0)).astype(mx.float32)  # [B, T+P, H]
        age = hage.astype(mx.float32)
        h1 = None
        for li, layer in enumerate(self.layers):
            x = self.layer_mbh(layer, self.ho[li], x, dpad, mask, pair_plan, self.plan_bias[li], he, hmask, same, age,
                               self.hx_null[li], self.hx_same[li], self.hx_slope[li])
            if li == 0:
                h1 = x
        return self.norm(x), h1

    @staticmethod
    def layer_mbh(layer, ho, h, dist, key_mask, pair_plan, pbias, he, hmask, same, age, nbias, sbias, slope):
        import math
        from .model import NEG
        b, t, d = h.shape
        heads = layer.heads
        dh = d // heads
        nh = he.shape[1]
        a = layer.norm1(h)
        q = layer.q(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        k = layer.k(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        v = layer.v(a).reshape(b, t, heads, dh).transpose(0, 2, 1, 3)
        sc = 1.0 / math.sqrt(dh)
        s = (q @ k.transpose(0, 1, 3, 2)) * sc
        bias = layer.dist_bias[:, dist].transpose(1, 0, 2, 3)
        bias = mx.where(pair_plan[None, None], pbias[None, :, None, None], bias)
        s = s + bias + mx.where(key_mask[:, None, None, :], 0.0, NEG)
        w = mx.softmax(s, axis=-1)
        ctx = (w @ v).transpose(0, 2, 1, 3).reshape(b, t, d)
        # The history: its own softmax over [null, facts].
        ah = layer.norm1(he)
        kh = layer.k(ah).reshape(b, nh, heads, dh).transpose(0, 2, 1, 3)
        vh = layer.v(ah).reshape(b, nh, heads, dh).transpose(0, 2, 1, 3)
        sh = (q @ kh.transpose(0, 1, 3, 2)) * sc - slope[None, :, None, None] * age[:, None, None, :] + same[:, None] * sbias[None, :, None, None]
        sh = sh + mx.where(hmask[:, None, None, :], 0.0, NEG)
        s0 = mx.broadcast_to(nbias[None, :, None, None], (b, heads, t, 1))
        wh = mx.softmax(mx.concatenate([s0, sh], axis=-1), axis=-1)
        ctxh = (wh[..., 1:] @ vh).transpose(0, 2, 1, 3).reshape(b, t, d)
        h = h + layer.o(ctx) + ho(ctxh)
        m = layer.norm2(h)
        return h + layer.f2(nn.relu(layer.f1(m)))


# MBN: the notes' parameters (their own lr group in train_m: `--new-lr-mult`).
NX_PARAMS = ('n1', 'n2', 'nx_null', 'nx_same', 'nx_slope', 'no', 'rest', 'reason', 'free')


class MBNNet(MBNet):
    """MB plus the NOTES the net writes about its plan at the end of each of its turns (M-HANDOFF.md, «MBN (24 Nov)»),
    read back by a separate per-layer cross-attention like MBH's. Starts equal to MB: the notes' output projections
    `no` are zero.

    The note stream of a play (`initial_notes`; MN's kinds, `model_mn.py`): PLAN(stage, engaged) with its age = turns
    in the stage, lives until the next PLAN; PLAN_CHANGE(reason) for CHANGE_SPAN turns after the stage changed;
    WAIT_STAGE(waited) while the plan is not engaged; PULL_BACK_HEAL(unit) while the unit rests (a state, not a transition: the teacher's
    resting set), its age = turns resting. A note token: MN's F_NOTE numbers (kind, stage, reason, waited / 10, age / 3,
    a free 8-vector tanh(free(P_0)), engaged), the unit's slot (-1 none) and its age (turns, for the bias).

    Per layer the tokens and the plan tokens (queries: the layer's q on norm1 of the stream) attend to the notes (keys
    / values: the layer's k / v on norm1 of the note embedding n2(relu(n1(note)))) with their own softmax over one
    always-present null key (logit nx_null[l, h], value 0), minus nx_slope[l, h] x age, plus nx_same[l, h] for a note of
    the query's own unit; the context enters the residual through no[l] (zero at the start).

    Written at our turn's end, after the memory's update (`note_heads`, `write_notes`): the stage = the plan token's
    stage readout (aux_p on P_0, trained by the auxiliary loss on the teacher's stage), engaged = aux_p[1] on
    P_0[P_ENGAGED] > 0, waited = P_0[P_WAITED] x 10, the
    resting units from `rest` on [H_i, token, u_s, resting before] (hand-set to the teacher's rule: rest under 70 hit
    points, stay until 90: `assault::ROTATE_BELOW`, `ROTATE_BACK`), the change's reason from `reason` on its
    indicators (a unit lost, the stage turned to heal-rotate, the stage advanced). In training the teacher's own track
    writes the notes of its plays (teacher forcing) and supervises the heads; in play the net writes its own."""

    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=MAP_CH):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
        from .model_mn import F_NOTE, N_NOTES, NOTE_KINDS, REASONS
        from .model_m import U
        from .data import F_TOK
        self.config['xnotes'] = {'kinds': NOTE_KINDS, 'reasons': REASONS, 'f_note': F_NOTE + 1, 'n_notes': N_NOTES, 'null_key': True, 'rest_state': True}
        self.n1 = nn.Linear(F_NOTE + 1, d)
        self.n2 = nn.Linear(d, d)
        self.nx_null = mx.zeros((layers, heads))
        self.nx_same = mx.full((layers, heads), 1.0)
        self.nx_slope = mx.full((layers, heads), 0.5)
        self.no = [nn.Linear(d, d) for _ in range(layers)]
        for o in self.no:
            o.weight = mx.zeros((d, d))
            o.bias = mx.zeros((d,))
        self.rest = nn.Linear(d + F_TOK + U + 1, 1)
        self.reason = nn.Linear(4, len(REASONS))
        from .model_mn import FREE
        self.free = nn.Linear(d, FREE)

    def hand_init(self):
        super().hand_init()
        from .model_mn import REASONS
        d = self.config['d']
        w = mx.zeros(self.rest.weight.shape)
        w[0, d + 7] = -20.0  # the token's hit points / 100
        w[0, -1] = 4.0       # resting before: stays until 90
        self.rest.weight = w
        self.rest.bias = mx.array([14.0])
        self.reason.weight = mx.eye(len(REASONS)) * 5.0
        self.reason.bias = mx.zeros((len(REASONS),))

    @staticmethod
    def initial_notes(b):
        from .model_m import SLOTS, STAGES
        from .model_mn import AGE_NONE, FREE, REASONS
        return {'stage': mx.zeros((b, STAGES)), 'stage_age': mx.zeros((b,)), 'engaged': mx.zeros((b,)), 'waited': mx.zeros((b,)), 'free': mx.zeros((b, FREE)),
                'change_age': mx.full((b,), AGE_NONE), 'reason': mx.zeros((b, len(REASONS))), 'rest': mx.zeros((b, SLOTS)), 'rest_age': mx.zeros((b, SLOTS))}

    NOTE_STATE = ('stage', 'stage_age', 'engaged', 'waited', 'free', 'change_age', 'reason', 'rest', 'rest_age')

    @staticmethod
    def note_tokens(ns):
        """[B, N_NOTES, F_NOTE + 1], mask [B, N_NOTES], slot [B, N_NOTES] (-1: not a unit's), age [B, N_NOTES] (turns).
        The last number: PLAN's engaged (the assault went in). WAIT_STAGE lives while a plan is not engaged (the
        teacher's stage label puts heal-rotate before stage, so "stage" alone misses the waits with units resting)."""
        from .model_m import SLOTS, STAGES
        from .model_mn import CHANGE_SPAN, FREE, NOTE_KINDS, REASONS, STAGE_WAIT
        b = ns['stage'].shape[0]
        k, st, rs = len(NOTE_KINDS), STAGES, len(REASONS)
        z = mx.zeros((b,))

        def tok(kind, stage=None, reason=None, waited=None, age=None, free=None, engaged=None):
            parts = [mx.broadcast_to(mx.array([1.0 if i == kind else 0.0 for i in range(k)]), (b, k)),
                     stage if stage is not None else mx.zeros((b, st)), reason if reason is not None else mx.zeros((b, rs)),
                     (waited if waited is not None else z)[:, None] * 0.1, (age if age is not None else z)[:, None] / 3.0,
                     free if free is not None else mx.zeros((b, FREE)), (engaged if engaged is not None else z)[:, None]]
            return mx.concatenate(parts, axis=1)

        has_plan = mx.sum(ns['stage'], axis=1) > 0
        sa = mx.minimum(ns['stage_age'], 9.0)
        ca = mx.minimum(ns['change_age'], 9.0)
        ra = mx.minimum(ns['rest_age'], 9.0)
        rows = [tok(0, stage=ns['stage'], age=sa, free=ns['free'], engaged=ns['engaged']), tok(1, stage=ns['stage'], reason=ns['reason'], age=ca), tok(2, waited=ns['waited'])]
        masks = [has_plan, ns['change_age'] < CHANGE_SPAN, has_plan & (ns['engaged'] < 0.5)]
        ages = [sa, ca, z]
        for s in range(SLOTS):
            rows.append(tok(3, age=ra[:, s]))
            masks.append(ns['rest'][:, s] > 0.5)
            ages.append(ra[:, s])
        slot = mx.concatenate([mx.full((b, 3), -1, dtype=mx.int32), mx.broadcast_to(mx.arange(SLOTS, dtype=mx.int32)[None], (b, SLOTS))], axis=1)
        return mx.stack(rows, axis=1), mx.stack(masks, axis=1), slot, mx.stack(ages, axis=1)

    def body_b(self, b, um, pm):
        """The body on a batch dict whose note stream is b['ns_<key>'] (b['ns_off'] [B] bool: the stream masked): (H, None)
        (MBA's body_b shape: H and the stream after layer 1, not kept here)."""
        ns = {k: b['ns_' + k] for k in self.NOTE_STATE}
        return self.body_mbn(b['tok'], b['dist'], b['tmask'], b['slot'], um, pm, b['gtok'], b['tok_ok'], b['mmean'], ns, b['ns_off']), None

    def body_mbn(self, tok, dist, tmask, slot, um, pm, gtok, tok_ok, mmean, ns, off):
        """`body_mbg` with the notes' cross-attention in every layer."""
        b, t, _ = tok.shape
        h = self.e2(nn.relu(self.e1(tok)))
        has = slot >= 0
        us = um[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        h = h + mx.where(has[..., None], self.um(us), 0.0)
        h = h + mx.where(tok_ok[..., None], self.m_tok(gtok), 0.0)
        pooled = self.m_pool(mmean)
        h = mx.concatenate([h[:, :1] + pooled[:, None, :], h[:, 1:]], axis=1)
        x = mx.concatenate([h, pm], axis=1)
        mask = mx.concatenate([tmask, mx.ones((b, PLANS), dtype=mx.bool_)], axis=1)
        is_plan = mx.concatenate([mx.zeros((t,), dtype=mx.bool_), mx.ones((PLANS,), dtype=mx.bool_)])
        pair_plan = is_plan[:, None] | is_plan[None, :]
        dpad = mx.pad(dist, [(0, 0), (0, PLANS), (0, PLANS)])
        nt, nmask, nslot, nage = self.note_tokens(ns)
        nmask = nmask & ~off[:, None]
        ne = self.n2(nn.relu(self.n1(nt)))
        qslot = mx.concatenate([slot, mx.full((b, PLANS), -1, dtype=slot.dtype)], axis=1)
        same = ((qslot[:, :, None] == nslot[:, None, :]) & (nslot[:, None, :] >= 0)).astype(mx.float32)
        for li, layer in enumerate(self.layers):
            x = MBHNet.layer_mbh(layer, self.no[li], x, dpad, mask, pair_plan, self.plan_bias[li], ne, nmask, same, nage,
                                 self.nx_null[li], self.nx_same[li], self.nx_slope[li])
        return self.norm(x)

    def note_heads(self, hfull, tok, slot, tmask, um2, pm2, ns):
        """The heads at our turn's end (after the memory's update um2 / pm2). Returns: stage logits [B, 6], engaged
        logit [B] (aux_p[1] on P_0[P_ENGAGED]), rest logits per token [B, T]."""
        from .model_m import P_ENGAGED, P_STAGE, STAGES
        from .model_mn import STAGE_HEAL
        t = tok.shape[1]
        b = tok.shape[0]
        ap = self.aux_p
        stage = ap[0, 0] * pm2[:, 0, P_STAGE:P_STAGE + STAGES] + ap[0, 1]
        sl = mx.maximum(slot, 0)
        us = um2[mx.arange(b)[:, None], sl]
        before = mx.take_along_axis(ns['rest'], sl, axis=1)[..., None]
        x = mx.concatenate([hfull[:, :t], tok, us, before], axis=-1)
        rest = self.rest(x)[..., 0]
        engaged = ap[1, 0] * pm2[:, 0, P_ENGAGED] + ap[1, 1]
        return stage, engaged, rest

    def reason_logits(self, ns, new_stage, event):
        from .model_mn import STAGE_HEAL
        had = mx.sum(ns['stage'], axis=1) > 0
        old = mx.argmax(ns['stage'], axis=1)
        ind = mx.stack([event, (new_stage == STAGE_HEAL).astype(mx.float32), (had & (new_stage > old)).astype(mx.float32), mx.zeros(event.shape)], axis=1)
        return self.reason(ind)

    @staticmethod
    def write_notes(ns, stage_oh, engaged, waited, reason_oh, rest_slots, free, valid):
        """The stream after a turn; everything [B, ...]; valid: the play had this turn."""
        had = mx.sum(ns['stage'], axis=1) > 0
        changed = had & (mx.argmax(stage_oh, axis=1) != mx.argmax(ns['stage'], axis=1))
        stage_age = mx.where(had & ~changed, ns['stage_age'] + 1.0, 0.0)
        change_age = mx.where(changed, 0.0, ns['change_age'] + 1.0)
        reason = mx.where(changed[:, None], reason_oh, ns['reason'])
        rest_age = mx.where(rest_slots > 0.5, mx.where(ns['rest'] > 0.5, ns['rest_age'] + 1.0, 0.0), 0.0)
        new = {'stage': stage_oh, 'stage_age': stage_age, 'engaged': engaged, 'waited': waited, 'free': free, 'change_age': change_age, 'reason': reason, 'rest': rest_slots, 'rest_age': rest_age}
        return {k: mx.where(valid.reshape((-1,) + (1,) * (v.ndim - 1)), v, ns[k]) for k, v in new.items()}


# MBe: the hand-set term's numbers (M-HANDOFF.md, "Hand edit: wounded out of range (24 Nov)"; model.rs WW_LEN).
WW = ('scale', 'hp_threshold', 'hp_slope', 'ranged_hp_threshold', 'resting_weight', 'city_low_threshold', 'city_low_slope', 'resting_slope', 'stay_scale', 'healing_weight', 'back_threshold', 'strike_scale', 'ranged_strike_threshold', 'stay_escape')
# Half the smallest step of a token's hit points (model.rs HEALING_STEP), of the threat numbers (THREAT_STEP).
HEALING_STEP = 0.005
THREAT_STEP = 0.125


class MBENet(MBNet):
    """MB plus hand-set terms of the action scorer (M-HANDOFF.md, "Hand edit: wounded out of range (24 Nov)"), both times the
    acting unit's gate g = wounded x (1 - city_low): (1) a candidate that does not strike (done, move, fortify, set up) and whose
    tile is in a foe city's strike range (candidate number 16) loses `scale x g` from its logit; (2) a move of a unit that stands
    out of that range (its token's number 36) loses `stay_scale x g` (a unit that moves does not heal); where
      wounded  = max(sigmoid(hp_slope (thr - hp)), resting_weight x sigmoid(resting_slope (aux_u[1] on u[U_RESTING])),
                 healing_weight x sigmoid(hp_slope (back_threshold - hp)) x sigmoid(hp_slope (gained - HEALING_STEP))),
                 gained = the token's number 46 (hit points gained since our last turn: the unit is healing, rests until back),
                 hp = the unit token's number 7, thr = ranged_hp_threshold for a unit with range (token number 13 > 0),
                 else hp_threshold; u = the unit's memory slot as the decision reads it;
      city_low = sigmoid(city_low_slope (city_low_threshold - the objective city's hp share)) (token numbers 3, 43, 7);
    and (3) a strike (kinds 2-4) loses `strike_scale x sigmoid(hp_slope (strike thr - hp)) x (1 - city_low)`, strike thr =
    ranged_strike_threshold for a unit with range, else hp_threshold. With stay_escape = 1 the stay term spares a move to a tile
    less threatened than the unit's own (candidate number 19 below the token's 39 by more than THREAT_STEP).
    `ww` = the numbers above (WW order). Zero scale = MB exactly. `crates/qa/src/army_net/model.rs` `actions_x` is the
    same arithmetic."""

    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400, map_ch=64):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos, map_ch=map_ch)
        self.config['ww'] = list(WW)
        self.ww = mx.zeros((len(WW),))

    def wounded_gate(self, tok, unit, slot, um):
        """([B], [B]): wounded x (1 - city_low) and the strike gate x (1 - city_low) of the acting unit."""
        from .model_m import U_RESTING
        b = tok.shape[0]
        w = self.ww
        ar = mx.arange(b)
        f = tok[ar, unit]
        thr = mx.where(f[:, 13] > 0.0, w[3], w[1])
        hp = mx.sigmoid(w[2] * (thr - f[:, 7]))
        s = slot[ar, unit]
        u = um[ar, mx.maximum(s, 0), U_RESTING]
        a = self.aux_u[1]
        rest = mx.where(s >= 0, w[4] * mx.sigmoid(w[7] * (a[0] * u + a[1])), 0.0)
        is_city = (tok[..., 3] > 0.5) & (tok[..., 43] > 0.5)
        first = mx.argmax(is_city.astype(mx.int32), axis=1)
        city = mx.where(mx.any(is_city, axis=1), tok[ar, first, 7], 1.0)
        healing = w[9] * mx.sigmoid(w[2] * (w[10] - f[:, 7])) * mx.sigmoid(w[2] * (f[:, 46] - HEALING_STEP))
        low = mx.sigmoid(w[6] * (w[5] - city))
        strike = mx.sigmoid(w[2] * (mx.where(f[:, 13] > 0.0, w[12], w[1]) - f[:, 7]))
        return mx.maximum(mx.maximum(hp, rest), healing) * (1.0 - low), strike * (1.0 - low)

    def wounded_term(self, cand, gates, out_of_fire, here_threat):
        """[B, C]: the terms subtracted from each candidate's logit (gates: `wounded_gate`; out_of_fire [B]: 1 - the unit token's
        number 36; here_threat [B]: its number 39)."""
        gate, gs = gates
        w = self.ww
        f = cand[..., 16] * (cand[..., 0] + cand[..., 1] + cand[..., 5] + cand[..., 6])
        strike = cand[..., 2] + cand[..., 3] + cand[..., 4]
        escape = mx.where(cand[..., 19] < here_threat[:, None] - THREAT_STEP, w[13], 0.0)
        return (w[0] * f + w[8] * cand[..., 1] * out_of_fire[:, None] * (1.0 - escape)) * gate[:, None] + w[11] * strike * gs[:, None]

    def action_logits_g(self, h, unit, cand, promo, cmask, gcand, cand_ok, tok=None, slot=None, um=None):
        from .model import NEG
        if tok is None:
            raise ValueError('MBENet scores read the tokens, the slots and the memory (tok, slot, um)')
        s = MBNet.action_logits_g(self, h, unit, cand, promo, cmask, gcand, cand_ok)
        fu = tok[mx.arange(tok.shape[0]), unit]
        s = s - self.wounded_term(cand, self.wounded_gate(tok, unit, slot, um), 1.0 - fu[:, 36], fu[:, 39])
        return mx.where(cmask, s, NEG)
