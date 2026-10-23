# Translation of classic-game-trade.ftl into es. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.game.trade.offer = { $city } · { $kind } · { $n ->
    [one] { $n } casilla
   *[other] { $n } casillas
}
classic.game.trade.summary-active = { $n ->
    [one] { $n } ruta activa
   *[other] { $n } rutas activas
}
classic.game.trade.summary-idle = { $n ->
    [one] { $n } comerciante inactivo
   *[other] { $n } comerciantes inactivos
}
classic.game.trade.summary-queued = { $n } en producción
classic.game.trade.summary-capacity = Capacidad ocupada: { $used }/{ $possible }
classic.game.trade.established = Ruta comercial establecida con { $city }
classic.game.trade.ended = La ruta comercial de { $from } a { $to } ha terminado
classic.game.trade.kind-international = Oro y ciencia
classic.game.trade.kind-food = Comida
classic.game.trade.kind-production = Producción
classic.game.trade.label-caravan = Caravana
classic.game.trade.label-cargo-ship = Barco de carga
classic.game.trade.yield-gold = +{ NUMBER($amount, minimumFractionDigits: 2) } de oro
classic.game.trade.yield-science = +{ NUMBER($amount, minimumFractionDigits: 2) } de ciencia
classic.game.trade.yield-food = +{ NUMBER($amount, minimumFractionDigits: 2) } de comida
classic.game.trade.yield-production = +{ NUMBER($amount, minimumFractionDigits: 2) } de producción
classic.game.trade.nothing = nada
