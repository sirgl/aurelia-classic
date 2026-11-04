# The Encyclopedia's pages (ui/pedia/entries.rs and entries_world.rs): subtitles, stat labels,
# link headings and the sentences around the numbers. Names of rule data come from
# content-classic.ftl. Source of truth: en.

## shared
classic.pedia.entries.subtitle-join = { $a } · { $b }
classic.pedia.entries.era-label = { $name } era
classic.pedia.entries.value-percent = { $n }%
classic.pedia.entries.value-science = { NUMBER($n) } Science
classic.pedia.entries.value-production = { NUMBER($n) } Production
classic.pedia.entries.value-gold = { NUMBER($n) } Gold
classic.pedia.entries.value-count-name = { $count } { $name }
classic.pedia.entries.value-up-to = up to +{ $n }

## stat labels
classic.pedia.entries.stat-leader = Leader
classic.pedia.entries.stat-adjective = Adjective
classic.pedia.entries.stat-friends-at-influence = Friends at influence
classic.pedia.entries.stat-allies-at-influence = Allies at influence
classic.pedia.entries.stat-cost = Cost
classic.pedia.entries.stat-combat-strength = Combat strength
classic.pedia.entries.stat-ranged-strength = Ranged strength
classic.pedia.entries.stat-range = Range
classic.pedia.entries.stat-movement = Movement
classic.pedia.entries.stat-sight = Sight
classic.pedia.entries.stat-needs = Needs
classic.pedia.entries.stat-faith-cost = Faith cost
classic.pedia.entries.stat-domain = Domain
classic.pedia.entries.stat-maintenance = Maintenance
classic.pedia.entries.stat-specialist-slots = Specialist slots
classic.pedia.entries.stat-great-work-slots = Great work slots
classic.pedia.entries.stat-theming-bonus = Theming bonus
classic.pedia.entries.stat-victory = Victory
classic.pedia.entries.stat-built-once = Built once in the world
classic.pedia.entries.stat-cost-per-nation = Cost per nation
classic.pedia.entries.stat-points-from = Points from
classic.pedia.entries.stat-yields = Yields
classic.pedia.entries.stat-great-person-points = Great person points
classic.pedia.entries.stat-starting-gold = Starting gold
classic.pedia.entries.stat-starting-military = Starting military units
classic.pedia.entries.stat-starting-workers = Starting workers
classic.pedia.entries.stat-starting-scouts = Starting scouts
classic.pedia.entries.stat-research-agreement-cost = Research agreement cost
classic.pedia.entries.stat-base-happiness = Base happiness
classic.pedia.entries.stat-happiness-per-luxury = Happiness per luxury
classic.pedia.entries.stat-research-cost = Research cost
classic.pedia.entries.stat-policy-cost = Policy cost
classic.pedia.entries.stat-unit-cost = Unit cost
classic.pedia.entries.stat-building-cost = Building cost
classic.pedia.entries.stat-free-unit-supply = Free unit supply

