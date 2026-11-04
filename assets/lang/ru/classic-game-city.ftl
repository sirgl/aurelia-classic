# Game layer: the city screen data (game/city.rs).

classic.game.city.subtitle = { $nation }  ·  УПРАВЛЕНИЕ ГОРОДОМ
classic.game.city.idle-none = Производство не выбрано
classic.game.city.idle-wealth = Производство превращается в золото
classic.game.city.idle-research = Производство превращается в науку
classic.game.city.idle-contribute = Вклад в проект: { $project }
classic.game.city.kind-unit = ОТРЯД
classic.game.city.kind-building = ЗДАНИЕ
classic.game.city.kind-wonder = ЧУДО
classic.game.city.kind-project = ПРОЕКТ
classic.game.city.buy = Купить
classic.game.city.buy-price = Купить · { NUMBER($price) } золота
classic.game.city.citizens-line = На клетках: { $tiles }  ·  { $specialists ->
    [one] { $specialists } специалист
    [few] { $specialists } специалиста
    [many] { $specialists } специалистов
   *[other] { $specialists } специалиста
}  ·  без работы: { $unemployed }
classic.game.city.process-none = Без превращения
classic.game.city.process-wealth = Богатство
classic.game.city.process-research = Исследования
classic.game.city.land-unexplored = Неисследованная земля
classic.game.city.amount-food = еда: { $n }
classic.game.city.amount-production = производство: { $n }
classic.game.city.amount-gold = золото: { $n }
classic.game.city.amount-science = наука: { $n }
classic.game.city.amount-culture = культура: { $n }
classic.game.city.amount-faith = вера: { $n }
classic.game.city.short-food = еда { $n }
classic.game.city.short-production = пр. { $n }
classic.game.city.short-gold = золото { $n }
classic.game.city.short-science = наука { $n }
classic.game.city.short-culture = культура { $n }
classic.game.city.short-faith = вера { $n }
classic.game.city.nothing = ничего
classic.game.city.renamed = { $old } теперь называется { $new }
classic.game.city.golden-age-turns = Золотой век · { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.game.city.golden-age-meter = Золотой век { $meter }/{ $threshold }
classic.game.city.starving = Голод: { $n } еды за ход
classic.game.city.not-growing = Рост остановлен
classic.game.city.grows-in = Рост через { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.game.city.row-happiness = Счастье
classic.game.city.row-upkeep = Содержание
classic.game.city.row-food = Еда
classic.game.city.row-production = Производство
classic.game.city.row-science = Наука
classic.game.city.row-gold = Золото
classic.game.city.row-culture = Культура
classic.game.city.row-faith = Вера
classic.game.city.row-defence = Защита
classic.game.city.row-hit-points = Здоровье
classic.game.city.no-effect = Пока без эффекта
