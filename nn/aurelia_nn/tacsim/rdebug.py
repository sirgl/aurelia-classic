"""Rust `Debug` text as Python values (the econ dump writes compiled effects and a few enums so).

    AddYield { yield_kind: Food, amount: 2 }  ->  {"AddYield": {"yield_kind": "Food", "amount": 2}}
    HasTechnology(TechId(5))                  ->  {"HasTechnology": [{"TechId": [5]}]}
    Some(3) -> [3] wrapped as {"Some": [3]};  None -> "None";  [1, 2] -> [1, 2]

`arg(v)` unwraps a one-field tuple (`TechId(5)` -> 5), `tag(v)` names a variant.
"""

import re

_TOKEN = re.compile(r'\s*(?:(-?\d+)|([A-Za-z_][A-Za-z_0-9]*)|("(?:[^"\\]|\\.)*")|(.))')


def _tokens(s):
    out = []
    for m in _TOKEN.finditer(s):
        num, ident, string, ch = m.groups()
        if num is not None:
            out.append(("n", int(num)))
        elif ident is not None:
            out.append(("i", ident))
        elif string is not None:
            out.append(("s", string[1:-1]))
        elif ch is not None and not ch.isspace():
            out.append(("p", ch))
    return out


def parse(s):
    toks = _tokens(s)
    pos = 0

    def peek():
        return toks[pos] if pos < len(toks) else ("p", "")

    def take(kind=None, val=None):
        nonlocal pos
        t = toks[pos]
        if (kind and t[0] != kind) or (val is not None and t[1] != val):
            raise ValueError(f"expected {kind} {val} at {pos} in {s!r}, got {t}")
        pos += 1
        return t

    def value():
        k, v = peek()
        if k == "n" or k == "s":
            take()
            return v
        if k == "p" and v == "[":
            take()
            items = []
            while peek() != ("p", "]"):
                items.append(value())
                if peek() == ("p", ","):
                    take()
            take("p", "]")
            return items
        if k == "p" and v == "(":
            take()
            items = []
            while peek() != ("p", ")"):
                items.append(value())
                if peek() == ("p", ","):
                    take()
            take("p", ")")
            return {"()": items}
        name = take("i")[1]
        if name in ("true", "false"):
            return name == "true"
        k2, v2 = peek()
        if k2 == "p" and v2 == "(":
            take()
            items = []
            while peek() != ("p", ")"):
                items.append(value())
                if peek() == ("p", ","):
                    take()
            take("p", ")")
            return {name: items}
        if k2 == "p" and v2 == "{":
            take()
            fields = {}
            while peek() != ("p", "}"):
                key = take("i")[1]
                take("p", ":")
                fields[key] = value()
                if peek() == ("p", ","):
                    take()
            take("p", "}")
            return {name: fields}
        return name

    v = value()
    return v


def tag(v):
    """The variant's name."""
    if isinstance(v, str):
        return v
    if isinstance(v, dict) and len(v) == 1:
        return next(iter(v))
    return None


def body(v):
    """The variant's payload: a list (tuple variant), a dict (struct variant) or None."""
    if isinstance(v, dict) and len(v) == 1:
        return next(iter(v.values()))
    return None


def arg(v):
    """`TechId(5)` -> 5; `Some(x)` -> arg(x); `None` -> None; a plain value stays."""
    if v == "None":
        return None
    if isinstance(v, dict) and len(v) == 1:
        b = next(iter(v.values()))
        if isinstance(b, list) and len(b) == 1:
            return arg(b[0])
    return v
