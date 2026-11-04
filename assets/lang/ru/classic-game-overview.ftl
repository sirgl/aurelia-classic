# Game layer: the advisor overviews and the top-bar breakdowns (game/overview.rs).

classic.game.overview.producing-nothing = Ничего
classic.game.overview.modifiers-empire = Бонусы государства
classic.game.overview.maintenance-units = Содержание отрядов
classic.game.overview.maintenance-buildings = Содержание зданий
classic.game.overview.maintenance-roads = Содержание дорог
classic.game.overview.city-connections = Связь городов
classic.game.overview.food-surplus = { $value } за ход
classic.game.overview.per-turn = { $value } за ход
classic.game.overview.total-with-rate = { NUMBER($stock) } ({ $rate } за ход)
classic.game.overview.total-stored = Накоплено: { NUMBER($stock) } ({ $rate } за ход)
classic.game.overview.tip-gold = Золото
classic.game.overview.tip-food = Еда
classic.game.overview.tip-production = Производство
classic.game.overview.tip-science = Наука
classic.game.overview.tip-culture = Культура
classic.game.overview.tip-faith = Вера
classic.game.overview.note-food = Каждый житель съедает 2 еды; излишек даёт рост города.
classic.game.overview.note-culture = На культуру покупают социальные институты.
classic.game.overview.note-faith = Вера нужна для пантеонов и религий.
classic.game.overview.demo-population = Население
classic.game.overview.demo-population-unit = жителей
classic.game.overview.demo-crop = Урожай
classic.game.overview.demo-crop-unit = еды
classic.game.overview.demo-goods = Товары
classic.game.overview.demo-goods-unit = производства
classic.game.overview.demo-gnp = Валовой национальный продукт
classic.game.overview.demo-gnp-unit = золота за ход
classic.game.overview.demo-literacy = Грамотность
classic.game.overview.demo-literacy-unit = % технологий
classic.game.overview.demo-land = Земля
classic.game.overview.demo-land-unit = клеток
classic.game.overview.demo-soldiers = Солдаты
classic.game.overview.demo-soldiers-unit = боевых отрядов
classic.game.overview.more-cities = Ещё { $n ->
    [one] { $n } город
    [few] { $n } города
    [many] { $n } городов
   *[other] { $n } города
}
classic.game.overview.no-cities = Городов пока нет
classic.game.overview.status-working = Работает · { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.game.overview.status-fortified = Укреплён
classic.game.overview.status-intercepting = Перехват
classic.game.overview.status-ready = Готов
classic.game.overview.status-done = Закончил
classic.game.overview.status-ready-embarked = Готов · На корабле
classic.game.overview.status-done-embarked = Закончил · На корабле

## banner of a met city-state
classic.game.overview.city-state-tip =
    { $name } · город-государство
    Сила обороны { $strength }
    Щёлкните, чтобы поговорить
