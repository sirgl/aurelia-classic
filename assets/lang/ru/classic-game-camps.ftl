# Game layer: the notices of a cleared barbarian camp (game/camps.rs), of income taken by a kill or by force, and of a natural wonder found (game/income.rs).

classic.game.camp.cleared = Лагерь варваров разгромлен: +{ NUMBER($gold) } золота
classic.game.camp.cleared-joined = Лагерь варваров разгромлен: +{ NUMBER($gold) } золота; варвар перешёл на вашу сторону (+{ NUMBER($joined) } золота)
classic.game.camp.joined = Варвар из лагеря перешёл на вашу сторону: +{ NUMBER($gold) } золота

classic.game.income.gold = +{ NUMBER($n) } золота
classic.game.income.culture = +{ NUMBER($n) } культуры
classic.game.income.science = +{ NUMBER($n) } науки
classic.game.income.food = +{ NUMBER($n) } еды
classic.game.income.production = +{ NUMBER($n) } производства
classic.game.income.faith = +{ NUMBER($n) } веры
classic.game.income.and = { $a }, { $b }
classic.game.kill.notice = { $source }: { $parts } { $barbarian ->
    [yes] за убитого варвара
   *[no] за убийство
}
classic.game.kill.notice-plain = { $parts } { $barbarian ->
    [yes] за убитого варвара
   *[no] за убийство
}
classic.game.plunder.improvement = Улучшение разграблено: +{ NUMBER($gold) } золота
classic.game.plunder.city = Город разграблен: +{ NUMBER($gold) } золота
classic.game.plunder.route = Торговый путь разграблен: +{ NUMBER($gold) } золота

classic.game.wonder.found = Найдено природное чудо: { $name }
classic.game.wonder.found-with = Найдено природное чудо: { $name }: { $parts }
classic.game.wonder.gold-first = +{ NUMBER($n) } золота (первым нашли вы)
classic.game.wonder.happiness = +{ NUMBER($n) } счастья
