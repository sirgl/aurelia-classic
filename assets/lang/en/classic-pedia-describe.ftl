# The Encyclopedia's effect text (ui/pedia/describe.rs and describe_classic.rs): every sentence the
# encyclopedia derives from the rules. Source of truth: en.
#
# Sentences are whole messages with named arguments. A sentence made of optional parts is
# built by wrapping: each `with-*` message gets the text so far as `$text`, so a language may
# put the new part before or after it. Signed numbers (`$amount`, `$percent`) arrive with
# their sign ("+2", "-10"). Names of rule data (`$name`, `$yield`) arrive translated.

## lists
classic.pedia.describe.join = { $a }, { $b }
classic.pedia.describe.join-or = { $a } or { $b }

## yields
classic.pedia.describe.yield-amount = { $amount } { $yield }
classic.pedia.describe.yield-output = output
classic.pedia.describe.yield-food = Food
classic.pedia.describe.yield-production = Production
classic.pedia.describe.yield-gold = Gold
classic.pedia.describe.yield-science = Science
classic.pedia.describe.yield-culture = Culture
classic.pedia.describe.yield-faith = Faith

## the parts of an effect sentence
classic.pedia.describe.with-trigger = { $trigger }: { $text }
classic.pedia.describe.with-selector = { $text } { $selector }
classic.pedia.describe.with-condition = { $text } { $condition }
classic.pedia.describe.with-second-condition = { $text } and { $condition }
classic.pedia.describe.with-duration = { $text } for { $turns ->
    [one] { $turns } turn
   *[other] { $turns } turns
}

classic.pedia.describe.trigger-on-built = when built
classic.pedia.describe.trigger-on-turn-start = every turn
classic.pedia.describe.trigger-on-technology-unlocked = on learning it
classic.pedia.describe.trigger-on-city-founded = when a city is founded
classic.pedia.describe.trigger-on-unit-killed = on a kill
classic.pedia.describe.trigger-on-unit-trained = units trained here
classic.pedia.describe.trigger-on-city-conquered = when a city is conquered
classic.pedia.describe.trigger-on-era-entered = on entering a new era

classic.pedia.describe.selector-source-city = in this city
classic.pedia.describe.selector-owner-cities = in every city
classic.pedia.describe.selector-event-city = in that city
classic.pedia.describe.selector-owner-units = for every unit
classic.pedia.describe.selector-event-unit = for that unit
classic.pedia.describe.selector-source-city-tiles = on tiles this city works
classic.pedia.describe.selector-owner-city-tiles = on worked tiles
classic.pedia.describe.selector-capital = in the capital

## conditions
classic.pedia.describe.condition-has-technology = with { $name }
classic.pedia.describe.condition-lacks-technology = until { $name }
classic.pedia.describe.condition-min-population = at { $n ->
    [one] { $n } citizen
   *[other] { $n } citizens
} or more
classic.pedia.describe.condition-has-building = with a { $name }
classic.pedia.describe.condition-on-tile = on { $name }
classic.pedia.describe.condition-terrain-until = on { $terrain } until { $technology }
classic.pedia.describe.condition-against-role = against { $role } units
classic.pedia.describe.condition-against-class = against units like the { $name }
classic.pedia.describe.condition-for-units = for { $kind } units
classic.pedia.describe.condition-near-city = within { $n ->
    [one] { $n } tile
   *[other] { $n } tiles
} of the city
classic.pedia.describe.condition-resource-class = on { $class } resources
classic.pedia.describe.condition-unimproved-feature = on unimproved { $name }
classic.pedia.describe.condition-adjacent-unimproved-feature = with { $n } or more unimproved { $name } next to the city
classic.pedia.describe.condition-producing = when producing { $what }
classic.pedia.describe.condition-garrisoned = with a garrison
classic.pedia.describe.condition-coastal = if coastal
classic.pedia.describe.condition-learned-technology = on learning { $name }
classic.pedia.describe.condition-on-river = if the city is on a river
classic.pedia.describe.condition-at-peace = while at peace with every nation
classic.pedia.describe.condition-connected = if the city is connected to the capital
classic.pedia.describe.condition-near-improvement = within { $n ->
    [one] { $n } tile
   *[other] { $n } tiles
} of { $name }

classic.pedia.describe.plot-river = on river tiles
classic.pedia.describe.plot-sea = on sea tiles
classic.pedia.describe.plot-lake = on lake tiles
classic.pedia.describe.plot-sea-resource = on sea resources
classic.pedia.describe.plot-natural-wonder = on natural wonders

## what a production bonus applies to
classic.pedia.describe.production-units = units
classic.pedia.describe.production-kind-units = { $kind } units
classic.pedia.describe.production-a = a { $name }
classic.pedia.describe.production-military-units = military units
classic.pedia.describe.production-spaceship-parts = spaceship parts
classic.pedia.describe.production-buildings = buildings
classic.pedia.describe.production-world-wonders = world wonders
classic.pedia.describe.production-world-wonders-before = world wonders before the { $era }
classic.pedia.describe.production-built-in-capital = buildings the capital already has

