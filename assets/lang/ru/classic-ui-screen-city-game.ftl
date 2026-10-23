# Translation of classic-ui-screen-city-game.ftl into ru. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.ui.screen-city-game.select-city = Выберите один из своих городов на карте.
classic.ui.screen-city-game.tab-city = Город
classic.ui.screen-city-game.tab-production = Производство
classic.ui.screen-city-game.rename = Назвать
classic.ui.screen-city-game.finish-buying = Закончить покупку
classic.ui.screen-city-game.buy-tile-mode = Купить клетку на карте
classic.ui.screen-city-game.tab-overview = Обзор
classic.ui.screen-city-game.tab-citizens = Жители
classic.ui.screen-city-game.tab-buildings = Здания
classic.ui.screen-city-game.choose-name = ВЫБЕРИТЕ НАЗВАНИЕ
classic.ui.screen-city-game.population-growth = НАСЕЛЕНИЕ И РОСТ
classic.ui.screen-city-game.citizens = { $n ->
    [one] { $n } житель
    [few] { $n } жителя
    [many] { $n } жителей
   *[other] { $n } жителя
}
classic.ui.screen-city-game.food-progress = { $stored } / { $needed } еды
classic.ui.screen-city-game.output-per-turn = ДОХОД ГОРОДА · ЗА ХОД
classic.ui.screen-city-game.citizen-assignment = РАСПРЕДЕЛЕНИЕ ЖИТЕЛЕЙ
classic.ui.screen-city-game.output-label = Доход за ход
classic.ui.screen-city-game.work-the-map = РАБОТА НА КАРТЕ
classic.ui.screen-city-game.legend-working = Работают
classic.ui.screen-city-game.legend-locked = Закреплены
classic.ui.screen-city-game.legend-available = Доступны
classic.ui.screen-city-game.work-help-click = Левый клик по пустой клетке — назначить и закрепить жителя. Клик по занятой клетке — снять жителя.
classic.ui.screen-city-game.work-help-lock = Правый клик — закрепить или открепить.
classic.ui.screen-city-game.work-help-center = Центр города работает автоматически.
classic.ui.screen-city-game.governor-focus = ПРИОРИТЕТ НАМЕСТНИКА
classic.ui.screen-city-game.avoid-growth = Не давать расти
classic.ui.screen-city-game.specialists = СПЕЦИАЛИСТЫ
classic.ui.screen-city-game.specialist-slots = { $name } · { $n ->
    [one] { $n } ячейка
    [few] { $n } ячейки
    [many] { $n } ячеек
   *[other] { $n } ячейки
}
classic.ui.screen-city-game.specialist-working = работают: { $working } · назначено: { $assigned }
classic.ui.screen-city-game.governor-manages = Эти ячейки распределяет наместник
classic.ui.screen-city-game.remove = Убрать
classic.ui.screen-city-game.assign = Назначить
classic.ui.screen-city-game.great-people = ВЕЛИКИЕ ЛЮДИ
classic.ui.screen-city-game.great-person-progress = { $points } / { $threshold } · +{ $per_turn } / ход
classic.ui.screen-city-game.buildings-heading = { $n ->
    [one] { $n } ПОСТРОЕННОЕ ЗДАНИЕ
    [few] { $n } ПОСТРОЕННЫХ ЗДАНИЯ
    [many] { $n } ПОСТРОЕННЫХ ЗДАНИЙ
   *[other] { $n } ПОСТРОЕННЫХ ЗДАНИЯ
}
classic.ui.screen-city-game.no-buildings = Зданий пока нет. Выберите здание на вкладке «Производство».
classic.ui.screen-city-game.expand = РАСШИРЬТЕ ГОРОД
classic.ui.screen-city-game.gold-available = Доступно золота: { $gold }
classic.ui.screen-city-game.buy-help = Выберите подсвеченную клетку на карте. Цена показана на клетке. Покупка происходит только после подтверждения.
classic.ui.screen-city-game.selected-tile = ВЫБРАННАЯ КЛЕТКА
classic.ui.screen-city-game.price = { $gold } золота
classic.ui.screen-city-game.no-tile = Клетка не выбрана
classic.ui.screen-city-game.outline-legend = 
    Золотая рамка: доступная земля
    Тусклая рамка: сейчас купить нельзя
    Esc: вернуться в город
classic.ui.screen-city-game.choose-tile = Выберите клетку
classic.ui.screen-city-game.buy-tile = Купить клетку · { $price } золота
classic.ui.screen-city-game.production-heading = ПРОИЗВОДСТВО
classic.ui.screen-city-game.search-hint = Поиск отрядов, зданий, чудес
classic.ui.screen-city-game.filter-available = Доступное
classic.ui.screen-city-game.filter-all = Все
classic.ui.screen-city-game.kind-all = Все типы
classic.ui.screen-city-game.kind-units = Юниты
classic.ui.screen-city-game.kind-buildings = Здания
classic.ui.screen-city-game.kind-wonders = Чудеса
classic.ui.screen-city-game.kind-projects = Проекты
classic.ui.screen-city-game.no-match = Подходящего производства нет. Выберите «Все», чтобы увидеть требования.
classic.ui.screen-city-game.choose-available = Выберите доступное
classic.ui.screen-city-game.enqueue = В очередь: { $name }
classic.ui.screen-city-game.cost-turns = { $cost } производства · { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.ui.screen-city-game.cost-no-output = { $cost } производства · нет дохода
classic.ui.screen-city-game.clear = Очистить
classic.ui.screen-city-game.queue-heading = { $n ->
    [one] ОЧЕРЕДЬ · { $n } ЗАДАНИЕ
    [few] ОЧЕРЕДЬ · { $n } ЗАДАНИЯ
    [many] ОЧЕРЕДЬ · { $n } ЗАДАНИЙ
   *[other] ОЧЕРЕДЬ · { $n } ЗАДАНИЯ
}
classic.ui.screen-city-game.queue-progress = { $stored } / { $cost } · { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.ui.screen-city-game.queue-progress-idle = { $stored } / { $cost } · производства нет
classic.ui.screen-city-game.queue-entry = { $n }. { $name } · { $cost } производства
classic.ui.screen-city-game.production-stored = Накоплено производства: { $n }
classic.ui.screen-city-game.convert-production = ОБРАТИТЬ НЕИСПОЛЬЗОВАННОЕ ПРОИЗВОДСТВО
classic.ui.screen-city-game.focus-balanced = Баланс
classic.ui.screen-city-game.focus-food = Еда
classic.ui.screen-city-game.focus-production = Производство
classic.ui.screen-city-game.focus-gold = Золото
classic.ui.screen-city-game.focus-science = Наука
classic.ui.screen-city-game.focus-culture = Культура
classic.ui.screen-city-game.focus-great-people = Великие люди
classic.ui.screen-city-game.focus-faith = Вера
