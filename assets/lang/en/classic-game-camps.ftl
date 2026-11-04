# Game layer: the notices of a cleared barbarian camp (game/camps.rs), of income taken by a kill or by force, and of a natural wonder found (game/income.rs).

classic.game.camp.cleared = Barbarian camp cleared: +{ NUMBER($gold) } gold
classic.game.camp.cleared-joined = Barbarian camp cleared: +{ NUMBER($gold) } gold; a barbarian joined you (+{ NUMBER($joined) } gold)
classic.game.camp.joined = A barbarian from the camp joined you: +{ NUMBER($gold) } gold

classic.game.income.gold = +{ NUMBER($n) } gold
classic.game.income.culture = +{ NUMBER($n) } culture
classic.game.income.science = +{ NUMBER($n) } science
classic.game.income.food = +{ NUMBER($n) } food
classic.game.income.production = +{ NUMBER($n) } production
classic.game.income.faith = +{ NUMBER($n) } faith
classic.game.income.and = { $a }, { $b }
classic.game.kill.notice = { $source }: { $parts } { $barbarian ->
    [yes] for a slain barbarian
   *[no] for a kill
}
classic.game.kill.notice-plain = { $parts } { $barbarian ->
    [yes] for a slain barbarian
   *[no] for a kill
}
classic.game.plunder.improvement = Improvement pillaged: +{ NUMBER($gold) } gold
classic.game.plunder.city = City plundered: +{ NUMBER($gold) } gold
classic.game.plunder.route = Trade route plundered: +{ NUMBER($gold) } gold

classic.game.wonder.found = Natural wonder found: { $name }
classic.game.wonder.found-with = Natural wonder found: { $name }: { $parts }
classic.game.wonder.gold-first = +{ NUMBER($n) } gold (first to find it)
classic.game.wonder.happiness = +{ NUMBER($n) } happiness