## unit roles, domains and resource classes as words inside a sentence
classic.pedia.describe.role-great-person = great person
classic.pedia.describe.role-religious = religious
classic.pedia.describe.role-civilian = civilian
classic.pedia.describe.role-spaceship-part = spaceship part
classic.pedia.describe.role-recon = recon
classic.pedia.describe.role-melee = melee
classic.pedia.describe.role-archer = archer
classic.pedia.describe.role-mounted = mounted
classic.pedia.describe.role-siege = siege
classic.pedia.describe.role-gun = gun
classic.pedia.describe.role-armor = armor
classic.pedia.describe.role-naval-melee = naval melee
classic.pedia.describe.role-naval-ranged = naval ranged
classic.pedia.describe.role-submarine = submarine
classic.pedia.describe.role-carrier = carrier
classic.pedia.describe.role-fighter = fighter
classic.pedia.describe.role-bomber = bomber
classic.pedia.describe.role-helicopter = helicopter
classic.pedia.describe.role-missile = missile
classic.pedia.describe.domain-land = land
classic.pedia.describe.domain-sea = sea
classic.pedia.describe.domain-air = air
classic.pedia.describe.resource-class-bonus = bonus
classic.pedia.describe.resource-class-strategic = strategic
classic.pedia.describe.resource-class-luxury = luxury
classic.pedia.describe.resource-class-artifact = artifact

## modifiers
classic.pedia.describe.modifier-add-yield = { $amount } { $yield }
classic.pedia.describe.modifier-add-yield-percent = { $percent }% { $yield }
classic.pedia.describe.modifier-multiply-yield = ×{ $factor } { $yield }
classic.pedia.describe.modifier-growth = { $percent }% growth
classic.pedia.describe.modifier-cost = { $percent }% { $target }
classic.pedia.describe.modifier-combat = { $percent }% combat strength
classic.pedia.describe.modifier-combat-in = { $text } { $context }
classic.pedia.describe.modifier-movement = { $amount } movement
classic.pedia.describe.modifier-unlocks = unlocks the { $name }
classic.pedia.describe.modifier-free-unit = { $count } free { $name }
classic.pedia.describe.modifier-free-building = a free { $name }
classic.pedia.describe.modifier-promotion = the { $name } promotion
classic.pedia.describe.modifier-free-building-in-cities = { $count ->
    [one] a free { $name } in the first city
   *[other] a free { $name } in the first { $count } cities
}
classic.pedia.describe.modifier-yield-per-count = { $amount } { $yield } { $per }
classic.pedia.describe.modifier-yield-percent-per-count = { $percent }% { $yield } { $per }
classic.pedia.describe.modifier-yield-from-kill = { $yield } for each enemy unit killed, more for stronger units
classic.pedia.describe.modifier-forbid-unit = cannot train or buy { $name } units
classic.pedia.describe.modifier-forbid-annexing = cannot annex cities
classic.pedia.describe.modifier-post-combat-promotion = may gain the { $name } promotion after its first combat
classic.pedia.describe.modifier-passive-promotion = eligible units receive the effects of the { $name } promotion
classic.pedia.describe.modifier-experience = +{ $amount } experience points

## "per 10 citizens (at most 15)": the counted thing agrees with the number
classic.pedia.describe.per-count-one = per { $unit }
classic.pedia.describe.per-count-many = per { $every } { $unit }
classic.pedia.describe.per-count-capped = { $per } (at most { $max })
classic.pedia.describe.count-population = { $n ->
    [one] citizen
   *[other] citizens
}
classic.pedia.describe.count-world-wonders = { $n ->
    [one] world wonder
   *[other] world wonders
}
classic.pedia.describe.count-connected-cities = { $n ->
    [one] city connected to the capital
   *[other] cities connected to the capital
}
classic.pedia.describe.per-population-step = { $n ->
    [one] +1 { $yield } per citizen
   *[other] +1 { $yield } per { $n } citizens
}
classic.pedia.describe.per-population = { $amount } { $yield } per citizen

## what a cost bonus changes
classic.pedia.describe.cost-units = unit production cost
classic.pedia.describe.cost-buildings = building production cost
classic.pedia.describe.cost-technologies = technology cost
classic.pedia.describe.cost-tile-purchase = tile purchase cost
classic.pedia.describe.cost-hurry = purchase cost
classic.pedia.describe.cost-border-growth = culture needed for border growth
classic.pedia.describe.cost-border-growth-exponent = border growth cost increase
classic.pedia.describe.cost-unit-upgrade = unit upgrade cost
classic.pedia.describe.cost-unit-purchase = gold cost of buying units
classic.pedia.describe.cost-building-purchase = gold cost of buying buildings

## where a combat bonus applies
classic.pedia.describe.context-attack = when attacking
classic.pedia.describe.context-defense = when defending
classic.pedia.describe.context-vs-city = against cities
classic.pedia.describe.context-attack-open = when attacking in open terrain
classic.pedia.describe.context-attack-rough = when attacking in rough terrain
classic.pedia.describe.context-defense-open = when defending in open terrain
classic.pedia.describe.context-defense-rough = when defending in rough terrain
classic.pedia.describe.context-ranged-attack-open = for ranged attacks on open terrain
classic.pedia.describe.context-ranged-attack-rough = for ranged attacks on rough terrain
classic.pedia.describe.context-defense-vs-ranged = when defending against ranged attacks
classic.pedia.describe.context-ranged-attack = for ranged attacks
classic.pedia.describe.context-vs-wounded = against wounded units
classic.pedia.describe.context-vs-fortified = against fortified units
classic.pedia.describe.context-friendly-land = in friendly territory
classic.pedia.describe.context-outside-friendly-land = outside friendly territory
classic.pedia.describe.context-attack-friendly-land = when attacking in friendly territory
classic.pedia.describe.context-flanking = per flanking unit
classic.pedia.describe.context-adjacent-friendly = next to a friendly unit
classic.pedia.describe.context-near-capital = near the capital
classic.pedia.describe.context-embarked-defense = when embarked and defending
classic.pedia.describe.context-interception = when intercepting
classic.pedia.describe.context-air-sweep = on air sweeps

