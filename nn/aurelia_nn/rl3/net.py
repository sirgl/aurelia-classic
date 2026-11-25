"""The nets rl3 trains: N175 as it is, and its typed conversion (MBTN: typed tokens and third parties on top of the notes)."""
import mlx.core as mx

# The parameters of the new (typed) part: a higher lr group in the loop (`--new-lr-mult`). m_in's new columns are inside m_in.weight.
NEW_PREFIXES = ("ty_obj1", "ty_obj2", "ty_city1", "ty_city2", "tp_u1", "tp_u2", "tp_c1", "tp_c2", "ty_emb", "ty_x", "tp_x", "tp_null", "tp_dist", "tp_o", "cx", "c_new")


def load_any(path):
    """A net from its weights: a typed one (`ty_emb`) as MBTN (with the notes) or MBT, any other as `armyloop.load_net` takes it."""
    w = mx.load(path)
    if "ty_emb" not in w:
        from aurelia_nn.tacsim.armyloop import load_net
        return load_net(path)
    from aurelia_nn.army.model_mb import MBTNet
    from aurelia_nn.army.model_mbtn import MBTNNet
    d, ff = w["e2.weight"].shape[0], w["layers.0.f1.weight"].shape[0]
    layers = 1 + max(int(k.split(".")[1]) for k in w if k.startswith("layers."))
    cls = MBTNNet if "nx_slope" in w else MBTNet
    model = cls(d=d, heads=w["layers.0.dist_bias"].shape[0], layers=layers, ff=ff, promos=w["promo"].shape[0] - 1, map_ch=w["m_in.weight"].shape[0])
    model.load_weights(path)
    model.eval()
    return model


def is_new(name):
    return any(name == p or name.startswith(p + ".") for p in NEW_PREFIXES)