## link headings (a page lists the pages it links to under these)
classic.pedia.entries.link-unique-units = Unique units
classic.pedia.entries.link-unique-buildings = Unique buildings
classic.pedia.entries.link-unique-improvements = Unique improvements
classic.pedia.entries.link-free-technologies = Free technologies
classic.pedia.entries.link-starting-units = Starting units
classic.pedia.entries.link-preferred-religions = Preferred religions
classic.pedia.entries.link-related = Related
classic.pedia.entries.link-type = Type
classic.pedia.entries.link-requires = Requires
classic.pedia.entries.link-requires-one-of = Requires one of
classic.pedia.entries.link-unlocks = Unlocks
classic.pedia.entries.link-era = Era
classic.pedia.entries.link-required-technology = Required technology
classic.pedia.entries.link-obsolete-with = Obsolete with
classic.pedia.entries.link-upgrades-to = Upgrades to
classic.pedia.entries.link-replaces = Replaces
classic.pedia.entries.link-unique-to = Unique to
classic.pedia.entries.link-requires-resources = Requires resources
classic.pedia.entries.link-requires-buildings = Requires buildings
classic.pedia.entries.link-requires-projects = Requires projects
classic.pedia.entries.link-requires-policy-branch = Requires policy branch
classic.pedia.entries.link-free-promotions = Free promotions
classic.pedia.entries.link-specialists = Specialists
classic.pedia.entries.link-proposed-once = Proposed once a member knows
classic.pedia.entries.link-for-units = For units
classic.pedia.entries.link-branch = Branch
classic.pedia.entries.link-opener = Opener
classic.pedia.entries.link-policies = Policies
classic.pedia.entries.link-finisher = Finisher
classic.pedia.entries.link-excludes = Excludes
classic.pedia.entries.link-preferred-by = Preferred by
classic.pedia.entries.link-unit = Unit
classic.pedia.entries.link-born-from = Born from
classic.pedia.entries.link-builds = Builds
classic.pedia.entries.link-great-person = Great person
classic.pedia.entries.link-technologies = Technologies
# the other end of a link
classic.pedia.entries.link-leads-to = Leads to
classic.pedia.entries.link-makes-obsolete = Makes obsolete
classic.pedia.entries.link-upgrades-from = Upgrades from
classic.pedia.entries.link-replaced-by = Replaced by
classic.pedia.entries.link-needed-by = Needed by
classic.pedia.entries.link-given-to = Given to
classic.pedia.entries.link-city-states = City-states

## nations and city-states
classic.pedia.entries.led-by = Led by { $leader }
classic.pedia.entries.unique-ability = Unique ability: { $name }
classic.pedia.entries.starts-with = Starts with { $count } { $name }
classic.pedia.entries.city-state-of-type = { $kind } city-state
classic.pedia.entries.city-state-type = Type of city-state

## technologies
classic.pedia.entries.technology = Technology
classic.pedia.entries.repeatable = May be researched again and again.
classic.pedia.entries.allows-action = Allows: { $action }
classic.pedia.entries.tech-antiquity-sites = The first to learn it makes the antiquity sites appear
classic.pedia.entries.tech-reveals-map = Reveals the whole map
classic.pedia.entries.tech-bridges = Roads cross rivers without losing the move
classic.pedia.entries.tech-roads-faster = Movement along roads costs less
classic.pedia.entries.tech-roads-slower = Movement along roads costs more

## units: the kind of unit, the domain
classic.pedia.entries.unit-of-role = { $role } unit
classic.pedia.entries.unique-to = Unique to { $nation }
classic.pedia.entries.role-civilian = Civilian
classic.pedia.entries.role-great-person = Great Person
classic.pedia.entries.role-religious = Religious
classic.pedia.entries.role-spaceship-part = Spaceship part
classic.pedia.entries.role-recon = Recon
classic.pedia.entries.role-melee = Melee
classic.pedia.entries.role-archer = Archery
classic.pedia.entries.role-mounted = Mounted
classic.pedia.entries.role-siege = Siege
classic.pedia.entries.role-gun = Gunpowder
classic.pedia.entries.role-armor = Armor
classic.pedia.entries.role-naval-melee = Naval melee
classic.pedia.entries.role-naval-ranged = Naval ranged
classic.pedia.entries.role-submarine = Submarine
classic.pedia.entries.role-carrier = Carrier
classic.pedia.entries.role-fighter = Fighter
classic.pedia.entries.role-bomber = Bomber
classic.pedia.entries.role-helicopter = Helicopter
classic.pedia.entries.role-missile = Missile
classic.pedia.entries.domain-land = Land
classic.pedia.entries.domain-sea = Sea
classic.pedia.entries.domain-air = Air