## city combat
classic.pedia.describe.city-combat-defense = { $amount } city combat strength from defenses
classic.pedia.describe.city-combat-hit-points = { $amount } city hit points
classic.pedia.describe.city-combat-defense-percent = { $amount }% defensive strength from buildings
classic.pedia.describe.city-combat-garrison-ranged = { $amount }% city ranged attack strength while garrisoned
classic.pedia.describe.city-combat-nuke-damage = { $amount }% damage received from nuclear attacks
classic.pedia.describe.city-combat-air-capacity = { $amount } aircraft capacity
classic.pedia.describe.city-combat-ranged = { $amount }% city ranged attack strength

## espionage and the World Congress
classic.pedia.describe.espionage-enemy-steal = { $amount }% speed of enemy spies stealing technologies
classic.pedia.describe.espionage-steal = { $amount }% speed of your spies stealing technologies
classic.pedia.describe.espionage-catch = { $amount }% chance to catch enemy spies
classic.pedia.describe.espionage-rig-election = { $amount }% votes when rigging city-state elections
classic.pedia.describe.espionage-extra-spies = { $amount } spies with the first spy
classic.pedia.describe.espionage-starting-rank = new spies start { $amount } ranks higher
classic.pedia.describe.espionage-spy-pressure = spies exert { $amount }x religious pressure in the city they are in
classic.pedia.describe.league-delegates = { $amount } delegates in the World Congress
classic.pedia.describe.league-delegates-per-diplomat = { $amount } delegates per diplomat in a foreign capital

## economy and society stats
classic.pedia.describe.stat-resource-copies = provides { $amount } { $resource }
classic.pedia.describe.stat-resource-quantity = { $amount }% { $resource } from your tiles
classic.pedia.describe.stat-strategic-quantity = { $amount }% strategic resources from your tiles
classic.pedia.describe.stat-extra-luxury-copies = { $amount } copy of each luxury around the city
classic.pedia.describe.stat-great-people-rate = { $amount }% great people
classic.pedia.describe.stat-great-person-rate = { $amount }% { $kind } points
classic.pedia.describe.stat-food-kept = { $amount }% food kept on growth
classic.pedia.describe.stat-specialist-yield = { $amount } { $yield } per specialist
classic.pedia.describe.stat-happiness-per-city = { $amount } happiness per city
classic.pedia.describe.stat-happiness-per-luxury = { $amount } happiness per luxury
classic.pedia.describe.stat-natural-wonder-happiness = { $amount }% happiness from natural wonders
classic.pedia.describe.stat-city-count-unhappiness = { $amount }% unhappiness from the number of cities
classic.pedia.describe.stat-population-unhappiness = { $amount }% unhappiness from population
classic.pedia.describe.stat-golden-age-length = { $amount }% golden age length
classic.pedia.describe.stat-connection-gold = { $amount }% gold from city connections
classic.pedia.describe.stat-research-agreement = { $amount }% science from research agreements
classic.pedia.describe.stat-great-scientist-science = { $amount }% science from Great Scientists
classic.pedia.describe.stat-great-people-per-friend = { $amount }% great people per declaration of friendship
classic.pedia.describe.stat-free-maintenance-units = { $amount } units with no gold maintenance
classic.pedia.describe.stat-barbarian-conversion = { $amount }% chance that a barbarian land unit defeated inside its camp joins you and brings 25 gold
classic.pedia.describe.stat-plunder-gold = { $amount }% gold from barbarian camps and captured cities
classic.pedia.describe.stat-captured-gold = { $amount }% gold for whoever captures this city
classic.pedia.describe.stat-worker-speed = { $amount }% worker speed
classic.pedia.describe.stat-route-maintenance = { $amount }% road and railroad maintenance
classic.pedia.describe.stat-any-specialist-yield = { $amount } { $yield } in a city with a specialist

classic.pedia.describe.rule-garrison-free-maintenance = units garrisoned in cities cost no maintenance
classic.pedia.describe.rule-free-hills-route-maintenance = no maintenance for roads and railroads on hills
classic.pedia.describe.rule-spaceship-part-purchase = spaceship parts may be bought with gold
classic.pedia.describe.rule-spaceship-part-hurry = Great Engineers may hurry spaceship parts
classic.pedia.describe.rule-see-barbarian-camps = new barbarian encampments show on explored land

classic.pedia.describe.society-specialist-food = { $amount }% food eaten by specialists
classic.pedia.describe.society-specialist-unhappiness = { $amount }% unhappiness from specialists
classic.pedia.describe.society-no-occupied-unhappiness = no extra unhappiness while occupied
classic.pedia.describe.society-wonder-culture = { $amount }% culture from world wonders
classic.pedia.describe.society-happiness-to-culture = excess happiness is turned into culture
classic.pedia.describe.society-science-while-happy = { $amount }% science while the empire is happy
classic.pedia.describe.society-luxury-retention = keeps { $amount }% of a luxury's happiness when its last copy is traded away
classic.pedia.describe.society-raze-speed = { $amount }% razing speed
classic.pedia.describe.society-founding-territory = new cities claim { $amount } extra tiles
classic.pedia.describe.society-capital-science-boost = science buildings built in the capital give a research boost
classic.pedia.describe.society-unique-luxury-cities = the first { $amount } cities on other landmasses get a unique luxury
classic.pedia.describe.society-unique-luxury-quantity = { $amount } copies of each unique luxury
classic.pedia.describe.society-unique-luxury-new-landmass = unique luxuries only on landmasses away from the capital

