"""The third side of the scenes of twin packages 4 and 8 (plan 1.9): the camp families' neutral nation and the seven
third-party families' ally / neutral / rival, as real unit slots (`u_side` 2) of the simulator, and the war table.

Layout of a play with a third seat (the loader `remap` rewrites the dump's scene, initial state, moments and events):
  slots 0..P-1        the scene's pieces, as in every other scene (mine, theirs: `n_mine`, `n_theirs`);
  slots P..P+SPARE-1  spare slots: the barbarians' spawns of a camp (`Sim.new_unit` starts at `n_slots` = P);
  slots P+SPARE..     the third seat's pieces (`scene["third"]`), side 2, in the order of its ids.
`scene["n_main"]` = P. The net's tokens read the sides 0 and 1 only: the third is no token (as in Rust, G4: "invisible to
the net"); the simulator's rules see it as a unit of another player (it blocks stops and walks).

The war table `s["war"]` [B, 3, 3] bool (absent without a third seat: every old expression, `Sim.is_foe`, runs as before):
the barbarians / the enemy are at war with the learner and with the third; the learner and the third are at peace
(the rules refuse every strike between them). `neutral-joins-war` flips the pair at the declaration; the allied families
put the third at war with the enemy only (the same table).
"""

import mlx.core as mx

from .sim import g1, I32

SPARE = 3
WIDTH = 24  # the window's width: a tile id is r * WIDTH + q

CAMP = ("camp", "camp-race")
THIRD = ("neutral-path", "ally-siege", "defend-friend", "closed-borders", "spare-friends", "neutral-joins-war", "capital-race")


def family(play_name):
    return play_name.split("/")[0]


def has_third(scene_line):
    return scene_line["scene"].get("third") is not None


def _near(tile, camp):
    """A window tile on the camp or next to it (hex distance <= 1): where a camp's spawn stands (`free_tile_near`)."""
    if tile < 0 or camp < 0:
        return False
    dq, dr = tile % WIDTH - camp % WIDTH, tile // WIDTH - camp // WIDTH
    return max(abs(dq), abs(dr), abs(dq + dr)) <= 1


def _empty():
    return [0, -1, 0, 0, 0, 0, 0, 0, -1, -1, 0, "", 0]


def remap_play(play):
    """Rewrite one play [scene line, moments, end] into the layout above (in place, once)."""
    sc, ms, _ = play
    if not has_third(sc) or sc["scene"].get("n_main") is not None:
        return play
    scene = sc["scene"]
    third = scene["third"]
    P = len(scene["profiles"])
    spare = SPARE if third.get("camp", -1) >= 0 else 0
    scene["n_main"] = P
    scene["n_spare"] = spare
    ids3 = list(third["ids"])
    k3 = len(ids3)
    base = scene["profiles"][-1]
    scene["profiles"] = scene["profiles"] + [dict(base) for _ in range(spare)] + list(third["profiles"])
    scene["third_slot0"] = P + spare

    # The barbarians' spawns of a camp take the next free slot of Rust's list, behind the strays of the fog camps the arena
    # closes (dead rows): the spare slots hold the units that first appear alive on the camp tile, in that order.
    smap = {}
    camp = third.get("camp", -1)
    if spare:
        rows = [list(u) for u in sc["initial"]["units"]]
        for e in sc.get("events", []):
            for k, row in e["u"]:
                while len(rows) <= k:
                    rows.append(_empty())
                rows[k] = row
                if k >= P and row[0] and row[8] == 1 and _near(row[1], camp) and k not in smap and len(smap) < spare:
                    smap[k] = len(smap)
        for m in ms:
            for k, u in enumerate(m["state"]["units"]):
                if k >= P and u[0] and u[8] == 1 and _near(u[1], camp) and k not in smap and len(smap) < spare:
                    smap[k] = len(smap)
    back = {v: k for k, v in smap.items()}
    scene["spare_map"] = {str(k): v for k, v in smap.items()}

    def units_of(st):
        us = st["units"]
        out = [list(u) for u in us[:P]]
        out += [_empty() for _ in range(P - len(out))]
        for j in range(spare):
            k = back.get(j)
            out.append(list(us[k]) if k is not None and k < len(us) else _empty())
        out += [list(u) for u in (st.get("third") or [])]
        out += [_empty() for _ in range(P + spare + k3 - len(out))]
        return out

    def fix_state(st):
        if st.get("third") is None:
            return
        st["units"] = units_of(st)
        tc = st.get("third_cities") or []
        for k, c in enumerate(st["cities"]):
            if k < len(tc) and tc[k]:
                c[0] = 2

    def fix_list(v, pad, third):
        v = list(v)
        out = v[:P] + [pad] * (P - len(v))
        for j in range(spare):
            k = back.get(j)
            out.append(v[k] if k is not None and k < len(v) else pad)
        return out + (third if third is not None else [pad] * k3)

    fix_state(sc["initial"])
    for m in ms:
        fix_state(m["state"])
        if "prev" in m and spare:
            # The turn-start snapshot (slot, tile, hp): a spawn's Rust slot is its spare slot here.
            m["prev"] = [[(p_[0] if p_[0] < P else (P + smap[p_[0]] if p_[0] in smap else -1))] + list(p_[1:]) for p_ in m["prev"]]
            m["prev"] = [p_ for p_ in m["prev"] if p_[0] >= 0]
        if "ids" in m:
            m["ids"] = fix_list(m["ids"], -1, ids3)
        if "handles" in m:
            m["handles"] = fix_list(m["handles"], -1, None)
    standing = set()  # the Rust slots that hold a spawn of the camp now (not a stray of the arena)
    for e in sc.get("events", []):
        if "t" not in e:
            continue
        u = []
        for k, row in e["u"]:
            if k < P:
                u.append([k, row])
            elif k in smap:
                # The fog camps' strays (placed at the barbarians' turn end, taken off before our next command) reuse the
                # recycled slots: a row of a spare slot counts only while the camp's own spawn lives in it.
                if not row[0]:
                    standing.discard(k)
                elif _near(row[1], camp):
                    standing.add(k)
                elif k not in standing:
                    continue
                u.append([P + smap[k], row])
        u += [[P + spare + k, row] for k, row in e["t"]]
        e["u"] = u
        e["t"] = []
    return play


