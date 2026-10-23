# Civ V game layer: unit orders, their details and the combat notices (game/orders.rs).

classic.game.orders.build = Build { $name }
classic.game.orders.build-turns = Build { $name } ({ $n ->
    [one] { $n } turn
   *[other] { $n } turns
})
classic.game.orders.status-working = Working · { $n ->
    [one] { $n } turn left
   *[other] { $n } turns left
}
classic.game.orders.status = HP { $hp } · Moves { $left }/{ $max }
classic.game.orders.status-embarked = HP { $hp } · Moves { $left }/{ $max } · Embarked
classic.game.orders.status-fortified = HP { $hp } · Moves { $left }/{ $max } · Fortified
classic.game.orders.status-embarked-fortified = HP { $hp } · Moves { $left }/{ $max } · Embarked · Fortified
classic.game.orders.status-intercepting = HP { $hp } · Moves { $left }/{ $max } · Intercepting
classic.game.orders.mark-intercepting = Intercepting
classic.game.orders.remove = Remove { $name } ({ $n ->
    [one] { $n } turn
   *[other] { $n } turns
})
classic.game.orders.remove-production = Remove { $name } ({ $n ->
    [one] { $n } turn
   *[other] { $n } turns
}, +{ $amount } production)
classic.game.orders.great-work-created = Great work created
classic.game.orders.treatise-written = Treatise written
classic.game.orders.concert-performed = Concert performed
classic.game.orders.no-enemy = No visible enemy there
classic.game.orders.declared-war = You declared war on { $name }
classic.game.orders.attack = Your attack: { $before } → { $after }
classic.game.orders.hp = { $n } HP
classic.game.orders.destroyed = destroyed
classic.game.orders.report-units = yours { $mine }, theirs { $theirs }
classic.game.orders.captured = captured
classic.game.orders.gone = gone
classic.game.orders.report-city = yours { $mine }, city { $city }