## trade
classic.pedia.describe.trade-routes = { $amount } trade routes
classic.pedia.describe.trade-range-tiles = { $amount } tiles of { $domain } trade route range
classic.pedia.describe.trade-range-percent = { $amount }% { $domain } trade route range
classic.pedia.describe.trade-route-gold = { $amount } gold from { $domain } trade routes
classic.pedia.describe.trade-target-gold = { $amount } gold for trade routes to this city
classic.pedia.describe.trade-recipient-gold = { $amount } gold to this city from foreign trade routes
classic.pedia.describe.trade-internal-percent = { $amount }% food and production of internal trade routes
classic.pedia.describe.trade-routes-percent = { $amount }% trade routes
classic.pedia.describe.trade-resource-gold-percent = { $amount }% gold from resource diversity on trade routes
classic.pedia.describe.trade-incoming-route-gold = { $amount } gold to whoever sends you a trade route
classic.pedia.describe.trade-city-state-route-gold = { $amount } gold from trade routes to city-states
classic.pedia.describe.trade-shared-ideology-route-gold = { $amount } gold from trade routes to nations of your ideology
classic.pedia.describe.trade-religious-pressure = { $amount }% religious pressure along trade routes

## natural wonder placement
classic.pedia.describe.placement-land = Placed on land
classic.pedia.describe.placement-water = Rises from the sea
classic.pedia.describe.placement-reef = Placed in open water near a coast
classic.pedia.describe.placement-peninsula-tip = Placed at the tip of a peninsula
classic.pedia.describe.placement-of-terrains = { $text } of { $terrains }
classic.pedia.describe.placement-away-from-coast = { $text }, away from the coast
classic.pedia.describe.placement-off-largest-landmass = { $text }, off the largest landmass
classic.pedia.describe.placement-sentence = { $text }.
classic.pedia.describe.adjacent-hills = hills
classic.pedia.describe.adjacent-mountains = mountains
classic.pedia.describe.adjacent-hills-or-mountains = hills or mountains
classic.pedia.describe.adjacent-land = land tiles
classic.pedia.describe.adjacent-water = water tiles
classic.pedia.describe.adjacent-at-least = At least { $n } neighbouring { $what }.
classic.pedia.describe.adjacent-none = No neighbouring { $what }.
classic.pedia.describe.adjacent-at-most = At most { $n } neighbouring { $what }.
classic.pedia.describe.shape-mountain = The tile becomes a mountain.
classic.pedia.describe.shape-flat = The tile becomes flat land.
classic.pedia.describe.surrounded-by-sea = The sea surrounds it.

## grants
classic.pedia.describe.grant-free-technologies = { $n ->
    [one] a free technology of your choice
   *[other] { $n } free technologies of your choice
}
classic.pedia.describe.grant-conquest-technology = a free technology known by the city's former owner (once per city)
classic.pedia.describe.grant-military-unit-copies = a copy of each of your land combat units
classic.pedia.describe.grant-population = { $n ->
    [one] { $amount } citizen
   *[other] { $amount } citizens
}

## unit stats
classic.pedia.describe.unit-stat-sight = { $amount } tiles of sight
classic.pedia.describe.unit-stat-embarked-sight = { $amount } tiles of sight while embarked
classic.pedia.describe.unit-stat-range = { $amount } tiles of ranged attack range
classic.pedia.describe.unit-stat-attacks-per-turn = { $amount } attacks per turn
classic.pedia.describe.unit-stat-cargo = { $amount } aircraft or missile carrying capacity
classic.pedia.describe.unit-stat-intercept-chance = { $amount } percentage points to interception chance
classic.pedia.describe.unit-stat-interceptions-per-turn = { $amount } interceptions per turn
classic.pedia.describe.unit-stat-evasion = { $amount } percentage points to evading interception
classic.pedia.describe.unit-stat-interception-damage = { $amount }% damage received from interceptions
classic.pedia.describe.unit-stat-paradrop-range = { $amount } tiles of paradrop range
classic.pedia.describe.unit-stat-naval-moves = { $amount } movement at sea
classic.pedia.describe.unit-stat-experience = { $amount }% experience earned
classic.pedia.describe.unit-stat-withdraw-chance = { $amount } percentage points to withdrawal chance before melee combat
classic.pedia.describe.unit-stat-city-attack-plunder = earns Gold when attacking cities, more for more damage
classic.pedia.describe.unit-stat-heal-enemy-land = { $amount } HP healed per turn in enemy territory
classic.pedia.describe.unit-stat-heal-neutral-land = { $amount } HP healed per turn in neutral territory
classic.pedia.describe.unit-stat-heal-friendly-land = { $amount } HP healed per turn in friendly land
classic.pedia.describe.unit-stat-heal-adjacent = adjacent friendly units heal { $amount } HP per turn
classic.pedia.describe.unit-stat-heal-on-kill = { $amount } HP healed after destroying an enemy unit
classic.pedia.describe.unit-stat-enemy-aura = { $amount }% combat strength for nearby enemy units
classic.pedia.describe.unit-stat-aura-range = { $amount } tiles to the range of the nearby-enemy effect
classic.pedia.describe.unit-stat-adjacent-damage = { $amount } damage to adjacent enemy units after attacking
classic.pedia.describe.unit-stat-capital-defense-falloff = changes how quickly the capital-defense bonus fades with distance
classic.pedia.describe.unit-stat-embarked-moves = { $amount } movement while embarked
classic.pedia.describe.unit-stat-wounded-damage = { $amount } percentage points to the wounded-unit damage modifier

