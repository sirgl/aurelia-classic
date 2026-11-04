# Translation of classic-ui-hud-game.ftl into es. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.ui.hud-game.top-gold = { $gold } ({ $delta }) de oro
classic.ui.hud-game.top-food = { $food } de comida
classic.ui.hud-game.top-production = { $production } de producción
classic.ui.hud-game.top-science = { $science } ({ $delta }) de ciencia
classic.ui.hud-game.top-happiness = { $happiness } de felicidad
classic.ui.hud-game.top-culture = { $culture } ({ $delta }) de cultura
classic.ui.hud-game.top-faith = { $faith } ({ $delta }) de fe

## barra superior: cultura y fe hasta el siguiente paso ("24/45"), indicación del clic y frases de la ayuda emergente
classic.ui.hud-game.top-culture-goal = { $have }/{ $cost } de cultura
classic.ui.hud-game.top-faith-goal = { $have }/{ $cost } de fe
classic.ui.hud-game.click-to-open = Clic para abrir: { $screen }
classic.ui.hud-game.goal-research-ready = Ya puedes elegir una tecnología: no se investiga nada
classic.ui.hud-game.goal-policy-ready = Ya puedes adoptar una política
classic.ui.hud-game.goal-policy-turns = Faltan { $missing } de cultura para la próxima política: { $turns ->
    [one] 1 turno
   *[other] { $turns } turnos
} a +{ $rate } por turno
classic.ui.hud-game.goal-policy-stalled = Faltan { $missing } de cultura para la próxima política: — (sin ingresos de cultura)
classic.ui.hud-game.goal-pantheon-ready = Ya puedes fundar un panteón
classic.ui.hud-game.goal-pantheon-turns = Faltan { $missing } de fe para el panteón: { $turns ->
    [one] 1 turno
   *[other] { $turns } turnos
} a +{ $rate } por turno
classic.ui.hud-game.goal-pantheon-stalled = Faltan { $missing } de fe para el panteón: — (sin ingresos de fe)
classic.ui.hud-game.goal-reformation-ready = Ya puedes añadir una creencia de Reforma
classic.ui.hud-game.goal-prophet-turns = Faltan { $missing } de fe para el próximo Gran Profeta: { $turns ->
    [one] 1 turno
   *[other] { $turns } turnos
} a +{ $rate } por turno
classic.ui.hud-game.goal-prophet-stalled = Faltan { $missing } de fe para el próximo Gran Profeta: — (sin ingresos de fe)
classic.ui.hud-game.open-research = Abrir investigación
classic.ui.hud-game.research-kicker = INVESTIGACIÓN
classic.ui.hud-game.research-choose = Elegir investigación
classic.ui.hud-game.research-idle = No se estudia nada
classic.ui.hud-game.research-detail = { $stored }/{ $cost }  ·  { $n ->
    [one] { $n } turno
   *[other] { $n } turnos
}
classic.ui.hud-game.research-detail-stalled = { $stored }/{ $cost }  ·  aún sin ciencia
classic.ui.hud-game.rival-war = { $name } · Guerra
classic.ui.hud-game.rival-peace = { $name } · Paz
classic.ui.hud-game.make-peace = Hacer la paz
classic.ui.hud-game.open-borders = Fronteras abiertas
classic.ui.hud-game.blocked-reason = { $action }: { $reason }
classic.ui.hud-game.notice-line = T{ $turn }  { $text }
classic.ui.hud-game.notice-hint = Mostrar en el mapa · F7 para el registro completo
classic.ui.hud-game.capture-title = { $city } es suya
classic.ui.hud-game.capture-line = Fundada por { $founder }. Decida qué hacer con ella.
classic.ui.hud-game.unavailable = No disponible
classic.ui.hud-game.ruins-title = Ruinas antiguas
classic.ui.hud-game.ruins-line = Su unidad ha encontrado unas ruinas. Elija una de tres recompensas.
classic.ui.hud-game.ruins-later = Más tarde
classic.ui.hud-game.ruins-later-hint = Las ruinas siguen en el mapa
classic.ui.hud-game.victory = Victoria
classic.ui.hud-game.victory-line = Su nación ha ganado la partida.
classic.ui.hud-game.defeat = Derrota
classic.ui.hud-game.defeat-line = Su nación ha perdido la partida.
classic.ui.hud-game.see-results = Ver los resultados

## city-state card over the map
classic.ui.hud-game.city-state-gold = Tesoro: { $gold } de oro
classic.ui.hud-game.city-state-close = Cerrar (Esc)
