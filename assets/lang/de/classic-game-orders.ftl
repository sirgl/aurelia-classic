# Translation of classic-game-orders.ftl into de. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.game.orders.build = { $name } bauen
classic.game.orders.build-turns = { $name } bauen ({ $n ->
    [one] { $n } Zug
   *[other] { $n } Züge
})
classic.game.orders.status-working = In Arbeit · { $n ->
    [one] noch { $n } Zug
   *[other] noch { $n } Züge
}
classic.game.orders.status = TP { $hp } · Bewegung { $left }/{ $max }
classic.game.orders.status-embarked = TP { $hp } · Bewegung { $left }/{ $max } · Eingeschifft
classic.game.orders.status-fortified = TP { $hp } · Bewegung { $left }/{ $max } · Befestigt
classic.game.orders.status-embarked-fortified = TP { $hp } · Bewegung { $left }/{ $max } · Eingeschifft · Befestigt
classic.game.orders.status-intercepting = LP { $hp } · Bewegung { $left }/{ $max } · Abfangbereit
classic.game.orders.mark-intercepting = Abfangbereit
classic.game.orders.remove = { $name } entfernen ({ $n ->
    [one] { $n } Zug
   *[other] { $n } Züge
})
classic.game.orders.remove-production = { $name } entfernen ({ $n ->
    [one] { $n } Zug
   *[other] { $n } Züge
}, +{ $amount } Produktion)
classic.game.orders.great-work-created = Großes Werk geschaffen
classic.game.orders.treatise-written = Abhandlung verfasst
classic.game.orders.concert-performed = Konzert gegeben
classic.game.orders.no-enemy = Dort ist kein Feind sichtbar
classic.game.orders.declared-war = Sie haben { $name } den Krieg erklärt
classic.game.orders.attack = Ihr Angriff: { $before } → { $after }
classic.game.orders.hp = { $n } TP
classic.game.orders.destroyed = zerstört
classic.game.orders.report-units = eigene { $mine }, gegnerische { $theirs }
classic.game.orders.captured = erobert
classic.game.orders.gone = verschwunden
classic.game.orders.report-city = eigene { $mine }, Stadt { $city }
classic.game.orders.disband-gold = Auflösen für { NUMBER($gold) } Gold
classic.game.orders.disband-no-gold = Außerhalb eigenen Landes kein Gold
classic.game.orders.confirm-disband-title = { $name } auflösen?
classic.game.orders.confirm-disband-gold = Die Einheit verschwindet für immer; die Schatzkammer erhält { NUMBER($gold) } Gold.
classic.game.orders.confirm-disband-line = Die Einheit verschwindet für immer. Außerhalb eigenen Landes bringt sie kein Gold.
classic.game.orders.confirm-disband = Auflösen
classic.game.orders.disbanded = { $name } aufgelöst
classic.game.orders.disbanded-gold = { $name } für { NUMBER($gold) } Gold aufgelöst