## unit abilities (what the catalog says a unit can do)
classic.pedia.describe.ability-found-religion = Can found or enhance a religion
classic.pedia.describe.ability-spread-religion = Can spread its religion to a city
classic.pedia.describe.ability-religion-spreads = Can spread religion { $n ->
    [one] { $n } time
   *[other] { $n } times
}
classic.pedia.describe.ability-religious-strength = Religious strength: { $n }
classic.pedia.describe.ability-remove-heresy = Can drive other religions out of a city
classic.pedia.describe.ability-prohibits-spread = Its presence blocks the spread of other religions
classic.pedia.describe.ability-requires-enhanced-religion = Can be bought only once your religion is enhanced
classic.pedia.describe.ability-one-shot-tourism = A concert gives tourism that grows with your current tourism
classic.pedia.describe.ability-one-shot-tourism-others = Other nations get { $n }% of the tourism a concert gives
classic.pedia.describe.ability-culture-turns = Grants Culture in proportion to your recent culture output
classic.pedia.describe.ability-excavates = Can excavate antiquity sites
classic.pedia.describe.ability-food = While it is trained, the city's spare Food adds to Production
classic.pedia.describe.ability-golden-age-turns = Can start a Golden Age with a base duration of { $n ->
    [one] { $n } turn
   *[other] { $n } turns
}
classic.pedia.describe.ability-beakers-turns = Grants a burst of Science in proportion to your recent output
classic.pedia.describe.ability-base-hurry = Can add Production to what a city is building
classic.pedia.describe.ability-hurry-multiplier = A larger city receives more Production this way
classic.pedia.describe.ability-base-gold = Trade missions grant Gold
classic.pedia.describe.ability-gold-per-era = Trade missions pay more in later eras
classic.pedia.describe.ability-no-maintenance = Costs no Gold maintenance
classic.pedia.describe.ability-trade = Can establish trade routes
classic.pedia.describe.ability-faith-purchase-needs-belief = Can be bought with Faith only where a belief unlocks its era
classic.pedia.describe.ability-mechanized = A machine, not people
classic.pedia.describe.ability-priority-low = Enemies go for it last
classic.pedia.describe.ability-priority-early = Enemies go for it first in the early game
classic.pedia.describe.ability-priority-high = Enemies go for it first
classic.pedia.describe.ability-conscription = Conscription value { $n }
classic.pedia.describe.ability-great-person = A great person
classic.pedia.describe.ability-stealth = Stealth aircraft: no carrier takes it
classic.pedia.describe.ability-air-slots = Takes { $n ->
    [one] { $n } air slot
   *[other] { $n } air slots
} in a city
classic.pedia.describe.ability-work-rate = Work rate { $n }
classic.pedia.describe.ability-move-after-purchase = Can move on the turn it is bought
classic.pedia.describe.ability-no-bad-goodies = Ancient ruins never harm it
classic.pedia.describe.ability-purchase-only = Can only be bought, never trained
classic.pedia.describe.ability-buy-city-state = Turns the city-state it stands in into its owner's puppet
classic.pedia.describe.ability-found-abroad = Founds cities only off the capital's landmass
classic.pedia.describe.ability-exotic-goods = Sells { $n } cargo of exotic goods next to a foreign land

