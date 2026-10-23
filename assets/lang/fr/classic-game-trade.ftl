# Translation of classic-game-trade.ftl into fr. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.game.trade.offer = { $city } · { $kind } · { $n ->
    [one] { $n } case
   *[other] { $n } cases
}
classic.game.trade.summary-active = { $n ->
    [one] { $n } route active
   *[other] { $n } routes actives
}
classic.game.trade.summary-idle = { $n ->
    [one] { $n } marchand inactif
   *[other] { $n } marchands inactifs
}
classic.game.trade.summary-queued = { $n } en production
classic.game.trade.summary-capacity = Capacité occupée : { $used }/{ $possible }
classic.game.trade.established = Route commerciale établie vers { $city }
classic.game.trade.ended = La route commerciale de { $from } vers { $to } a pris fin
classic.game.trade.kind-international = Or et science
classic.game.trade.kind-food = Nourriture
classic.game.trade.kind-production = Production
classic.game.trade.label-caravan = Caravane
classic.game.trade.label-cargo-ship = Cargo
classic.game.trade.yield-gold = +{ NUMBER($amount, minimumFractionDigits: 2) } or
classic.game.trade.yield-science = +{ NUMBER($amount, minimumFractionDigits: 2) } science
classic.game.trade.yield-food = +{ NUMBER($amount, minimumFractionDigits: 2) } nourriture
classic.game.trade.yield-production = +{ NUMBER($amount, minimumFractionDigits: 2) } production
classic.game.trade.nothing = rien