def war_for(families):
    """[n, 3, 3] bool: each row's war table at the play's start, by family."""
    rows = []
    for fam in families:
        w = [[False] * 3 for _ in range(3)]
        w[0][1] = w[1][0] = True
        if fam in CAMP:
            w[1][2] = w[2][1] = True
        elif fam in THIRD:
            w[1][2] = w[2][1] = fam not in ("neutral-path", "closed-borders", "spare-friends", "neutral-joins-war")
            # neutral-path, closed-borders, spare-friends: the third is at peace with both; neutral-joins-war: at peace until
            # the declaration (round `join_at`: the learner and the third are at war from then on).
            if fam == "capital-race":
                w[1][2] = w[2][1] = True
        rows.append(w)
    return mx.array(rows)


def template_slots(tb, scenes):
    """The templates' `n_slots` (the next free slot) and `n_cap` (the first slot a spawn may not take): a play with a third
    seat starts its spawns at P and may fill the spare slots only."""
    n = tb.s0_all["n_slots"].tolist()
    cap = [tb.u] * len(n)
    first = [0] * len(n)
    for i, sc in enumerate(scenes):
        if has_third(sc):
            P = sc["scene"]["n_main"]
            n[i] = P
            cap[i] = P + sc["scene"]["n_spare"]
            first[i] = P
    tb.s0_all["n_slots"] = mx.array(n, dtype=I32)
    tb.s0_all["n_cap"] = mx.array(cap, dtype=I32)
    tb.s0_all["n_first"] = mx.array(first, dtype=I32)


def setup_rows(sim, s, rows):
    """The war table, the camp's state and the spawned barbarians' profiles of the rows [(scene line, moment)]."""
    if not any(has_third(sc) for sc, _ in rows):
        return s
    fams = [family(sc["play"]) for sc, _ in rows]
    s["war"] = war_for(fams)
    s["barb"] = mx.array([f in CAMP for f in fams])  # side 1 is the barbarians: the bonus against them
    tile, up, cd = [], [], []
    for sc, m in rows:
        t = sc["scene"]["third"]["camp"] if has_third(sc) else -1
        st = m["state"]
        c = st.get("camp", -1) if st.get("camp") is not None else -1
        tile.append(t)
        up.append(t >= 0 and c >= 0)
        cd.append(max(c, 0))
    B = len(rows)
    s["camp_tile"] = mx.array(tile, dtype=I32)
    s["camp_up"] = mx.array(up)
    s["camp_cd"] = mx.array(cd, dtype=I32)
    s["camp_by"] = mx.full((B,), -1, dtype=I32)
    s["camp_rel"] = mx.full((B,), -1, dtype=I32)
    s["lost_at_clear"] = mx.full((B,), -1, dtype=I32)
    s["camp_row"] = mx.array([sc["scene"].get("camp_spawn", -1) if has_third(sc) else -1 for sc, _ in rows], dtype=I32)
    # A barbarian spawn already in the state (a forced moment): its profile is the camp's spawn row.
    n_main = mx.array([sc["scene"].get("n_main", 0) if has_third(sc) else 0 for sc, _ in rows], dtype=I32)
    for k in range(SPARE):
        slot = n_main + k
        known = (s["camp_row"] >= 0) & g1(s["u_alive"], slot) & (g1(s["u_side"], slot) == 1)
        if bool(mx.any(known).item()):
            s = sim.put_spawn_profile(s, slot, known, s["camp_row"])
    return s