## buildings
classic.pedia.entries.building-regular = Building
classic.pedia.entries.building-religious = Religious building
classic.pedia.entries.building-guild = Guild
classic.pedia.entries.building-palace = Palace
classic.pedia.entries.building-unique = Unique building
classic.pedia.entries.building-world-wonder = World wonder
classic.pedia.entries.building-national-wonder = National wonder
classic.pedia.entries.only-in-holy-city = Only in a holy city.
classic.pedia.entries.only-through-project = Only through the World Congress project { $name }.
classic.pedia.entries.faith-needs-belief = Bought with Faith only where a belief of the city's religion allows it.

## projects
classic.pedia.entries.project = Project
classic.pedia.entries.league-project = World Congress project
classic.pedia.entries.project-allows-nukes = Its owner may build nuclear weapons
classic.pedia.entries.league-tier-half = Two fifths of a share contributed
classic.pedia.entries.league-tier-full = Nine tenths of a share contributed
classic.pedia.entries.league-tier-largest = The largest contribution
classic.pedia.entries.league-tier = { $tier }: { $rewards }

## promotions
classic.pedia.entries.promotion = Promotion
classic.pedia.entries.promotion-for-roles = Promotion · { $roles }
classic.pedia.entries.promotion-granted = Granted, not chosen.
classic.pedia.entries.promotion-lost-on-upgrade = Lost when the unit upgrades.

## policies and branches
classic.pedia.entries.policy = Policy
classic.pedia.entries.policy-tenet = Tenet, level { $level }
classic.pedia.entries.policy-opener = Adopting the branch
classic.pedia.entries.policy-finisher = Completing the branch
classic.pedia.entries.branch-normal = Policy branch
classic.pedia.entries.branch-ideology = Ideology
classic.pedia.entries.branch-from-era = from the { $era }
classic.pedia.entries.branch-needs-religion = Needs a founded religion or pantheon.
classic.pedia.entries.branch-first-adopter = { $n ->
    [one] The first nation to adopt it gets { $n } free tenet.
   *[other] The first nation to adopt it gets { $n } free tenets.
}

## beliefs, religions, people, eras, difficulty levels
classic.pedia.entries.belief-pantheon = Pantheon belief
classic.pedia.entries.belief-founder = Founder belief
classic.pedia.entries.belief-follower = Follower belief
classic.pedia.entries.belief-enhancer = Enhancer belief
classic.pedia.entries.belief-reformation = Reformation belief
classic.pedia.entries.belief = Belief
classic.pedia.entries.religion = Religion
classic.pedia.entries.points-specialists = Great person points of specialists and buildings
classic.pedia.entries.points-combat = Experience of combat
classic.pedia.entries.points-faith = Faith
classic.pedia.entries.specialist = Specialist
classic.pedia.entries.era-number = Era { $n }
classic.pedia.entries.handicap-ai = Played by every AI nation
classic.pedia.entries.handicap-level = Difficulty level { $n }

# ---- entries_world.rs: terrain, features, resources, improvements, natural wonders ----

## shared
classic.pedia.entries-world.none = None
classic.pedia.entries-world.impassable = Impassable
classic.pedia.entries-world.turns = { $n ->
    [one] { $n } turn
   *[other] { $n } turns
}
classic.pedia.entries-world.worker-turns = { $n ->
    [one] { $n } worker turn
   *[other] { $n } worker turns
}

## stat labels
classic.pedia.entries-world.stat-as-hills = As hills
classic.pedia.entries-world.stat-movement-cost = Movement cost
classic.pedia.entries-world.stat-defense = Defense
classic.pedia.entries-world.stat-cities = Cities
classic.pedia.entries-world.stat-removed-in = Removed in
classic.pedia.entries-world.stat-chop-production = Chop production
classic.pedia.entries-world.stat-happiness-inside = Happiness inside your borders
classic.pedia.entries-world.stat-quantity = Quantity
classic.pedia.entries-world.stat-wonders = Wonders
classic.pedia.entries-world.stat-build-time = Build time
classic.pedia.entries-world.stat-pillage-gold = Pillage gold
classic.pedia.entries-world.stat-culture-per-era = Culture per era
classic.pedia.entries-world.stat-first-finder-gold = Gold for the first to find it

