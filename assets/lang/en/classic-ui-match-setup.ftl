# The new game setup screen of the Civ V module: nation roster, nation card, world form, loading screen.

## Frame
classic.ui.match-setup.roster-heading = NATIONS
classic.ui.match-setup.tab-world = World
classic.ui.match-setup.tab-rules = Rules & seed
classic.ui.match-setup.footer-hint = Same settings, same world. Right-click a nation for its Encyclopedia page.
classic.ui.match-setup.start = Start game
classic.ui.match-setup.world-title = Your world
classic.ui.match-setup.world-hint = Choose a nation, then shape its world.
classic.ui.match-setup.rules-hint = Keep the seed to recreate this world. Unavailable speeds and victory options show why on hover.
classic.ui.match-setup.randomize = Randomize
classic.ui.match-setup.more = More
classic.ui.match-setup.fewer = Fewer

## Section headings
classic.ui.match-setup.section-map-type = MAP TYPE
classic.ui.match-setup.section-map-size = MAP SIZE
classic.ui.match-setup.section-difficulty = DIFFICULTY
classic.ui.match-setup.section-speed = GAME SPEED
classic.ui.match-setup.section-rivals = RIVALS
classic.ui.match-setup.section-rival-nations = RIVAL NATIONS
classic.ui.match-setup.section-city-states = CITY-STATES
classic.ui.match-setup.section-sea-level = SEA LEVEL
classic.ui.match-setup.section-victories = VICTORY CONDITIONS
classic.ui.match-setup.section-seed = WORLD SEED

## Map types
classic.ui.match-setup.map-continents = Continents
classic.ui.match-setup.map-pangaea = Pangaea
classic.ui.match-setup.map-archipelago = Archipelago
classic.ui.match-setup.map-fractal = Fractal
classic.ui.match-setup.map-small-continents = Small Cont.
classic.ui.match-setup.map-hint-continents = Two large land masses divided by ocean.
classic.ui.match-setup.map-hint-pangaea = One super-continent; every nation can walk to every other.
classic.ui.match-setup.map-hint-archipelago = Many islands. Land units embark after { $coast } (coast) and { $deep } (deep water).
classic.ui.match-setup.map-hint-fractal = Irregular land and water shaped by noise alone.
classic.ui.match-setup.map-hint-small-continents = Several medium continents.

## Map sizes, sea level, speeds
classic.ui.match-setup.size-duel = Duel
classic.ui.match-setup.size-tiny = Tiny
classic.ui.match-setup.size-small = Small
classic.ui.match-setup.size-standard = Standard
classic.ui.match-setup.size-large = Large
classic.ui.match-setup.size-huge = Huge
classic.ui.match-setup.size-custom = Custom
classic.ui.match-setup.sea-low = Low
classic.ui.match-setup.sea-medium = Medium
classic.ui.match-setup.sea-high = High
classic.ui.match-setup.speed-quick = Quick
classic.ui.match-setup.speed-standard = Standard
classic.ui.match-setup.speed-epic = Epic
classic.ui.match-setup.speed-marathon = Marathon
classic.ui.match-setup.speed-unavailable = The rules core has no speed scaling yet: Standard only.
classic.ui.match-setup.city-states-hint = City-states on the map: gifts, quests, alliances and their votes in the World Congress.

## Victory conditions
classic.ui.match-setup.victory-domination = Domination
classic.ui.match-setup.victory-science = Science
classic.ui.match-setup.victory-culture = Cultural
classic.ui.match-setup.victory-diplomacy = Diplomatic
classic.ui.match-setup.victory-time = Time
classic.ui.match-setup.victory-domination-tip = Hold every original capital. Always on in this build.
classic.ui.match-setup.victory-science-tip = Build the { $project } and every spaceship part.
classic.ui.match-setup.victory-culture-tip = Become influential over every nation.
classic.ui.match-setup.victory-diplomacy-tip = Win the World Leader vote of the United Nations.
classic.ui.match-setup.victory-time-tip = Highest score on the last turn.

## Seed
classic.ui.match-setup.seed-tip = A number from 0 to 18446744073709551615. Keep it to recreate this world.

## Nation card
classic.ui.match-setup.ability-heading = Unique ability
classic.ui.match-setup.ability-inactive = { $line } (not applied yet)
classic.ui.match-setup.unique-unit = UNIQUE UNIT: { $name }
classic.ui.match-setup.unique-unit-replaces = UNIQUE UNIT: { $name } · replaces { $old }
classic.ui.match-setup.unique-building = UNIQUE BUILDING: { $name }
classic.ui.match-setup.unique-building-replaces = UNIQUE BUILDING: { $name } · replaces { $old }
classic.ui.match-setup.unique-improvement = UNIQUE IMPROVEMENT: { $name }
classic.ui.match-setup.unique-improvement-replaces = UNIQUE IMPROVEMENT: { $name } · replaces { $old }

## Loading screen
classic.ui.match-setup.summary = { $map } · { $size } ({ $width } × { $height }) · { $players ->
    [one] { $players } nation
   *[other] { $players } nations
} · { $difficulty }
classic.ui.match-setup.creating = Creating the world{ $dots }

## Errors
classic.ui.match-setup.error-nation = Choose an available nation
classic.ui.match-setup.error-world-settings = Invalid world settings
classic.ui.match-setup.error-difficulty = Choose a difficulty level
classic.ui.match-setup.error-speed = This game speed is not available yet
classic.ui.match-setup.error-create = Could not create this world: { $error }
classic.ui.match-setup.error-seed = Enter a whole-number seed from 0 to 18446744073709551615
