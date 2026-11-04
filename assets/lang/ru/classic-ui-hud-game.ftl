# Translation of classic-ui-hud-game.ftl into ru. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.ui.hud-game.top-gold = { $gold } ({ $delta }) золота
classic.ui.hud-game.top-food = { $food } еды
classic.ui.hud-game.top-production = { $production } производства
classic.ui.hud-game.top-science = { $science } ({ $delta }) науки
classic.ui.hud-game.top-happiness = { $happiness } счастья
classic.ui.hud-game.top-culture = { $culture } ({ $delta }) культуры
classic.ui.hud-game.top-faith = { $faith } ({ $delta }) веры

## верхняя панель: культура и вера до следующего шага («24/45»), подсказка о щелчке и предложения подсказок
classic.ui.hud-game.top-culture-goal = { $have }/{ $cost } культуры
classic.ui.hud-game.top-faith-goal = { $have }/{ $cost } веры
classic.ui.hud-game.click-to-open = Нажмите, чтобы открыть: { $screen }
classic.ui.hud-game.goal-research-ready = Можно выбрать технологию: сейчас ничего не изучается
classic.ui.hud-game.goal-policy-ready = Институт можно принять прямо сейчас
classic.ui.hud-game.goal-policy-turns = Ещё { $missing } культуры до следующего института: { $turns ->
    [one] { $turns } ход
    [few] { $turns } хода
    [many] { $turns } ходов
   *[other] { $turns } хода
} при +{ $rate } за ход
classic.ui.hud-game.goal-policy-stalled = Ещё { $missing } культуры до следующего института: — (нет дохода культуры)
classic.ui.hud-game.goal-pantheon-ready = Пантеон можно основать прямо сейчас
classic.ui.hud-game.goal-pantheon-turns = Ещё { $missing } веры до пантеона: { $turns ->
    [one] { $turns } ход
    [few] { $turns } хода
    [many] { $turns } ходов
   *[other] { $turns } хода
} при +{ $rate } за ход
classic.ui.hud-game.goal-pantheon-stalled = Ещё { $missing } веры до пантеона: — (нет дохода веры)
classic.ui.hud-game.goal-reformation-ready = Верование Реформации можно добавить прямо сейчас
classic.ui.hud-game.goal-prophet-turns = Ещё { $missing } веры до следующего Великого пророка: { $turns ->
    [one] { $turns } ход
    [few] { $turns } хода
    [many] { $turns } ходов
   *[other] { $turns } хода
} при +{ $rate } за ход
classic.ui.hud-game.goal-prophet-stalled = Ещё { $missing } веры до следующего Великого пророка: — (нет дохода веры)
classic.ui.hud-game.open-research = Открыть исследования
classic.ui.hud-game.research-kicker = ИССЛЕДОВАНИЕ
classic.ui.hud-game.research-choose = Выбрать исследование
classic.ui.hud-game.research-idle = Ничего не изучается
classic.ui.hud-game.research-detail = { $stored }/{ $cost }  ·  { $n ->
    [one] { $n } ход
    [few] { $n } хода
    [many] { $n } ходов
   *[other] { $n } хода
}
classic.ui.hud-game.research-detail-stalled = { $stored }/{ $cost }  ·  науки пока нет
classic.ui.hud-game.rival-war = { $name } · война
classic.ui.hud-game.rival-peace = { $name } · мир
classic.ui.hud-game.make-peace = Заключить мир
classic.ui.hud-game.open-borders = Открытые границы
classic.ui.hud-game.blocked-reason = { $action }: { $reason }
classic.ui.hud-game.notice-line = Ход { $turn }  { $text }
classic.ui.hud-game.notice-hint = Показать на карте · F7 — полный журнал
classic.ui.hud-game.capture-title = { $city } теперь ваш
classic.ui.hud-game.capture-line = Основал { $founder }. Решите его судьбу.
classic.ui.hud-game.unavailable = Недоступно
classic.ui.hud-game.ruins-title = Древние руины
classic.ui.hud-game.ruins-line = Ваш юнит нашёл руины. Выберите одну из трёх наград.
classic.ui.hud-game.ruins-later = Позже
classic.ui.hud-game.ruins-later-hint = Руины останутся на карте
classic.ui.hud-game.victory = Победа
classic.ui.hud-game.victory-line = Ваша нация выиграла игру.
classic.ui.hud-game.defeat = Поражение
classic.ui.hud-game.defeat-line = Ваша нация проиграла игру.
classic.ui.hud-game.see-results = Посмотреть итоги

## city-state card over the map
classic.ui.hud-game.city-state-gold = Золото в казне: { $gold }
classic.ui.hud-game.city-state-close = Закрыть (Esc)