## link headings
classic.pedia.entries-world.link-resources = Resources
classic.pedia.entries-world.link-found-on = Found on
classic.pedia.entries-world.link-revealed-by = Revealed by
classic.pedia.entries-world.link-reveals = Reveals
classic.pedia.entries-world.link-tradeable-with = Tradeable with
classic.pedia.entries-world.link-improved-by = Improved by
classic.pedia.entries-world.link-improves = Improves
classic.pedia.entries-world.link-built-on = Built on
classic.pedia.entries-world.link-better-with = Better with
classic.pedia.entries-world.link-nearby-units-gain = Nearby units gain

## terrain and features
classic.pedia.entries-world.water-terrain = Water terrain
classic.pedia.entries-world.land-terrain = Land terrain
classic.pedia.entries-world.cities-may-be-founded = May be founded here
classic.pedia.entries-world.terrain-feature = Terrain feature
classic.pedia.entries-world.yields-replace = { $yields } (replaces the terrain's)
classic.pedia.entries-world.rough-terrain = Rough terrain.
classic.pedia.entries-world.provides-fresh-water = Provides fresh water.
classic.pedia.entries-world.cannot-be-improved = Cannot be improved.
classic.pedia.entries-world.only-along-rivers = Only along rivers.
classic.pedia.entries-world.only-on-flat-land = Only on flat land.

## resources
classic.pedia.entries-world.resource-bonus = Bonus resource
classic.pedia.entries-world.resource-strategic = Strategic resource
classic.pedia.entries-world.resource-luxury = Luxury resource
classic.pedia.entries-world.resource-artifact = Artifact
classic.pedia.entries-world.resource = Resource
classic.pedia.entries-world.wonder-production = { $percent }% Production toward world wonders in a city that has it
classic.pedia.entries-world.wonder-production-before = { $percent }% Production toward world wonders before the { $era } in a city that has it
classic.pedia.entries-world.appears-on-hills = Appears on hills.
classic.pedia.entries-world.appears-on-flat-land = Appears on flat land.
classic.pedia.entries-world.only-city-states = Only city-states have it.

## improvements
classic.pedia.entries-world.improvement-regular = Improvement
classic.pedia.entries-world.improvement-great-person = Great person improvement
classic.pedia.entries-world.improvement-unique = Unique improvement
classic.pedia.entries-world.improvement-barbarian-camp = Barbarian encampment
classic.pedia.entries-world.improvement-ancient-ruins = Ancient ruins
classic.pedia.entries-world.rule-anybodys-land = May be built on anybody's land
classic.pedia.entries-world.rule-city-state-land = Only on a city-state's land
classic.pedia.entries-world.rule-replaces-improvement = Replaces another improvement
classic.pedia.entries-world.rule-builder-chooses = When done, its builder chooses what to take from the site
classic.pedia.entries-world.rule-destroyed-when-pillaged = Destroyed when pillaged
classic.pedia.entries-world.rule-removes-resource = Removes the tile's resource
classic.pedia.entries-world.culture-per-adjacent = +{ $n } Culture per adjacent { $name }
classic.pedia.entries-world.city-state-luxury-copies = { $n ->
    [one] { $n } copy of each luxury of the city-state whose land it stands on (cannot be traded)
   *[other] { $n } copies of each luxury of the city-state whose land it stands on (cannot be traded)
}
classic.pedia.entries-world.bonus-with-technology = { $text } with { $name }
classic.pedia.entries-world.bonus-with-fresh-water = { $text } with fresh water
classic.pedia.entries-world.bonus-without-fresh-water = { $text } without fresh water

## natural wonders
classic.pedia.entries-world.natural-wonder = Natural wonder
classic.pedia.entries-world.rough-ground-in-combat = Rough ground in combat.
classic.pedia.entries-world.happiness-for-finders = +1 happiness for every nation that finds it.
