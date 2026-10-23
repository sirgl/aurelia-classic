# Civ V game layer: victory progress, the end of the game, the Hall of Fame line (game/victory.rs).

classic.game.victory.row-domination = Господство
classic.game.victory.row-science = Наука
classic.game.victory.row-culture = Культура
classic.game.victory.row-diplomacy = Дипломатия
classic.game.victory.row-time = Время
classic.game.victory.goal-domination = Удержать все исходные столицы, включая одну чужую
classic.game.victory.goal-science = Построить программу «Аполлон» и установить все части космического корабля
classic.game.victory.goal-culture = Влиять на все остальные нации
classic.game.victory.goal-diplomacy = Выиграть голосование за Мирового лидера в ООН
classic.game.victory.goal-time = Набрать больше всех очков к ходу { $last }
classic.game.victory.progress-domination-lone = Удержано { $held } из { $total ->
    [one] { $total } основанной столицы
    [few] { $total } основанных столиц
    [many] { $total } основанных столиц
   *[other] { $total } основанных столиц
} · Чужих столиц не захвачено
classic.game.victory.progress-domination = Удержано { $held } из { $total ->
    [one] { $total } исходной столицы
    [few] { $total } исходных столиц
    [many] { $total } исходных столиц
   *[other] { $total } исходных столиц
}
classic.game.victory.progress-science-built = Программа «Аполлон» построена  ·  частей корабля: { $have } из { $need }
classic.game.victory.progress-science = Программа «Аполлон» не построена  ·  частей корабля: { $have } из { $need }
classic.game.victory.progress-culture = Влияние на { $have } из { $need ->
    [one] { $need } нации
    [few] { $need } наций
    [many] { $need } наций
   *[other] { $need } наций
}
classic.game.victory.progress-diplomacy-leader = Мировой лидер: { $name }
classic.game.victory.progress-diplomacy-no-congress = Всемирный конгресс ещё не основан
classic.game.victory.progress-diplomacy-no-un = Голосов: { $have } из { $need ->
    [one] { $need } делегата
    [few] { $need } делегатов
    [many] { $need } делегатов
   *[other] { $need } делегатов
}  ·  ООН ещё не основана
classic.game.victory.progress-diplomacy = Голосов: { $have } из { $need ->
    [one] { $need } делегата
    [few] { $need } делегатов
    [many] { $need } делегатов
   *[other] { $need } делегатов
}
classic.game.victory.progress-time = Ход { $turn } из { $last }
classic.game.victory.title-victory = Победа
classic.game.victory.title-defeat = Поражение
classic.game.victory.end-fallen = Ваша нация пала: не осталось ни города, ни отряда.
classic.game.victory.end-won = Вы победили на ходу { $turn }: { $victory }.
classic.game.victory.end-lost = Победитель — { $winner }: { $victory } на ходу { $turn }.
classic.game.victory.hall-defeat = Поражение ({ $winner }: { $victory })
classic.game.victory.hall-eliminated = Выбыла из игры
classic.game.victory.hall-line = { $nation } · { $result } · ход { $turn } · очки { $score }
classic.game.victory.one-more-turn = Ещё один ход
classic.game.victory.kind-time = Победа по времени
classic.game.victory.kind-science = Научная победа
classic.game.victory.kind-domination = Победа господством
classic.game.victory.kind-cultural = Культурная победа
classic.game.victory.kind-diplomatic = Дипломатическая победа
