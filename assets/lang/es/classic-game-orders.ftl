# Translation of classic-game-orders.ftl into es. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.game.orders.build = Construir { $name }
classic.game.orders.build-turns = Construir { $name } ({ $n ->
    [one] { $n } turno
   *[other] { $n } turnos
})
classic.game.orders.status-working = Trabajando · { $n ->
    [one] queda { $n } turno
   *[other] quedan { $n } turnos
}
classic.game.orders.status = PV { $hp } · Mov. { $left }/{ $max }
classic.game.orders.status-embarked = PV { $hp } · Mov. { $left }/{ $max } · Embarcada
classic.game.orders.status-fortified = PV { $hp } · Mov. { $left }/{ $max } · Fortificada
classic.game.orders.status-embarked-fortified = PV { $hp } · Mov. { $left }/{ $max } · Embarcada · Fortificada
classic.game.orders.status-intercepting = PV { $hp } · Movimiento { $left }/{ $max } · Interceptando
classic.game.orders.mark-intercepting = Interceptando
classic.game.orders.remove = Eliminar { $name } ({ $n ->
    [one] { $n } turno
   *[other] { $n } turnos
})
classic.game.orders.remove-production = Eliminar { $name } ({ $n ->
    [one] { $n } turno
   *[other] { $n } turnos
}, +{ $amount } de producción)
classic.game.orders.great-work-created = Gran obra creada
classic.game.orders.treatise-written = Ensayo escrito
classic.game.orders.concert-performed = Concierto ofrecido
classic.game.orders.no-enemy = No hay ningún enemigo visible allí
classic.game.orders.declared-war = Ha declarado la guerra a { $name }
classic.game.orders.attack = Su ataque: { $before } → { $after }
classic.game.orders.hp = { $n } PV
classic.game.orders.destroyed = destruida
classic.game.orders.report-units = propias { $mine }, rivales { $theirs }
classic.game.orders.captured = capturada
classic.game.orders.gone = desaparecida
classic.game.orders.report-city = propias { $mine }, ciudad { $city }