## combat abilities granted by effects
classic.pedia.describe.combat-ability-move-after-attacking = can move after attacking
classic.pedia.describe.combat-ability-always-heal = can heal after moving or attacking
classic.pedia.describe.combat-ability-embarkation = can embark to cross water
classic.pedia.describe.combat-ability-embarked-all-water = can enter deep water while embarked
classic.pedia.describe.combat-ability-move-impassable = can enter normally impassable terrain
classic.pedia.describe.combat-ability-ranged-support-fire = makes a supporting ranged strike before melee combat
classic.pedia.describe.combat-ability-hills-double-move = pays half the usual movement cost on hills
classic.pedia.describe.combat-ability-double-move = pays half the usual movement cost on the specified terrain
classic.pedia.describe.combat-ability-impassable = cannot enter the specified terrain
classic.pedia.describe.combat-ability-insta-heal = instantly heals the unit instead of adding a lasting promotion
classic.pedia.describe.combat-ability-amphibious = no attack penalty when landing from water or crossing a river
classic.pedia.describe.combat-ability-river-crossing = no attack penalty when crossing a river
classic.pedia.describe.combat-ability-indirect-fire = ranged attacks do not require line of sight
classic.pedia.describe.combat-ability-heal-outside-friendly = can heal at sea outside friendly territory
classic.pedia.describe.combat-ability-air-sweep = can perform air sweeps
classic.pedia.describe.combat-ability-rival-territory = can enter rival territory without Open Borders
classic.pedia.describe.combat-ability-rough-terrain-ends-turn = entering rough terrain ends this unit's movement
classic.pedia.describe.combat-ability-ignore-terrain-cost = ignores extra terrain movement costs
classic.pedia.describe.combat-ability-flat-movement-cost = every tile has the same movement cost, including roads
classic.pedia.describe.combat-ability-hovering = can move over land and water
classic.pedia.describe.combat-ability-no-capture = cannot capture cities or units
classic.pedia.describe.combat-ability-only-defensive = cannot make melee attacks
classic.pedia.describe.combat-ability-no-defensive-bonus = receives no terrain defense or fortification bonus
classic.pedia.describe.combat-ability-must-set-up = must set up before a ranged attack
classic.pedia.describe.combat-ability-free-pillage-moves = pillaging costs no movement
classic.pedia.describe.combat-ability-city-attack-only = can attack only cities
classic.pedia.describe.combat-ability-capture-defeated-enemy = can capture defeated enemy units
classic.pedia.describe.combat-ability-heavy-charge = stronger attacks against units that cannot withdraw
classic.pedia.describe.combat-ability-sapper = helps adjacent friendly units attack a city next to this unit
classic.pedia.describe.combat-ability-ignore-zone-of-control = ignores enemy zones of control
classic.pedia.describe.combat-ability-embarked-to-land-flat-cost = landing from the water costs one movement point
classic.pedia.describe.combat-ability-hills-ignore-terrain-cost = ignores terrain costs when moving onto hills
classic.pedia.describe.combat-ability-cross-mountains = can cross mountains
classic.pedia.describe.combat-ability-road-in-friendly-land = moves along the specified terrain in its own territory as if along a road
classic.pedia.describe.combat-ability-invisible = seen only next to enemy units and cities or by units that see submarines
classic.pedia.describe.combat-ability-see-invisible = sees submarines within its sight

## effects the catalog lists but the engine does not evaluate yet
classic.pedia.describe.inert-amount = { $subject }: { $amount }
classic.pedia.describe.inert-refs = { $text } ({ $names })
classic.pedia.describe.inert-trigger = { $trigger }: { $text }

## building abilities
classic.pedia.describe.building-ideology-choice = With { $n } of these you may choose an ideology
classic.pedia.describe.building-free-great-work = Comes with a free Great Work of Art
classic.pedia.describe.building-nullify-influence = Shields your nation from the tourism bonus other nations get from the Internet
classic.pedia.describe.building-missionary-extra-spreads = Missionaries purchased here can spread religion { $n ->
    [one] { $n } additional time
   *[other] { $n } additional times
}
classic.pedia.describe.building-religious-pressure = +{ $n }% religious pressure from this city
classic.pedia.describe.building-food-trade-routes = This city can send Food trade routes to your other cities
classic.pedia.describe.building-production-trade-routes = This city can send Production trade routes to your other cities
classic.pedia.describe.building-counter-espionage = Provides counterespionage in this city
classic.pedia.describe.building-affect-spies-now = Spies already gathering intelligence here feel its espionage effects at once
classic.pedia.describe.building-border-obstacle = Enemy land units spend 1 extra movement point per tile in your territory
classic.pedia.describe.building-airlift = Land units may be airlifted to or from this city
classic.pedia.describe.building-allows-range-strike = Allows this city to make ranged strikes

## a name that could not be found
classic.pedia.describe.unknown = Unknown

# ---- describe_classic.rs: the Civ V words ----

## triggers and selectors
classic.pedia.describe-classic.trigger-on-adopt = when adopted
classic.pedia.describe-classic.trigger-wonder-found-first = on finding a natural wonder first
classic.pedia.describe-classic.trigger-wonder-found = on finding a natural wonder another nation found before
classic.pedia.describe-classic.trigger-long-count = at the end of each Long Count cycle
classic.pedia.describe-classic.trigger-great-person-expended = when a Great Person is expended
classic.pedia.describe-classic.trigger-city-converted = when a city first converts to the religion
classic.pedia.describe-classic.trigger-religion-spread = when a missionary or prophet spreads the religion
classic.pedia.describe-classic.trigger-great-person-born = when a great person is born

classic.pedia.describe-classic.selector-following-cities = in cities following the religion
classic.pedia.describe-classic.selector-following-city-tiles = on tiles of cities following the religion
classic.pedia.describe-classic.selector-holy-city = in the holy city
classic.pedia.describe-classic.selector-founder = for the founder
classic.pedia.describe-classic.selector-own-religion = for the religion

## conditions
classic.pedia.describe-classic.condition-barbarian = against barbarians
classic.pedia.describe-classic.condition-city-state = against city-states
classic.pedia.describe-classic.condition-more-cities = against nations with more cities
classic.pedia.describe-classic.condition-min-followers = with { $n ->
    [one] { $n } follower
   *[other] { $n } followers
} or more
classic.pedia.describe-classic.condition-near-own-following-city = near own cities following the religion
classic.pedia.describe-classic.condition-near-enemy-following-city = near enemy cities following the religion
classic.pedia.describe-classic.condition-golden-age = during a golden age
classic.pedia.describe-classic.condition-great-general-earned = once a great general has been earned
classic.pedia.describe-classic.condition-great-person-is = if it is a { $kind }

