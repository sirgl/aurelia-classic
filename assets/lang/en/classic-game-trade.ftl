# Civ V game layer: trade routes in the overview and their notices (game/trade.rs).

classic.game.trade.offer = { $city } · { $kind } · { $n ->
    [one] { $n } tile
   *[other] { $n } tiles
}
classic.game.trade.summary-active = { $n ->
    [one] { $n } active route
   *[other] { $n } active routes
}
classic.game.trade.summary-idle = { $n ->
    [one] { $n } idle trader
   *[other] { $n } idle traders
}
classic.game.trade.summary-queued = { $n } in production
classic.game.trade.summary-capacity = { $used }/{ $possible } capacity occupied
classic.game.trade.established = Trade route to { $city } established
classic.game.trade.ended = The trade route from { $from } to { $to } has ended
classic.game.trade.kind-international = Gold and science
classic.game.trade.kind-food = Food
classic.game.trade.kind-production = Production
classic.game.trade.label-caravan = Caravan
classic.game.trade.label-cargo-ship = Cargo ship
classic.game.trade.yield-gold = +{ NUMBER($amount, minimumFractionDigits: 2) } gold
classic.game.trade.yield-science = +{ NUMBER($amount, minimumFractionDigits: 2) } science
classic.game.trade.yield-food = +{ NUMBER($amount, minimumFractionDigits: 2) } food
classic.game.trade.yield-production = +{ NUMBER($amount, minimumFractionDigits: 2) } production
classic.game.trade.nothing = nothing
