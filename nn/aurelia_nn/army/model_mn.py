"""Variant MN of the army net (M-HANDOFF.md, addition 4): MH plus a stream of NOTE records the net writes at the end of
each of its turns, read back like the history (keys / values of every layer).

Notes (`NOTE_KINDS`): PLAN(stage) — the operation's stage, from the plan token's stage head; it lives until the next
PLAN; PLAN_CHANGE(reason) — written when PLAN's stage differs from the last PLAN's, its reason from a head
(losses, the city stronger: the rotation, the stage advanced, other), lives 2 turns; WAIT_STAGE(waited) — while the
stage is "stage", the turns waited (the plan's P_WAITED); PULL_BACK_HEAL(unit) — a unit leaves to heal, lives until
RETURN(unit). Each note token: the kind, the stage, the reason, waited / 10, its age / 3, a free 8-dim vector (the
plan's nuance, tanh of a linear of P_0), and the unit's slot (the pointer; the same-unit bias of the history).
Not built (named as gaps): CAPTURER, FOCUS, HOLD, THREAT, and net2's STRATEGY records (in the trainers the
objective token already carries the aim and the target).

Training: the stream of a teacher's play is written from the teacher's own state (its track: stage, waited, the
resting units), the head supervised by it; the net's own plays (DAgger) carry the net's own notes. The heads:
PULL_BACK_HEAL starts at the teacher's rule (hp < 70: `assault::ROTATE_BELOW`), RETURN at hp >= 90
(`ROTATE_BACK`), the reason at the indicator of each reason (a unit lost; the stage turned to heal-rotate; the
stage advanced).
"""
import math

import mlx.core as mx
import mlx.nn as nn

from .data import F_TOK
from .model_m import P_STAGE, P_WAITED, SLOTS, STAGES, U, U_RESTING
from .model_mh import MHNet

NOTE_KINDS = ['PLAN', 'PLAN_CHANGE', 'WAIT_STAGE', 'PULL_BACK_HEAL', 'RETURN']
REASONS = ['losses', 'city stronger', 'advance', 'other']
FREE = 8
F_NOTE = len(NOTE_KINDS) + STAGES + len(REASONS) + 2 + FREE
N_NOTES = 3 + SLOTS  # PLAN, PLAN_CHANGE, WAIT_STAGE, one PULL_BACK_HEAL per slot
STAGE_WAIT = 2
STAGE_HEAL = 4
CHANGE_SPAN = 2
AGE_NONE = 99.0