## modifiers
classic.pedia.describe-classic.modifier-happiness = { $amount } happiness
classic.pedia.describe-classic.modifier-great-person-points = { $amount } { $kind } points
classic.pedia.describe-classic.modifier-free-policies = { $count ->
    [one] { $count } free social policy
   *[other] { $count } free social policies
}
classic.pedia.describe-classic.modifier-yield-per-following-city = { $foreign ->
    [true] { $amount } { $yield } per foreign city following the religion
   *[false] { $amount } { $yield } per city following the religion
}
classic.pedia.describe-classic.modifier-yield-per-followers = { $foreign ->
    [true] +1 { $yield } per { $per } foreign followers
   *[false] +1 { $yield } per { $per } followers
}
classic.pedia.describe-classic.modifier-happiness-per-following-city = { $amount } happiness per city following the religion
classic.pedia.describe-classic.modifier-religion = { $percent }% { $stat }
classic.pedia.describe-classic.modifier-faith-building = the { $name } may be bought with faith
classic.pedia.describe-classic.modifier-faith-units = land units of the { $era } may be bought with faith
classic.pedia.describe-classic.modifier-reformation-belief = a Reformation belief may be added
classic.pedia.describe-classic.modifier-tourism-toward = { $percent }% tourism toward nations { $when }
classic.pedia.describe-classic.modifier-happiness-per-count = { $amount } happiness { $per }
classic.pedia.describe-classic.modifier-grant-spies = { $count ->
    [one] a free spy
   *[other] { $count } free spies
}
classic.pedia.describe-classic.modifier-promote-spies = { $ranks ->
    [one] every spy rises one rank
   *[other] every spy rises { $ranks } ranks
}
classic.pedia.describe-classic.modifier-extra-founding-belief = one more belief when founding a religion
classic.pedia.describe-classic.modifier-second-religion-pantheon = cities also get the pantheon belief of their second religion
classic.pedia.describe-classic.modifier-instant-tourism = +{ $amount } tourism with every known nation
classic.pedia.describe-classic.modifier-experience-per-great-work = units trained here start with { $amount } experience per Great Work in the city

## grants
classic.pedia.describe-classic.grant-free-great-people = { $n ->
    [one] a free great person of your choice
   *[other] { $n } free great people of your choice
}
classic.pedia.describe-classic.grant-long-count-great-person = a free great person of a kind not chosen before
classic.pedia.describe-classic.grant-golden-age = a golden age of { $turns ->
    [one] { $turns } turn
   *[other] { $turns } turns
}

## unit stats and abilities
classic.pedia.describe-classic.unit-stat-general-points = { $amount }% Great General points from combat
classic.pedia.describe-classic.unit-stat-stacked-general = { $amount }% combat strength for a unit sharing this Great General's tile
classic.pedia.describe-classic.unit-stat-maintenance = { $amount }% Gold maintenance
classic.pedia.describe-classic.unit-stat-experience-needed = { $amount }% experience needed for a promotion
classic.pedia.describe-classic.unit-stat-general-aura = { $amount }% combat bonus of Great Generals and Admirals
classic.pedia.describe-classic.unit-stat-mission-gold = { $amount }% gold from a trade mission
classic.pedia.describe-classic.unit-stat-mission-influence = { $amount }% influence from a trade mission
classic.pedia.describe-classic.unit-stat-adjacent-damage-chance = { $amount }% chance to damage adjacent enemies after attacking
classic.pedia.describe-classic.unit-stat-rival-land-loss = loses religious strength for each turn it ends in rival territory
classic.pedia.describe-classic.unit-stat-golden-age-on-kill = kills add golden age points, more for stronger victims

classic.pedia.describe-classic.ability-great-general = provides a Great General's combat bonus to nearby friendly land units
classic.pedia.describe-classic.ability-post-combat-promotions = is replaced by a random promotion after its first combat
classic.pedia.describe-classic.ability-lends-movement = a Great General stacked with it at the start of the turn moves at its rate
classic.pedia.describe-classic.ability-ruins-reward = takes the best of the rewards when it enters Ancient Ruins
classic.pedia.describe-classic.ability-exotic-cargo = carries exotic goods to sell once next to a foreign land
classic.pedia.describe-classic.ability-great-admiral = provides a Great Admiral's combat bonus to nearby friendly naval units
classic.pedia.describe-classic.ability-heal-not-barbarians = healing after a kill does not apply to barbarians

## costs and counts
classic.pedia.describe-classic.cost-policies = social policy cost
classic.pedia.describe-classic.cost-policy-city-penalty = policy cost per city
classic.pedia.describe-classic.cost-faith-purchase = faith purchase cost
classic.pedia.describe-classic.count-policies = { $n ->
    [one] social policy
   *[other] social policies
}
classic.pedia.describe-classic.count-majority-followers = { $n ->
    [one] follower
   *[other] followers
}
classic.pedia.describe-classic.count-religions-present = { $n ->
    [one] religion present
   *[other] religions present
}
classic.pedia.describe-classic.count-peaceful-foreign-followers = { $n ->
    [one] follower in a foreign city not at war
   *[other] followers in foreign cities not at war
}
classic.pedia.describe-classic.count-other-religion-followers = { $n ->
    [one] follower of other religions
   *[other] followers of other religions
}
classic.pedia.describe-classic.count-trade-partners = { $n ->
    [one] trading partner
   *[other] trading partners
}
classic.pedia.describe-classic.count-city-state-routes = { $n ->
    [one] trade route to a city-state
   *[other] trade routes to city-states
}

