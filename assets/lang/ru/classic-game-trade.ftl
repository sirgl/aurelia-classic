# Game layer: trade routes in the overview and their notices (game/trade.rs).

classic.game.trade.offer = { $city } · { $kind } · { $n ->
    [one] { $n } клетка
    [few] { $n } клетки
    [many] { $n } клеток
   *[other] { $n } клетки
}
classic.game.trade.summary-active = { $n ->
    [one] { $n } действующий путь
    [few] { $n } действующих пути
    [many] { $n } действующих путей
   *[other] { $n } действующих пути
}
classic.game.trade.summary-idle = { $n ->
    [one] { $n } торговец без дела
    [few] { $n } торговца без дела
    [many] { $n } торговцев без дела
   *[other] { $n } торговца без дела
}
classic.game.trade.summary-queued = { $n } в производстве
classic.game.trade.summary-capacity = занято { $used } из { $possible }
classic.game.trade.established = Торговый путь в город { $city } открыт
classic.game.trade.ended = Торговый путь из города { $from } в город { $to } закончился
classic.game.trade.kind-international = Золото и наука
classic.game.trade.kind-food = Еда
classic.game.trade.kind-production = Производство
classic.game.trade.label-caravan = Караван
classic.game.trade.label-cargo-ship = Грузовой корабль
classic.game.trade.yield-gold = +{ NUMBER($amount, minimumFractionDigits: 2) } золота
classic.game.trade.yield-science = +{ NUMBER($amount, minimumFractionDigits: 2) } науки
classic.game.trade.yield-food = +{ NUMBER($amount, minimumFractionDigits: 2) } еды
classic.game.trade.yield-production = +{ NUMBER($amount, minimumFractionDigits: 2) } производства
classic.game.trade.nothing = ничего
