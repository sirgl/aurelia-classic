# Translation of classic-game-trade.ftl into de. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.game.trade.offer = { $city } · { $kind } · { $n ->
    [one] { $n } Feld
   *[other] { $n } Felder
}
classic.game.trade.summary-active = { $n ->
    [one] { $n } aktive Route
   *[other] { $n } aktive Routen
}
classic.game.trade.summary-idle = { $n ->
    [one] { $n } freier Händler
   *[other] { $n } freie Händler
}
classic.game.trade.summary-queued = { $n } in Produktion
classic.game.trade.summary-capacity = { $used }/{ $possible } Kapazität belegt
classic.game.trade.established = Handelsroute nach { $city } eingerichtet
classic.game.trade.ended = Die Handelsroute von { $from } nach { $to } ist beendet
classic.game.trade.kind-international = Gold und Wissenschaft
classic.game.trade.kind-food = Nahrung
classic.game.trade.kind-production = Produktion
classic.game.trade.label-caravan = Karawane
classic.game.trade.label-cargo-ship = Frachtschiff
classic.game.trade.yield-gold = +{ NUMBER($amount, minimumFractionDigits: 2) } Gold
classic.game.trade.yield-science = +{ NUMBER($amount, minimumFractionDigits: 2) } Wissenschaft
classic.game.trade.yield-food = +{ NUMBER($amount, minimumFractionDigits: 2) } Nahrung
classic.game.trade.yield-production = +{ NUMBER($amount, minimumFractionDigits: 2) } Produktion
classic.game.trade.nothing = nichts