## World Congress project rewards
classic.pedia.describe-classic.league-happiness = +{ $n } happiness
classic.pedia.describe-classic.league-free-policies = { $n ->
    [one] { $n } free social policy
   *[other] { $n } free social policies
}
classic.pedia.describe-classic.league-golden-age-points = +{ $n } points toward a golden age
classic.pedia.describe-classic.league-culture-boost = +100% culture for { $n ->
    [one] { $n } turn
   *[other] { $n } turns
}
classic.pedia.describe-classic.league-tourism-boost = +100% tourism for { $n ->
    [one] { $n } turn
   *[other] { $n } turns
}
classic.pedia.describe-classic.league-city-state-influence = +{ $n } influence with every city-state met
classic.pedia.describe-classic.league-research-turns = science of the last { $n ->
    [one] turn
   *[other] { $n } turns
}
classic.pedia.describe-classic.league-free-unit = a free { $name } in the capital
classic.pedia.describe-classic.league-building = the { $name } in the capital

## city-states
classic.pedia.describe-classic.city-state-resting-point = { $amount } resting influence with city-states
classic.pedia.describe-classic.city-state-decay = { $amount }% influence decay with city-states
classic.pedia.describe-classic.city-state-gold-gift = { $amount }% influence from gold gifts to city-states
classic.pedia.describe-classic.city-state-bully-score = { $amount }% military might when demanding tribute
classic.pedia.describe-classic.city-state-unit-gift-influence = { $amount } influence per unit gifted to a city-state
classic.pedia.describe-classic.city-state-friend-bonus = { $amount }% food, culture and faith from city-state friends
classic.pedia.describe-classic.city-state-friendship-rate = { $amount }% influence recovery with city-states; { $decay }% influence decay
classic.pedia.describe-classic.city-state-annex = may buy out city-states allied for five turns
classic.pedia.describe-classic.city-state-resting-point-religion = { $amount } resting point of influence with city-states following the religion
classic.pedia.describe-classic.city-state-afraid-influence = { $amount } influence per turn with city-states afraid of you
classic.pedia.describe-classic.city-state-trade-route-influence = { $amount } influence per turn with city-states you have a trade route with
classic.pedia.describe-classic.city-state-unit-gift-rate = militaristic city-states give units { $amount }% faster while you share an enemy
classic.pedia.describe-classic.city-state-ally-resources = allied city-states give twice their resources, their luxuries 50% more happiness
classic.pedia.describe-classic.city-state-ally-great-people = allied city-states give a great person now and then
classic.pedia.describe-classic.city-state-ally-science = a quarter of the science of allied city-states
classic.pedia.describe-classic.city-state-great-person-gift = { $amount } influence per great person gifted to a city-state
classic.pedia.describe-classic.city-state-mission-gold = { $amount }% gold from trade missions

## great people
classic.pedia.describe-classic.great-artist = Great Artist
classic.pedia.describe-classic.great-engineer = Great Engineer
classic.pedia.describe-classic.great-merchant = Great Merchant
classic.pedia.describe-classic.great-scientist = Great Scientist
classic.pedia.describe-classic.great-writer = Great Writer
classic.pedia.describe-classic.great-musician = Great Musician
classic.pedia.describe-classic.great-general = Great General
classic.pedia.describe-classic.great-admiral = Great Admiral
classic.pedia.describe-classic.great-prophet = Great Prophet

## religion
classic.pedia.describe-classic.religion-pressure-out = religious pressure
classic.pedia.describe-classic.religion-spread-distance = religious spread distance
classic.pedia.describe-classic.religion-missionary-strength = missionary strength
classic.pedia.describe-classic.religion-prophet-strength = Great Prophet strength
classic.pedia.describe-classic.religion-prophet-cost = Great Prophet faith cost
classic.pedia.describe-classic.religion-missionary-cost = missionary faith cost
classic.pedia.describe-classic.religion-friendly-city-state-pressure = religious pressure on friendly city-states
classic.pedia.describe-classic.religion-other-erosion = of a missionary's pressure taken from other religions
classic.pedia.describe-classic.religion-pressure-retention = of its pressure kept when inquisitors or prophets remove it
classic.pedia.describe-classic.religion-converts-barbarians = missionaries convert adjacent barbarian units
classic.pedia.describe-classic.religion-faith-great-people = every Great Person may be bought with faith from the Industrial era

## tourism
classic.pedia.describe-classic.tourism-flat = { $amount } tourism
classic.pedia.describe-classic.tourism-percent = { $amount }% tourism
classic.pedia.describe-classic.tourism-great-works = { $amount }% tourism from great works
classic.pedia.describe-classic.tourism-landmarks = { $amount }% tourism from landmarks and wonders
classic.pedia.describe-classic.tourism-per-faith-building = { $amount } tourism per faith building
classic.pedia.describe-classic.tourism-spread = { $amount }% tourism spread
classic.pedia.describe-classic.tourism-theming = { $amount }% theming bonuses
classic.pedia.describe-classic.tourism-great-work-culture = { $amount } culture per great work
classic.pedia.describe-classic.tourism-shared-religion = that share your religion
classic.pedia.describe-classic.tourism-trade-route = you have a trade route with
classic.pedia.describe-classic.tourism-open-borders = with open borders
classic.pedia.describe-classic.tourism-same-ideology = of your ideology
classic.pedia.describe-classic.tourism-less-happy = less happy than you
classic.pedia.describe-classic.tourism-common-enemy = at war with your enemies