class MNNet(MHNet):
    def __init__(self, d=64, heads=4, layers=3, ff=128, promos=400):
        super().__init__(d=d, heads=heads, layers=layers, ff=ff, promos=promos)
        self.config['notes'] = {'kinds': NOTE_KINDS, 'reasons': REASONS, 'f_note': F_NOTE, 'n_notes': N_NOTES}
        self.n1 = nn.Linear(F_NOTE, d)
        self.n2 = nn.Linear(d, d)
        x_unit = d + F_TOK + U
        self.pull = nn.Linear(x_unit, 1)
        self.ret = nn.Linear(x_unit, 1)
        self.reason = nn.Linear(4, len(REASONS))
        self.free = nn.Linear(d, FREE)

    def hand_init(self):
        super().hand_init()
        d = self.config['d']
        x_unit = d + F_TOK + U
        for head, wt, b in ((self.pull, -20.0, 14.0), (self.ret, 20.0, -18.0)):
            w = mx.zeros((1, x_unit))
            w[0, d + 7] = wt  # the token's hit points / 100: pull back under 70, return from 90 (ROTATE_BELOW, ROTATE_BACK)
            head.weight = w
            head.bias = mx.array([b])
        self.reason.weight = mx.eye(len(REASONS)) * 5.0
        self.reason.bias = mx.zeros((len(REASONS),))

    def initial_notes(self, b):
        """The note stream's state at a play's start: stage one-hot [6] (none yet: zeros), waited, free [8], the change's
        age and reason [4], each slot's PULL_BACK_HEAL active and its age."""
        return {'stage': mx.zeros((b, STAGES)), 'waited': mx.zeros((b,)), 'free': mx.zeros((b, FREE)), 'change_age': mx.full((b,), AGE_NONE),
                'reason': mx.zeros((b, len(REASONS))), 'pull': mx.zeros((b, SLOTS)), 'pull_age': mx.zeros((b, SLOTS))}

    @staticmethod
    def note_tokens(ns):
        """[B, N_NOTES, F_NOTE], mask, slot (-1: not a unit's)."""
        b = ns['stage'].shape[0]
        k = len(NOTE_KINDS)
        st, rs = STAGES, len(REASONS)

        def tok(kind, stage=None, reason=None, waited=None, age=None, free=None):
            parts = [mx.broadcast_to(mx.array([1.0 if i == kind else 0.0 for i in range(k)]), (b, k)),
                     stage if stage is not None else mx.zeros((b, st)), reason if reason is not None else mx.zeros((b, rs)),
                     (waited if waited is not None else mx.zeros((b,)))[:, None] * 0.1, (age if age is not None else mx.zeros((b,)))[:, None] / 3.0,
                     free if free is not None else mx.zeros((b, FREE))]
            return mx.concatenate(parts, axis=1)

        has_plan = mx.sum(ns['stage'], axis=1) > 0
        rows = [tok(0, stage=ns['stage'], free=ns['free']), tok(1, stage=ns['stage'], reason=ns['reason'], age=mx.minimum(ns['change_age'], 9.0)), tok(2, waited=ns['waited'])]
        masks = [has_plan, ns['change_age'] < CHANGE_SPAN, has_plan & (ns['stage'][:, STAGE_WAIT] > 0.5)]
        for s in range(SLOTS):
            rows.append(tok(3, age=mx.minimum(ns['pull_age'][:, s], 9.0)))
            masks.append(ns['pull'][:, s] > 0.5)
        toks = mx.stack(rows, axis=1)
        mask = mx.stack(masks, axis=1)
        slot = mx.concatenate([mx.full((b, 3), -1, dtype=mx.int32), mx.broadcast_to(mx.arange(SLOTS, dtype=mx.int32)[None], (b, SLOTS))], axis=1)
        return toks, mask, slot

    def body_mn(self, tok, dist, tmask, slot, um, pm, hist, hmask, hslot, hplan, ns):
        nt, nmask, nslot = self.note_tokens(ns)
        ne = self.n2(nn.relu(self.n1(nt)))
        return self.body_mh(tok, dist, tmask, slot, um, pm, hist, hmask, hslot, hplan, extra=(ne, nmask, nslot))

    def note_heads(self, hfull, tok, slot, tmask, um2, pm_old, pm_new, event, stall):
        """The heads at our turn's end. Returns logits: stage [B, 6] (of the new plan), reason [B, 4], pull / ret [B, T]."""
        t = tok.shape[1]
        b = tok.shape[0]
        ht = hfull[:, :t]
        ap = self.aux_p
        stage = ap[0, 0] * pm_new[:, 0, P_STAGE:P_STAGE + STAGES] + ap[0, 1]
        old = mx.argmax(ap[0, 0] * pm_old[:, 0, P_STAGE:P_STAGE + STAGES] + ap[0, 1], axis=1)
        new = mx.argmax(stage, axis=1)
        ind = mx.stack([event, (new == STAGE_HEAL).astype(mx.float32), (new > old).astype(mx.float32), mx.zeros((b,))], axis=1)
        reason = self.reason(ind + 0.0 * stall[:, None])
        us = um2[mx.arange(b)[:, None], mx.maximum(slot, 0)]
        x = mx.concatenate([ht, tok, us], axis=-1)
        pull = self.pull(x)[..., 0]
        ret = self.ret(x)[..., 0]
        return stage, reason, pull, ret

    def write_notes(self, ns, stage_oh, waited, reason_oh, pull_slots, ret_slots, free, valid):
        """The stream after a turn: the new PLAN (its stage one-hot), PLAN_CHANGE when the stage changed, WAIT, the
        units' PULL_BACK_HEAL (on from pull, off from ret). Everything [B, ...]; valid: the play had this turn."""
        had = mx.sum(ns['stage'], axis=1) > 0
        changed = had & (mx.argmax(stage_oh, axis=1) != mx.argmax(ns['stage'], axis=1))
        change_age = mx.where(changed, 0.0, ns['change_age'] + 1.0)
        reason = mx.where(changed[:, None], reason_oh, ns['reason'])
        pull = mx.maximum(ns['pull'], pull_slots) * (1.0 - ret_slots)
        pull_age = mx.where(pull > 0.5, mx.where(ns['pull'] > 0.5, ns['pull_age'] + 1.0, 0.0), 0.0)
        new = {'stage': stage_oh, 'waited': waited, 'free': free, 'change_age': change_age, 'reason': reason, 'pull': pull, 'pull_age': pull_age}
        return {k: mx.where(valid.reshape((-1,) + (1,) * (v.ndim - 1)), v, ns[k]) for k, v in new.items()}
