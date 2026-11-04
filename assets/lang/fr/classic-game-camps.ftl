# Translation of classic-game-camps.ftl into fr. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.game.camp.cleared = Camp barbare détruit : +{ NUMBER($gold) } or
classic.game.camp.cleared-joined = Camp barbare détruit : +{ NUMBER($gold) } or ; un barbare vous a rejoint (+{ NUMBER($joined) } or)
classic.game.camp.joined = Un barbare du camp vous a rejoint : +{ NUMBER($gold) } or

classic.game.income.gold = +{ NUMBER($n) } or
classic.game.income.culture = +{ NUMBER($n) } culture
classic.game.income.science = +{ NUMBER($n) } science
classic.game.income.food = +{ NUMBER($n) } nourriture
classic.game.income.production = +{ NUMBER($n) } production
classic.game.income.faith = +{ NUMBER($n) } foi
classic.game.income.and = { $a }, { $b }
classic.game.kill.notice = { $source } : { $parts } { $barbarian ->
    [yes] pour un barbare tué
   *[no] pour un ennemi tué
}
classic.game.kill.notice-plain = { $parts } { $barbarian ->
    [yes] pour un barbare tué
   *[no] pour un ennemi tué
}
classic.game.plunder.improvement = Aménagement pillé : +{ NUMBER($gold) } or
classic.game.plunder.city = Ville pillée : +{ NUMBER($gold) } or
classic.game.plunder.route = Route commerciale pillée : +{ NUMBER($gold) } or

classic.game.wonder.found = Merveille naturelle découverte : { $name }
classic.game.wonder.found-with = Merveille naturelle découverte : { $name } : { $parts }
classic.game.wonder.gold-first = +{ NUMBER($n) } or (première à la découvrir)
classic.game.wonder.happiness = +{ NUMBER($n) } bonheur
