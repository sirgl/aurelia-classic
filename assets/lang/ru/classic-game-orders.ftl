# Game layer: unit orders, their details and the combat notices (game/orders.rs).

classic.game.orders.build = Построить: { $name }
classic.game.orders.build-turns = Построить: { $name } ({ $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
})
classic.game.orders.status-working = Работает · осталось { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.game.orders.status = Здоровье { $hp } · Движение { $left }/{ $max }
classic.game.orders.status-embarked = Здоровье { $hp } · Движение { $left }/{ $max } · На корабле
classic.game.orders.status-fortified = Здоровье { $hp } · Движение { $left }/{ $max } · Укреплён
classic.game.orders.status-embarked-fortified = Здоровье { $hp } · Движение { $left }/{ $max } · На корабле · Укреплён
classic.game.orders.status-intercepting = Здоровье { $hp } · Движение { $left }/{ $max } · Перехват
classic.game.orders.mark-intercepting = Перехват
classic.game.orders.remove = Убрать: { $name } ({ $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
})
classic.game.orders.remove-production = Убрать: { $name } ({ $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}, +{ $amount } производства)
classic.game.orders.great-work-created = Великое произведение создано
classic.game.orders.treatise-written = Трактат написан
classic.game.orders.concert-performed = Концерт дан
classic.game.orders.no-enemy = Там нет видимого врага
classic.game.orders.declared-war = Вы объявили войну: { $name }
classic.game.orders.attack = Ваша атака: { $before } → { $after }
classic.game.orders.hp = здоровья: { $n }
classic.game.orders.destroyed = уничтожен
classic.game.orders.report-units = ваш — { $mine }, их — { $theirs }
classic.game.orders.captured = захвачен
classic.game.orders.gone = исчез
classic.game.orders.report-city = ваш — { $mine }, город — { $city }
classic.game.orders.disband-gold = Распустить за { NUMBER($gold) } золота
classic.game.orders.disband-no-gold = Вне своей земли — без золота
classic.game.orders.confirm-disband-title = Распустить отряд: { $name }?
classic.game.orders.confirm-disband-gold = Отряд исчезнет навсегда; казна получит { NUMBER($gold) } золота.
classic.game.orders.confirm-disband-line = Отряд исчезнет навсегда. Вне своей земли золота за него не будет.
classic.game.orders.confirm-disband = Распустить
classic.game.orders.disbanded = Отряд распущен: { $name }
classic.game.orders.disbanded-gold = Отряд распущен: { $name }, казна получила { NUMBER($gold) } золота
