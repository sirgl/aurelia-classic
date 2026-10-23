# Translation of classic-game-orders.ftl into fr. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.game.orders.build = Construire { $name }
classic.game.orders.build-turns = Construire { $name } ({ $n ->
    [one] { $n } tour
   *[other] { $n } tours
})
classic.game.orders.status-working = Travail en cours · { $n ->
    [one] { $n } tour restant
   *[other] { $n } tours restants
}
classic.game.orders.status = PV { $hp } · Mouvement { $left }/{ $max }
classic.game.orders.status-embarked = PV { $hp } · Mouvement { $left }/{ $max } · Embarquée
classic.game.orders.status-fortified = PV { $hp } · Mouvement { $left }/{ $max } · Fortifiée
classic.game.orders.status-embarked-fortified = PV { $hp } · Mouvement { $left }/{ $max } · Embarquée · Fortifiée
classic.game.orders.status-intercepting = PV { $hp } · Mouvement { $left }/{ $max } · En interception
classic.game.orders.mark-intercepting = En interception
classic.game.orders.remove = Retirer { $name } ({ $n ->
    [one] { $n } tour
   *[other] { $n } tours
})
classic.game.orders.remove-production = Retirer { $name } ({ $n ->
    [one] { $n } tour
   *[other] { $n } tours
}, +{ $amount } production)
classic.game.orders.great-work-created = Grande œuvre créée
classic.game.orders.treatise-written = Traité rédigé
classic.game.orders.concert-performed = Concert donné
classic.game.orders.no-enemy = Aucun ennemi visible à cet endroit
classic.game.orders.declared-war = Vous avez déclaré la guerre à { $name }
classic.game.orders.attack = Votre attaque : { $before } → { $after }
classic.game.orders.hp = { $n } PV
classic.game.orders.destroyed = détruite
classic.game.orders.report-units = les vôtres { $mine }, les leurs { $theirs }
classic.game.orders.captured = capturée
classic.game.orders.gone = disparue
classic.game.orders.report-city = les vôtres { $mine }, ville { $city }
