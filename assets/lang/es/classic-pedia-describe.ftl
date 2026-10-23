# Translation of classic-pedia-describe.ftl into es. Keys are in the order of the English file.
# Lines marked TODO still hold the English text: translate them and delete the marker.

classic.pedia.describe.join = { $a }, { $b }
classic.pedia.describe.join-or = { $a } o { $b }
classic.pedia.describe.yield-amount = { $amount } de { $yield }
classic.pedia.describe.yield-output = rendimiento
classic.pedia.describe.yield-food = comida
classic.pedia.describe.yield-production = producción
classic.pedia.describe.yield-gold = oro
classic.pedia.describe.yield-science = ciencia
classic.pedia.describe.yield-culture = cultura
classic.pedia.describe.yield-faith = fe
classic.pedia.describe.with-trigger = { $trigger }: { $text }
classic.pedia.describe.with-selector = { $text } { $selector }
classic.pedia.describe.with-condition = { $text } { $condition }
classic.pedia.describe.with-second-condition = { $text } y { $condition }
classic.pedia.describe.with-duration = { $text } durante { $turns ->
    [one] { $turns } turno
   *[other] { $turns } turnos
}
classic.pedia.describe.trigger-on-built = al construirse
classic.pedia.describe.trigger-on-turn-start = cada turno
classic.pedia.describe.trigger-on-technology-unlocked = al aprenderla
classic.pedia.describe.trigger-on-city-founded = al fundar una ciudad
classic.pedia.describe.trigger-on-unit-killed = al destruir una unidad
classic.pedia.describe.trigger-on-unit-trained = unidades entrenadas aquí
classic.pedia.describe.trigger-on-city-conquered = al conquistar una ciudad
classic.pedia.describe.trigger-on-era-entered = al entrar en una nueva era
classic.pedia.describe.selector-source-city = en esta ciudad
classic.pedia.describe.selector-owner-cities = en todas las ciudades
classic.pedia.describe.selector-event-city = en esa ciudad
classic.pedia.describe.selector-owner-units = por cada unidad
classic.pedia.describe.selector-event-unit = por esa unidad
classic.pedia.describe.selector-source-city-tiles = en las casillas que trabaja esta ciudad
classic.pedia.describe.selector-owner-city-tiles = en las casillas trabajadas
classic.pedia.describe.selector-capital = en la capital
classic.pedia.describe.condition-has-technology = con { $name }
classic.pedia.describe.condition-lacks-technology = hasta { $name }
classic.pedia.describe.condition-min-population = con { $n ->
    [one] { $n } ciudadano
   *[other] { $n } ciudadanos
} o más
classic.pedia.describe.condition-has-building = con { $name }
classic.pedia.describe.condition-on-tile = en { $name }
classic.pedia.describe.condition-terrain-until = en { $terrain } hasta { $technology }
classic.pedia.describe.condition-against-role = contra unidades { $role }
classic.pedia.describe.condition-against-class = contra unidades como { $name }
classic.pedia.describe.condition-for-units = para unidades { $kind }
classic.pedia.describe.condition-near-city = a { $n ->
    [one] { $n } casilla
   *[other] { $n } casillas
} o menos de la ciudad
classic.pedia.describe.condition-resource-class = en recursos { $class }
classic.pedia.describe.condition-unimproved-feature = en { $name } sin mejorar
classic.pedia.describe.condition-adjacent-unimproved-feature = con { $n } o más { $name } sin mejorar junto a la ciudad
classic.pedia.describe.condition-producing = al producir { $what }
classic.pedia.describe.condition-garrisoned = con guarnición
classic.pedia.describe.condition-coastal = si es costera
classic.pedia.describe.condition-learned-technology = al aprender { $name }
classic.pedia.describe.condition-on-river = si la ciudad está junto a un río
classic.pedia.describe.condition-at-peace = mientras haya paz con todas las naciones
classic.pedia.describe.condition-connected = si la ciudad está conectada con la capital
classic.pedia.describe.condition-near-improvement = a { $n ->
    [one] { $n } casilla
   *[other] { $n } casillas
} o menos de { $name }
classic.pedia.describe.plot-river = en casillas de río
classic.pedia.describe.plot-sea = en casillas de mar
classic.pedia.describe.plot-lake = en casillas de lago
classic.pedia.describe.plot-sea-resource = en recursos marinos
classic.pedia.describe.plot-natural-wonder = en maravillas naturales
classic.pedia.describe.production-units = unidades
classic.pedia.describe.production-kind-units = unidades { $kind }
classic.pedia.describe.production-a = { $name }
classic.pedia.describe.production-military-units = unidades militares
classic.pedia.describe.production-spaceship-parts = piezas de nave espacial
classic.pedia.describe.production-buildings = edificios
classic.pedia.describe.production-world-wonders = maravillas mundiales
classic.pedia.describe.production-world-wonders-before = maravillas mundiales anteriores a la { $era }
classic.pedia.describe.production-built-in-capital = edificios que la capital ya tiene
classic.pedia.describe.role-great-person = de gran personaje
classic.pedia.describe.role-religious = religiosas
classic.pedia.describe.role-civilian = civiles
classic.pedia.describe.role-spaceship-part = de nave espacial
classic.pedia.describe.role-recon = de reconocimiento
classic.pedia.describe.role-melee = cuerpo a cuerpo
classic.pedia.describe.role-archer = de arco
classic.pedia.describe.role-mounted = montadas
classic.pedia.describe.role-siege = de asedio
classic.pedia.describe.role-gun = de armas de fuego
classic.pedia.describe.role-armor = blindadas
classic.pedia.describe.role-naval-melee = navales cuerpo a cuerpo
classic.pedia.describe.role-naval-ranged = navales a distancia
classic.pedia.describe.role-submarine = submarinas
classic.pedia.describe.role-carrier = portaaviones
classic.pedia.describe.role-fighter = de caza
classic.pedia.describe.role-bomber = de bombardeo
classic.pedia.describe.role-helicopter = de helicóptero
classic.pedia.describe.role-missile = de misil
classic.pedia.describe.domain-land = terrestres
classic.pedia.describe.domain-sea = marítimas
classic.pedia.describe.domain-air = aéreas
classic.pedia.describe.resource-class-bonus = de bonificación
classic.pedia.describe.resource-class-strategic = estratégicos
classic.pedia.describe.resource-class-luxury = de lujo
classic.pedia.describe.resource-class-artifact = de artefacto
classic.pedia.describe.modifier-add-yield = { $amount } de { $yield }
classic.pedia.describe.modifier-add-yield-percent = { $percent }% de { $yield }
classic.pedia.describe.modifier-multiply-yield = ×{ $factor } de { $yield }
classic.pedia.describe.modifier-growth = { $percent }% de crecimiento
classic.pedia.describe.modifier-cost = { $percent }% de { $target }
classic.pedia.describe.modifier-combat = { $percent }% de fuerza de combate
classic.pedia.describe.modifier-combat-in = { $text } { $context }
classic.pedia.describe.modifier-movement = { $amount } de movimiento
classic.pedia.describe.modifier-unlocks = desbloquea { $name }
classic.pedia.describe.modifier-free-unit = { $count } { $name } gratis
classic.pedia.describe.modifier-free-building = { $name } gratis
classic.pedia.describe.modifier-promotion = el ascenso { $name }
classic.pedia.describe.modifier-free-building-in-cities = { $count ->
    [one] { $name } gratis en la primera ciudad
   *[other] { $name } gratis en las primeras { $count } ciudades
}
classic.pedia.describe.modifier-yield-per-count = { $amount } de { $yield } { $per }
classic.pedia.describe.modifier-yield-percent-per-count = { $percent }% de { $yield } { $per }
classic.pedia.describe.modifier-yield-from-kill = { $yield } por cada unidad enemiga destruida, más por las más fuertes
classic.pedia.describe.modifier-forbid-unit = no puede entrenar ni comprar unidades { $name }
classic.pedia.describe.modifier-forbid-annexing = no puede anexionar ciudades
classic.pedia.describe.modifier-post-combat-promotion = puede obtener el ascenso { $name } tras su primer combate
classic.pedia.describe.modifier-passive-promotion = las unidades aptas reciben los efectos del ascenso { $name }
classic.pedia.describe.modifier-experience = +{ $amount } puntos de experiencia
classic.pedia.describe.per-count-one = por { $unit }
classic.pedia.describe.per-count-many = por cada { $every } { $unit }
classic.pedia.describe.per-count-capped = { $per } (máx. { $max })
classic.pedia.describe.count-population = { $n ->
    [one] ciudadano
   *[other] ciudadanos
}
classic.pedia.describe.count-world-wonders = { $n ->
    [one] maravilla mundial
   *[other] maravillas mundiales
}
classic.pedia.describe.count-connected-cities = { $n ->
    [one] ciudad conectada con la capital
   *[other] ciudades conectadas con la capital
}
classic.pedia.describe.per-population-step = { $n ->
    [one] +1 de { $yield } por ciudadano
   *[other] +1 de { $yield } por cada { $n } ciudadanos
}
classic.pedia.describe.per-population = { $amount } de { $yield } por ciudadano
classic.pedia.describe.cost-units = coste de producción de unidades
classic.pedia.describe.cost-buildings = coste de producción de edificios
classic.pedia.describe.cost-technologies = coste de tecnologías
classic.pedia.describe.cost-tile-purchase = coste de compra de casillas
classic.pedia.describe.cost-hurry = coste de compra
classic.pedia.describe.cost-border-growth = cultura necesaria para ampliar las fronteras
classic.pedia.describe.cost-border-growth-exponent = aumento del coste de ampliar las fronteras
classic.pedia.describe.cost-unit-upgrade = coste de mejora de unidades
classic.pedia.describe.cost-unit-purchase = coste en oro de comprar unidades
classic.pedia.describe.cost-building-purchase = coste en oro de comprar edificios
classic.pedia.describe.context-attack = al atacar
classic.pedia.describe.context-defense = al defender
classic.pedia.describe.context-vs-city = contra ciudades
classic.pedia.describe.context-attack-open = al atacar en terreno abierto
classic.pedia.describe.context-attack-rough = al atacar en terreno accidentado
classic.pedia.describe.context-defense-open = al defender en terreno abierto
classic.pedia.describe.context-defense-rough = al defender en terreno accidentado
classic.pedia.describe.context-ranged-attack-open = en ataques a distancia en terreno abierto
classic.pedia.describe.context-ranged-attack-rough = en ataques a distancia en terreno accidentado
classic.pedia.describe.context-defense-vs-ranged = al defender contra ataques a distancia
classic.pedia.describe.context-ranged-attack = en ataques a distancia
classic.pedia.describe.context-vs-wounded = contra unidades heridas
classic.pedia.describe.context-vs-fortified = contra unidades fortificadas
classic.pedia.describe.context-friendly-land = en territorio amigo
classic.pedia.describe.context-outside-friendly-land = fuera de territorio amigo
classic.pedia.describe.context-attack-friendly-land = al atacar en territorio amigo
classic.pedia.describe.context-flanking = por cada unidad que flanquea
classic.pedia.describe.context-adjacent-friendly = junto a una unidad amiga
classic.pedia.describe.context-near-capital = cerca de la capital
classic.pedia.describe.context-embarked-defense = al defender estando embarcada
classic.pedia.describe.context-interception = al interceptar
classic.pedia.describe.context-air-sweep = en barridos aéreos
classic.pedia.describe.city-combat-defense = { $amount } de fuerza de combate de la ciudad por las defensas
classic.pedia.describe.city-combat-hit-points = { $amount } de puntos de vida de la ciudad
classic.pedia.describe.city-combat-defense-percent = { $amount }% de fuerza defensiva por los edificios
classic.pedia.describe.city-combat-garrison-ranged = { $amount }% de fuerza de ataque a distancia de la ciudad con guarnición
classic.pedia.describe.city-combat-nuke-damage = { $amount }% de daño recibido por ataques nucleares
classic.pedia.describe.city-combat-air-capacity = { $amount } de capacidad de aeronaves
classic.pedia.describe.city-combat-ranged = { $amount }% de fuerza de ataque a distancia de la ciudad
classic.pedia.describe.espionage-enemy-steal = { $amount }% de velocidad de robo de tecnologías de los espías enemigos
classic.pedia.describe.espionage-steal = { $amount }% de velocidad de robo de tecnologías de sus espías
classic.pedia.describe.espionage-catch = { $amount }% de probabilidad de capturar espías enemigos
classic.pedia.describe.espionage-rig-election = { $amount }% de votos al manipular elecciones de ciudades-estado
classic.pedia.describe.espionage-extra-spies = { $amount } espías con el primer espía
classic.pedia.describe.espionage-starting-rank = los espías nuevos empiezan { $amount } rangos más arriba
classic.pedia.describe.espionage-spy-pressure = los espías ejercen { $amount }x de presión religiosa en la ciudad donde están
classic.pedia.describe.league-delegates = { $amount } delegados en el Congreso Mundial
classic.pedia.describe.league-delegates-per-diplomat = { $amount } delegados por diplomático en una capital extranjera
classic.pedia.describe.stat-resource-copies = aporta { $amount } de { $resource }
classic.pedia.describe.stat-resource-quantity = { $amount }% de { $resource } de sus casillas
classic.pedia.describe.stat-strategic-quantity = { $amount }% de recursos estratégicos de sus casillas
classic.pedia.describe.stat-extra-luxury-copies = { $amount } extra de cada recurso de lujo en torno a la ciudad
classic.pedia.describe.stat-great-people-rate = { $amount }% de grandes personajes
classic.pedia.describe.stat-great-person-rate = { $amount }% de puntos de { $kind }
classic.pedia.describe.stat-food-kept = { $amount }% de comida conservada al crecer
classic.pedia.describe.stat-specialist-yield = { $amount } de { $yield } por especialista
classic.pedia.describe.stat-happiness-per-city = { $amount } de felicidad por ciudad
classic.pedia.describe.stat-happiness-per-luxury = { $amount } de felicidad por recurso de lujo
classic.pedia.describe.stat-natural-wonder-happiness = { $amount }% de felicidad de las maravillas naturales
classic.pedia.describe.stat-city-count-unhappiness = { $amount }% de infelicidad por el número de ciudades
classic.pedia.describe.stat-population-unhappiness = { $amount }% de infelicidad por población
classic.pedia.describe.stat-golden-age-length = { $amount }% de duración de la edad de oro
classic.pedia.describe.stat-connection-gold = { $amount }% de oro por las conexiones de ciudades
classic.pedia.describe.stat-research-agreement = { $amount }% de ciencia de los acuerdos de investigación
classic.pedia.describe.stat-great-scientist-science = { $amount }% de ciencia de los Grandes Científicos
classic.pedia.describe.stat-great-people-per-friend = { $amount }% de grandes personajes por declaración de amistad
classic.pedia.describe.stat-free-maintenance-units = { $amount } unidades sin mantenimiento de oro
classic.pedia.describe.stat-barbarian-conversion = { $amount }% de probabilidad de que una unidad terrestre bárbara derrotada en su campamento se una a usted y le aporte 25 de oro
classic.pedia.describe.stat-plunder-gold = { $amount }% de oro de campamentos bárbaros y ciudades capturadas
classic.pedia.describe.stat-captured-gold = { $amount }% de oro para quien capture esta ciudad
classic.pedia.describe.stat-worker-speed = { $amount }% de velocidad de los trabajadores
classic.pedia.describe.stat-route-maintenance = { $amount }% de mantenimiento de caminos y ferrocarriles
classic.pedia.describe.stat-any-specialist-yield = { $amount } de { $yield } en una ciudad con un especialista
classic.pedia.describe.rule-garrison-free-maintenance = las unidades en guarnición en ciudades no cuestan mantenimiento
classic.pedia.describe.rule-free-hills-route-maintenance = sin mantenimiento para caminos y ferrocarriles en colinas
classic.pedia.describe.rule-spaceship-part-purchase = las piezas de nave espacial se pueden comprar con oro
classic.pedia.describe.rule-spaceship-part-hurry = los Grandes Ingenieros pueden acelerar las piezas de nave espacial
classic.pedia.describe.rule-see-barbarian-camps = los nuevos campamentos bárbaros se muestran en el territorio explorado
classic.pedia.describe.society-specialist-food = { $amount }% de comida consumida por los especialistas
classic.pedia.describe.society-specialist-unhappiness = { $amount }% de infelicidad por los especialistas
classic.pedia.describe.society-no-occupied-unhappiness = sin infelicidad adicional mientras esté ocupada
classic.pedia.describe.society-wonder-culture = { $amount }% de cultura de las maravillas mundiales
classic.pedia.describe.society-happiness-to-culture = la felicidad sobrante se convierte en cultura
classic.pedia.describe.society-science-while-happy = { $amount }% de ciencia mientras el imperio está contento
classic.pedia.describe.society-luxury-retention = conserva el { $amount }% de la felicidad de un recurso de lujo cuando se comercia su última copia
classic.pedia.describe.society-raze-speed = { $amount }% de velocidad de arrasamiento
classic.pedia.describe.society-founding-territory = las ciudades nuevas reclaman { $amount } casillas adicionales
classic.pedia.describe.society-capital-science-boost = los edificios de ciencia construidos en la capital dan un impulso de investigación
classic.pedia.describe.society-unique-luxury-cities = las primeras { $amount } ciudades en otras masas de tierra reciben un recurso de lujo único
classic.pedia.describe.society-unique-luxury-quantity = { $amount } copias de cada recurso de lujo único
classic.pedia.describe.society-unique-luxury-new-landmass = recursos de lujo únicos solo en masas de tierra alejadas de la capital
classic.pedia.describe.trade-routes = { $amount } rutas comerciales
classic.pedia.describe.trade-range-tiles = { $amount } casillas de alcance de las rutas comerciales { $domain }
classic.pedia.describe.trade-range-percent = { $amount }% de alcance de las rutas comerciales { $domain }
classic.pedia.describe.trade-route-gold = { $amount } de oro de las rutas comerciales { $domain }
classic.pedia.describe.trade-target-gold = { $amount } de oro por las rutas comerciales hacia esta ciudad
classic.pedia.describe.trade-recipient-gold = { $amount } de oro para esta ciudad por rutas comerciales extranjeras
classic.pedia.describe.trade-internal-percent = { $amount }% de comida y producción de las rutas comerciales internas
classic.pedia.describe.trade-routes-percent = { $amount }% de rutas comerciales
classic.pedia.describe.trade-resource-gold-percent = { $amount }% de oro por la diversidad de recursos en las rutas comerciales
classic.pedia.describe.trade-incoming-route-gold = { $amount } de oro para quien le envíe una ruta comercial
classic.pedia.describe.trade-city-state-route-gold = { $amount } de oro de las rutas comerciales hacia ciudades-estado
classic.pedia.describe.trade-shared-ideology-route-gold = { $amount } de oro de las rutas comerciales hacia naciones de su ideología
classic.pedia.describe.trade-religious-pressure = { $amount }% de presión religiosa a lo largo de las rutas comerciales
classic.pedia.describe.placement-land = Se coloca en tierra
classic.pedia.describe.placement-water = Emerge del mar
classic.pedia.describe.placement-reef = Se coloca en aguas abiertas cerca de una costa
classic.pedia.describe.placement-peninsula-tip = Se coloca en la punta de una península
classic.pedia.describe.placement-of-terrains = { $text } de { $terrains }
classic.pedia.describe.placement-away-from-coast = { $text }, lejos de la costa
classic.pedia.describe.placement-off-largest-landmass = { $text }, fuera de la mayor masa de tierra
classic.pedia.describe.placement-sentence = { $text }.
classic.pedia.describe.adjacent-hills = colinas
classic.pedia.describe.adjacent-mountains = montañas
classic.pedia.describe.adjacent-hills-or-mountains = colinas o montañas
classic.pedia.describe.adjacent-land = casillas de tierra
classic.pedia.describe.adjacent-water = casillas de agua
classic.pedia.describe.adjacent-at-least = Al menos { $n } { $what } vecinas.
classic.pedia.describe.adjacent-none = Sin { $what } vecinas.
classic.pedia.describe.adjacent-at-most = Como máximo { $n } { $what } vecinas.
classic.pedia.describe.shape-mountain = La casilla se convierte en montaña.
classic.pedia.describe.shape-flat = La casilla se convierte en terreno llano.
classic.pedia.describe.surrounded-by-sea = Está rodeada por el mar.
classic.pedia.describe.grant-free-technologies = { $n ->
    [one] una tecnología gratis a su elección
   *[other] { $n } tecnologías gratis a su elección
}
classic.pedia.describe.grant-conquest-technology = una tecnología gratis conocida por el antiguo dueño de la ciudad (una vez por ciudad)
classic.pedia.describe.grant-military-unit-copies = una copia de cada una de sus unidades de combate terrestres
classic.pedia.describe.grant-population = { $n ->
    [one] { $amount } ciudadano
   *[other] { $amount } ciudadanos
}
classic.pedia.describe.unit-stat-sight = { $amount } casillas de visión
classic.pedia.describe.unit-stat-embarked-sight = { $amount } casillas de visión estando embarcada
classic.pedia.describe.unit-stat-range = { $amount } casillas de alcance de ataque a distancia
classic.pedia.describe.unit-stat-attacks-per-turn = { $amount } ataques por turno
classic.pedia.describe.unit-stat-cargo = { $amount } de capacidad de transporte de aeronaves o misiles
classic.pedia.describe.unit-stat-intercept-chance = { $amount } puntos porcentuales a la probabilidad de interceptación
classic.pedia.describe.unit-stat-interceptions-per-turn = { $amount } interceptaciones por turno
classic.pedia.describe.unit-stat-evasion = { $amount } puntos porcentuales a la evasión de la interceptación
classic.pedia.describe.unit-stat-interception-damage = { $amount }% de daño recibido por interceptaciones
classic.pedia.describe.unit-stat-paradrop-range = { $amount } casillas de alcance de lanzamiento en paracaídas
classic.pedia.describe.unit-stat-naval-moves = { $amount } de movimiento en el mar
classic.pedia.describe.unit-stat-experience = { $amount }% de experiencia ganada
classic.pedia.describe.unit-stat-withdraw-chance = { $amount } puntos porcentuales a la probabilidad de retirada antes del combate cuerpo a cuerpo
classic.pedia.describe.unit-stat-city-attack-plunder = obtiene oro al atacar ciudades, más cuanto mayor sea el daño
classic.pedia.describe.unit-stat-heal-enemy-land = { $amount } PV curados por turno en territorio enemigo
classic.pedia.describe.unit-stat-heal-neutral-land = { $amount } PV curados por turno en territorio neutral
classic.pedia.describe.unit-stat-heal-friendly-land = { $amount } PV curados por turno en territorio amigo
classic.pedia.describe.unit-stat-heal-adjacent = las unidades amigas adyacentes curan { $amount } PV por turno
classic.pedia.describe.unit-stat-heal-on-kill = { $amount } PV curados tras destruir una unidad enemiga
classic.pedia.describe.unit-stat-enemy-aura = { $amount }% de fuerza de combate para las unidades enemigas cercanas
classic.pedia.describe.unit-stat-aura-range = { $amount } casillas al alcance del efecto de enemigos cercanos
classic.pedia.describe.unit-stat-adjacent-damage = { $amount } de daño a las unidades enemigas adyacentes tras atacar
classic.pedia.describe.unit-stat-capital-defense-falloff = cambia la rapidez con que el bonus de defensa de la capital se debilita con la distancia
classic.pedia.describe.unit-stat-embarked-moves = { $amount } de movimiento estando embarcada
classic.pedia.describe.unit-stat-wounded-damage = { $amount } puntos porcentuales al modificador de daño por unidad herida
classic.pedia.describe.ability-found-religion = Puede fundar o potenciar una religión
classic.pedia.describe.ability-spread-religion = Puede extender su religión a una ciudad
classic.pedia.describe.ability-religion-spreads = Puede extender la religión { $n ->
    [one] { $n } vez
   *[other] { $n } veces
}
classic.pedia.describe.ability-religious-strength = Fuerza religiosa: { $n }
classic.pedia.describe.ability-remove-heresy = Puede expulsar otras religiones de una ciudad
classic.pedia.describe.ability-prohibits-spread = Su presencia bloquea la expansión de otras religiones
classic.pedia.describe.ability-requires-enhanced-religion = Solo se puede comprar cuando su religión esté potenciada
classic.pedia.describe.ability-one-shot-tourism = Un concierto aporta un turismo que crece con su turismo actual
classic.pedia.describe.ability-one-shot-tourism-others = Las demás naciones reciben el { $n }% del turismo que aporta un concierto
classic.pedia.describe.ability-culture-turns = Otorga cultura en proporción a su producción de cultura reciente
classic.pedia.describe.ability-excavates = Puede excavar yacimientos de antigüedades
classic.pedia.describe.ability-food = Mientras se entrena, el excedente de comida de la ciudad se suma a la producción
classic.pedia.describe.ability-golden-age-turns = Puede iniciar una edad de oro con una duración base de { $n ->
    [one] { $n } turno
   *[other] { $n } turnos
}
classic.pedia.describe.ability-beakers-turns = Otorga un impulso de ciencia en proporción a su producción reciente
classic.pedia.describe.ability-base-hurry = Puede añadir producción a lo que construye una ciudad
classic.pedia.describe.ability-hurry-multiplier = Una ciudad más grande recibe así más producción
classic.pedia.describe.ability-base-gold = Las misiones comerciales aportan oro
classic.pedia.describe.ability-gold-per-era = En eras posteriores las misiones comerciales pagan más
classic.pedia.describe.ability-no-maintenance = No cuesta mantenimiento de oro
classic.pedia.describe.ability-trade = Puede establecer rutas comerciales
classic.pedia.describe.ability-faith-purchase-needs-belief = Solo se puede comprar con fe donde una creencia desbloquea su era
classic.pedia.describe.ability-mechanized = Una máquina, no personas
classic.pedia.describe.ability-priority-low = Los enemigos van a por ella en último lugar
classic.pedia.describe.ability-priority-early = Los enemigos van a por ella primero al inicio de la partida
classic.pedia.describe.ability-priority-high = Los enemigos van a por ella primero
classic.pedia.describe.ability-conscription = Valor de reclutamiento { $n }
classic.pedia.describe.ability-great-person = Un gran personaje
classic.pedia.describe.ability-stealth = Aeronave furtiva: ningún portaaviones puede llevarla
classic.pedia.describe.ability-air-slots = Ocupa { $n ->
    [one] { $n } ranura aérea
   *[other] { $n } ranuras aéreas
} en una ciudad
classic.pedia.describe.ability-work-rate = Ritmo de trabajo { $n }
classic.pedia.describe.ability-move-after-purchase = Puede moverse el turno en que se compra
classic.pedia.describe.ability-no-bad-goodies = Las ruinas antiguas nunca la dañan
classic.pedia.describe.ability-purchase-only = Solo se puede comprar, nunca entrenar
classic.pedia.describe.ability-buy-city-state = Convierte la ciudad-estado en la que se encuentra en un títere de su dueño
classic.pedia.describe.ability-found-abroad = Funda ciudades solo fuera de la masa de tierra de la capital
classic.pedia.describe.ability-exotic-goods = Vende { $n } de carga de bienes exóticos junto a una tierra extranjera
classic.pedia.describe.combat-ability-move-after-attacking = puede moverse después de atacar
classic.pedia.describe.combat-ability-always-heal = puede curarse tras moverse o atacar
classic.pedia.describe.combat-ability-embarkation = puede embarcar para cruzar el agua
classic.pedia.describe.combat-ability-embarked-all-water = puede entrar en aguas profundas estando embarcada
classic.pedia.describe.combat-ability-move-impassable = puede entrar en terreno normalmente intransitable
classic.pedia.describe.combat-ability-ranged-support-fire = realiza un ataque de apoyo a distancia antes del combate cuerpo a cuerpo
classic.pedia.describe.combat-ability-hills-double-move = paga la mitad del coste de movimiento habitual en colinas
classic.pedia.describe.combat-ability-double-move = paga la mitad del coste de movimiento habitual en el terreno indicado
classic.pedia.describe.combat-ability-impassable = no puede entrar en el terreno indicado
classic.pedia.describe.combat-ability-insta-heal = cura al instante a la unidad en lugar de añadir un ascenso duradero
classic.pedia.describe.combat-ability-amphibious = sin penalización de ataque al desembarcar del agua o cruzar un río
classic.pedia.describe.combat-ability-river-crossing = sin penalización de ataque al cruzar un río
classic.pedia.describe.combat-ability-indirect-fire = los ataques a distancia no requieren línea de visión
classic.pedia.describe.combat-ability-heal-outside-friendly = puede curarse en el mar fuera de territorio amigo
classic.pedia.describe.combat-ability-air-sweep = puede realizar barridos aéreos
classic.pedia.describe.combat-ability-rival-territory = puede entrar en territorio rival sin fronteras abiertas
classic.pedia.describe.combat-ability-rough-terrain-ends-turn = entrar en terreno accidentado termina el movimiento de esta unidad
classic.pedia.describe.combat-ability-ignore-terrain-cost = ignora los costes de movimiento adicionales del terreno
classic.pedia.describe.combat-ability-flat-movement-cost = todas las casillas tienen el mismo coste de movimiento, incluidos los caminos
classic.pedia.describe.combat-ability-hovering = puede moverse sobre tierra y agua
classic.pedia.describe.combat-ability-no-capture = no puede capturar ciudades ni unidades
classic.pedia.describe.combat-ability-only-defensive = no puede realizar ataques cuerpo a cuerpo
classic.pedia.describe.combat-ability-no-defensive-bonus = no recibe bonificación de defensa por terreno ni por fortificación
classic.pedia.describe.combat-ability-must-set-up = debe prepararse antes de un ataque a distancia
classic.pedia.describe.combat-ability-free-pillage-moves = saquear no cuesta movimiento
classic.pedia.describe.combat-ability-city-attack-only = solo puede atacar ciudades
classic.pedia.describe.combat-ability-capture-defeated-enemy = puede capturar unidades enemigas derrotadas
classic.pedia.describe.combat-ability-heavy-charge = ataques más fuertes contra unidades que no pueden retirarse
classic.pedia.describe.combat-ability-sapper = ayuda a las unidades amigas adyacentes a atacar una ciudad junto a esta unidad
classic.pedia.describe.combat-ability-ignore-zone-of-control = ignora las zonas de control enemigas
classic.pedia.describe.combat-ability-embarked-to-land-flat-cost = desembarcar cuesta un punto de movimiento
classic.pedia.describe.combat-ability-hills-ignore-terrain-cost = ignora los costes de terreno al subir a colinas
classic.pedia.describe.combat-ability-cross-mountains = puede cruzar montañas
classic.pedia.describe.combat-ability-road-in-friendly-land = se mueve por el terreno indicado en su propio territorio como si fuera por un camino
classic.pedia.describe.combat-ability-invisible = solo se ve junto a unidades y ciudades enemigas o por unidades que ven submarinos
classic.pedia.describe.combat-ability-see-invisible = ve submarinos dentro de su visión
classic.pedia.describe.inert-amount = { $subject }: { $amount }
classic.pedia.describe.inert-refs = { $text } ({ $names })
classic.pedia.describe.inert-trigger = { $trigger }: { $text }
classic.pedia.describe.building-ideology-choice = Con { $n } de estos edificios puede elegir una ideología
classic.pedia.describe.building-free-great-work = Incluye una gran obra de arte gratuita
classic.pedia.describe.building-nullify-influence = Protege a su nación del bonus de turismo que otras naciones obtienen de Internet
classic.pedia.describe.building-missionary-extra-spreads = Los misioneros comprados aquí pueden extender la religión { $n ->
    [one] { $n } vez adicional
   *[other] { $n } veces adicionales
}
classic.pedia.describe.building-religious-pressure = +{ $n }% de presión religiosa de esta ciudad
classic.pedia.describe.building-food-trade-routes = Esta ciudad puede enviar rutas comerciales de comida a sus otras ciudades
classic.pedia.describe.building-production-trade-routes = Esta ciudad puede enviar rutas comerciales de producción a sus otras ciudades
classic.pedia.describe.building-counter-espionage = Proporciona contraespionaje en esta ciudad
classic.pedia.describe.building-affect-spies-now = Los espías que ya reúnen información aquí notan sus efectos de espionaje de inmediato
classic.pedia.describe.building-border-obstacle = Las unidades terrestres enemigas gastan 1 punto de movimiento adicional por casilla en su territorio
classic.pedia.describe.building-airlift = Las unidades terrestres pueden ser transportadas por aire desde o hacia esta ciudad
classic.pedia.describe.building-allows-range-strike = Permite a esta ciudad realizar ataques a distancia
classic.pedia.describe.unknown = Desconocido
classic.pedia.describe-classic.trigger-on-adopt = al adoptarse
classic.pedia.describe-classic.trigger-wonder-found-first = al ser el primero en descubrir una maravilla natural
classic.pedia.describe-classic.trigger-wonder-found = al descubrir una maravilla natural que otra nación descubrió antes
classic.pedia.describe-classic.trigger-long-count = al final de cada ciclo de la Cuenta Larga
classic.pedia.describe-classic.trigger-great-person-expended = al sacrificar a un gran personaje
classic.pedia.describe-classic.trigger-city-converted = cuando una ciudad se convierte por primera vez a la religión
classic.pedia.describe-classic.trigger-religion-spread = cuando un misionero o profeta extiende la religión
classic.pedia.describe-classic.trigger-great-person-born = al nacer un gran personaje
classic.pedia.describe-classic.selector-following-cities = en las ciudades que siguen la religión
classic.pedia.describe-classic.selector-following-city-tiles = en las casillas de las ciudades que siguen la religión
classic.pedia.describe-classic.selector-holy-city = en la ciudad santa
classic.pedia.describe-classic.selector-founder = para el fundador
classic.pedia.describe-classic.selector-own-religion = para la religión
classic.pedia.describe-classic.condition-barbarian = contra bárbaros
classic.pedia.describe-classic.condition-city-state = contra ciudades-estado
classic.pedia.describe-classic.condition-more-cities = contra naciones con más ciudades
classic.pedia.describe-classic.condition-min-followers = con { $n ->
    [one] { $n } seguidor
   *[other] { $n } seguidores
} o más
classic.pedia.describe-classic.condition-near-own-following-city = cerca de ciudades propias que siguen la religión
classic.pedia.describe-classic.condition-near-enemy-following-city = cerca de ciudades enemigas que siguen la religión
classic.pedia.describe-classic.condition-golden-age = durante una edad de oro
classic.pedia.describe-classic.condition-great-general-earned = una vez obtenido un gran general
classic.pedia.describe-classic.condition-great-person-is = si es { $kind }
classic.pedia.describe-classic.modifier-happiness = { $amount } de felicidad
classic.pedia.describe-classic.modifier-great-person-points = { $amount } puntos de { $kind }
classic.pedia.describe-classic.modifier-free-policies = { $count ->
    [one] { $count } política social gratis
   *[other] { $count } políticas sociales gratis
}
classic.pedia.describe-classic.modifier-yield-per-following-city = { $foreign ->
    [true] { $amount } de { $yield } por cada ciudad extranjera que sigue la religión
   *[false] { $amount } de { $yield } por cada ciudad que sigue la religión
}
classic.pedia.describe-classic.modifier-yield-per-followers = { $foreign ->
    [true] +1 de { $yield } por cada { $per } seguidores extranjeros
   *[false] +1 de { $yield } por cada { $per } seguidores
}
classic.pedia.describe-classic.modifier-happiness-per-following-city = { $amount } de felicidad por cada ciudad que sigue la religión
classic.pedia.describe-classic.modifier-religion = { $percent }% { $stat }
classic.pedia.describe-classic.modifier-faith-building = { $name } se puede comprar con fe
classic.pedia.describe-classic.modifier-faith-units = las unidades terrestres de la { $era } se pueden comprar con fe
classic.pedia.describe-classic.modifier-reformation-belief = se puede añadir una creencia de Reforma
classic.pedia.describe-classic.modifier-tourism-toward = { $percent }% de turismo hacia las naciones { $when }
classic.pedia.describe-classic.modifier-happiness-per-count = { $amount } de felicidad { $per }
classic.pedia.describe-classic.modifier-grant-spies = { $count ->
    [one] un espía gratis
   *[other] { $count } espías gratis
}
classic.pedia.describe-classic.modifier-promote-spies = { $ranks ->
    [one] todos los espías suben un rango
   *[other] todos los espías suben { $ranks } rangos
}
classic.pedia.describe-classic.modifier-extra-founding-belief = una creencia más al fundar una religión
classic.pedia.describe-classic.modifier-second-religion-pantheon = las ciudades también reciben la creencia de panteón de su segunda religión
classic.pedia.describe-classic.modifier-instant-tourism = +{ $amount } de turismo con todas las naciones conocidas
classic.pedia.describe-classic.modifier-experience-per-great-work = las unidades entrenadas aquí empiezan con { $amount } de experiencia por cada gran obra de la ciudad
classic.pedia.describe-classic.grant-free-great-people = { $n ->
    [one] un gran personaje gratis a su elección
   *[other] { $n } grandes personajes gratis a su elección
}
classic.pedia.describe-classic.grant-long-count-great-person = un gran personaje gratis de un tipo no elegido antes
classic.pedia.describe-classic.grant-golden-age = una edad de oro de { $turns ->
    [one] { $turns } turno
   *[other] { $turns } turnos
}
classic.pedia.describe-classic.unit-stat-general-points = { $amount }% de puntos de Gran General por combate
classic.pedia.describe-classic.unit-stat-stacked-general = { $amount }% de fuerza de combate para una unidad que comparte casilla con este Gran General
classic.pedia.describe-classic.unit-stat-maintenance = { $amount }% de mantenimiento de oro
classic.pedia.describe-classic.unit-stat-experience-needed = { $amount }% de experiencia necesaria para un ascenso
classic.pedia.describe-classic.unit-stat-general-aura = { $amount }% de bonificación de combate de los Grandes Generales y Almirantes
classic.pedia.describe-classic.unit-stat-mission-gold = { $amount }% de oro de una misión comercial
classic.pedia.describe-classic.unit-stat-mission-influence = { $amount }% de influencia de una misión comercial
classic.pedia.describe-classic.unit-stat-adjacent-damage-chance = { $amount }% de probabilidad de dañar a los enemigos adyacentes tras atacar
classic.pedia.describe-classic.unit-stat-rival-land-loss = pierde fuerza religiosa por cada turno que termina en territorio rival
classic.pedia.describe-classic.unit-stat-golden-age-on-kill = cada baja añade puntos de edad de oro, más cuanto más fuerte sea la víctima
classic.pedia.describe-classic.ability-great-general = aporta la bonificación de combate de un Gran General a las unidades terrestres amigas cercanas
classic.pedia.describe-classic.ability-post-combat-promotions = se sustituye por un ascenso aleatorio tras su primer combate
classic.pedia.describe-classic.ability-lends-movement = un Gran General que comparte casilla con ella al inicio del turno se mueve a su ritmo
classic.pedia.describe-classic.ability-ruins-reward = elige la mejor de las recompensas al entrar en unas ruinas antiguas
classic.pedia.describe-classic.ability-exotic-cargo = lleva bienes exóticos para vender una vez junto a una tierra extranjera
classic.pedia.describe-classic.ability-great-admiral = aporta la bonificación de combate de un Gran Almirante a las unidades navales amigas cercanas
classic.pedia.describe-classic.ability-heal-not-barbarians = la curación tras una baja no se aplica a los bárbaros
classic.pedia.describe-classic.cost-policies = coste de las políticas sociales
classic.pedia.describe-classic.cost-policy-city-penalty = coste de las políticas por ciudad
classic.pedia.describe-classic.cost-faith-purchase = coste de compra con fe
classic.pedia.describe-classic.count-policies = { $n ->
    [one] política social
   *[other] políticas sociales
}
classic.pedia.describe-classic.count-majority-followers = { $n ->
    [one] seguidor
   *[other] seguidores
}
classic.pedia.describe-classic.count-religions-present = { $n ->
    [one] religión presente
   *[other] religiones presentes
}
classic.pedia.describe-classic.count-peaceful-foreign-followers = { $n ->
    [one] seguidor en una ciudad extranjera que no está en guerra
   *[other] seguidores en ciudades extranjeras que no están en guerra
}
classic.pedia.describe-classic.count-other-religion-followers = { $n ->
    [one] seguidor de otras religiones
   *[other] seguidores de otras religiones
}
classic.pedia.describe-classic.count-trade-partners = { $n ->
    [one] socio comercial
   *[other] socios comerciales
}
classic.pedia.describe-classic.count-city-state-routes = { $n ->
    [one] ruta comercial hacia una ciudad-estado
   *[other] rutas comerciales hacia ciudades-estado
}
classic.pedia.describe-classic.league-happiness = +{ $n } de felicidad
classic.pedia.describe-classic.league-free-policies = { $n ->
    [one] { $n } política social gratis
   *[other] { $n } políticas sociales gratis
}
classic.pedia.describe-classic.league-golden-age-points = +{ $n } puntos para una edad de oro
classic.pedia.describe-classic.league-culture-boost = +100% de cultura durante { $n ->
    [one] { $n } turno
   *[other] { $n } turnos
}
classic.pedia.describe-classic.league-tourism-boost = +100% de turismo durante { $n ->
    [one] { $n } turno
   *[other] { $n } turnos
}
classic.pedia.describe-classic.league-city-state-influence = +{ $n } de influencia con cada ciudad-estado conocida
classic.pedia.describe-classic.league-research-turns = ciencia { $n ->
    [one] del último turno
   *[other] de los últimos { $n } turnos
}
classic.pedia.describe-classic.league-free-unit = { $name } gratis en la capital
classic.pedia.describe-classic.league-building = { $name } en la capital
classic.pedia.describe-classic.city-state-resting-point = { $amount } de influencia en reposo con las ciudades-estado
classic.pedia.describe-classic.city-state-decay = { $amount }% de pérdida de influencia con las ciudades-estado
classic.pedia.describe-classic.city-state-gold-gift = { $amount }% de influencia por regalos de oro a ciudades-estado
classic.pedia.describe-classic.city-state-bully-score = { $amount }% de poderío militar al exigir tributo
classic.pedia.describe-classic.city-state-unit-gift-influence = { $amount } de influencia por unidad regalada a una ciudad-estado
classic.pedia.describe-classic.city-state-friend-bonus = { $amount }% de comida, cultura y fe de las ciudades-estado amigas
classic.pedia.describe-classic.city-state-friendship-rate = { $amount }% de recuperación de influencia con las ciudades-estado; { $decay }% de pérdida de influencia
classic.pedia.describe-classic.city-state-annex = puede comprar las ciudades-estado aliadas durante cinco turnos
classic.pedia.describe-classic.city-state-resting-point-religion = { $amount } de punto de reposo de influencia con las ciudades-estado que siguen la religión
classic.pedia.describe-classic.city-state-afraid-influence = { $amount } de influencia por turno con las ciudades-estado que le temen
classic.pedia.describe-classic.city-state-trade-route-influence = { $amount } de influencia por turno con las ciudades-estado con las que tiene una ruta comercial
classic.pedia.describe-classic.city-state-unit-gift-rate = las ciudades-estado militaristas regalan unidades un { $amount }% más rápido mientras comparten un enemigo con usted
classic.pedia.describe-classic.city-state-ally-resources = las ciudades-estado aliadas dan el doble de recursos y sus recursos de lujo dan un 50% más de felicidad
classic.pedia.describe-classic.city-state-ally-great-people = las ciudades-estado aliadas dan un gran personaje de vez en cuando
classic.pedia.describe-classic.city-state-ally-science = una cuarta parte de la ciencia de las ciudades-estado aliadas
classic.pedia.describe-classic.city-state-great-person-gift = { $amount } de influencia por gran personaje regalado a una ciudad-estado
classic.pedia.describe-classic.city-state-mission-gold = { $amount }% de oro de las misiones comerciales
classic.pedia.describe-classic.great-artist = Gran Artista
classic.pedia.describe-classic.great-engineer = Gran Ingeniero
classic.pedia.describe-classic.great-merchant = Gran Mercader
classic.pedia.describe-classic.great-scientist = Gran Científico
classic.pedia.describe-classic.great-writer = Gran Escritor
classic.pedia.describe-classic.great-musician = Gran Músico
classic.pedia.describe-classic.great-general = Gran General
classic.pedia.describe-classic.great-admiral = Gran Almirante
classic.pedia.describe-classic.great-prophet = Gran Profeta
classic.pedia.describe-classic.religion-pressure-out = de presión religiosa
classic.pedia.describe-classic.religion-spread-distance = de distancia de expansión religiosa
classic.pedia.describe-classic.religion-missionary-strength = de fuerza de los misioneros
classic.pedia.describe-classic.religion-prophet-strength = de fuerza de los Grandes Profetas
classic.pedia.describe-classic.religion-prophet-cost = de coste en fe del Gran Profeta
classic.pedia.describe-classic.religion-missionary-cost = de coste en fe de los misioneros
classic.pedia.describe-classic.religion-friendly-city-state-pressure = de presión religiosa en las ciudades-estado amigas
classic.pedia.describe-classic.religion-other-erosion = de la presión de un misionero tomada de otras religiones
classic.pedia.describe-classic.religion-pressure-retention = de su presión conservada cuando inquisidores o profetas la eliminan
classic.pedia.describe-classic.religion-converts-barbarians = los misioneros convierten a las unidades bárbaras adyacentes
classic.pedia.describe-classic.religion-faith-great-people = todos los grandes personajes se pueden comprar con fe desde la era industrial
classic.pedia.describe-classic.tourism-flat = { $amount } de turismo
classic.pedia.describe-classic.tourism-percent = { $amount }% de turismo
classic.pedia.describe-classic.tourism-great-works = { $amount }% de turismo de las grandes obras
classic.pedia.describe-classic.tourism-landmarks = { $amount }% de turismo de monumentos y maravillas
classic.pedia.describe-classic.tourism-per-faith-building = { $amount } de turismo por edificio de fe
classic.pedia.describe-classic.tourism-spread = { $amount }% de expansión del turismo
classic.pedia.describe-classic.tourism-theming = { $amount }% de bonificaciones de temática
classic.pedia.describe-classic.tourism-great-work-culture = { $amount } de cultura por gran obra
classic.pedia.describe-classic.tourism-shared-religion = que comparten su religión
classic.pedia.describe-classic.tourism-trade-route = con las que tiene una ruta comercial
classic.pedia.describe-classic.tourism-open-borders = con fronteras abiertas
classic.pedia.describe-classic.tourism-same-ideology = de su ideología
classic.pedia.describe-classic.tourism-less-happy = menos felices que usted
classic.pedia.describe-classic.tourism-common-enemy = en guerra con sus enemigos
