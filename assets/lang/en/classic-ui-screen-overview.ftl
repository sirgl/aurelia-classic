# The advisor overviews: economy (cities), military, demographics, victory, notifications, resources, trade routes.

## tabs
classic.ui.screen-overview.tab-economy = Economy
classic.ui.screen-overview.tab-military = Military
classic.ui.screen-overview.tab-demographics = Demographics
classic.ui.screen-overview.tab-diplomacy = Diplomacy
classic.ui.screen-overview.tab-victory = Victory
classic.ui.screen-overview.tab-notifications = Notifications
classic.ui.screen-overview.tab-resources = Resources
classic.ui.screen-overview.tab-trade = Trade routes

## cities table (short column headers)
classic.ui.screen-overview.col-city = City
classic.ui.screen-overview.col-pop = Pop
classic.ui.screen-overview.col-food = Food
classic.ui.screen-overview.col-prod = Prod
classic.ui.screen-overview.col-gold = Gold
classic.ui.screen-overview.col-sci = Sci
classic.ui.screen-overview.col-cult = Cult
classic.ui.screen-overview.col-faith = Faith
classic.ui.screen-overview.col-str = Str
classic.ui.screen-overview.col-producing = Producing
classic.ui.screen-overview.sort-tip = Sort by this column; click again to reverse
classic.ui.screen-overview.no-cities = You have no cities yet. Found one with a Settler (B).
classic.ui.screen-overview.city-tip-grows = { $n ->
    [one] Grows in { $n } turn. Click to manage the city.
   *[other] Grows in { $n } turns. Click to manage the city.
}
classic.ui.screen-overview.city-tip-static = Not growing. Click to manage the city.
classic.ui.screen-overview.treasury = TREASURY
classic.ui.screen-overview.empire-per-turn = EMPIRE PER TURN
classic.ui.screen-overview.food-surplus = Food surplus
classic.ui.screen-overview.production = Production
classic.ui.screen-overview.science = Science
classic.ui.screen-overview.culture = Culture
classic.ui.screen-overview.faith = Faith

## military
classic.ui.screen-overview.col-unit = Unit
classic.ui.screen-overview.col-status = Status
classic.ui.screen-overview.col-hp = HP
classic.ui.screen-overview.col-strength = Strength
classic.ui.screen-overview.col-ranged = Ranged
classic.ui.screen-overview.col-moves = Moves
classic.ui.screen-overview.unit-summary = { $military } military · { $civilian } civilian
classic.ui.screen-overview.unit-tip = Click to select it on the map · right-click for the Encyclopedia

## demographics
classic.ui.screen-overview.col-measure = Measure
classic.ui.screen-overview.col-value = Value
classic.ui.screen-overview.col-rank = Rank
classic.ui.screen-overview.col-best = Best
classic.ui.screen-overview.col-average = Average
classic.ui.screen-overview.col-worst = Worst
classic.ui.screen-overview.demographics-note = Rank among the nations still in the game. Best, average and worst cover all of them, met or not.

## resources
classic.ui.screen-overview.col-resource = Resource
classic.ui.screen-overview.col-kind = Kind
classic.ui.screen-overview.col-connected = Connected
classic.ui.screen-overview.col-in-use = In use
classic.ui.screen-overview.col-available = Available
classic.ui.screen-overview.resources-none = No resources connected yet: improve a resource in your territory (a mine on iron, a pasture on horses) once its technology is known.
classic.ui.screen-overview.resources-note = Units and buildings that need a resource hold one copy each while they exist. Without enough, they cannot be built; units short of it fight worse.

## trade routes
classic.ui.screen-overview.active-connections = ACTIVE CONNECTIONS
classic.ui.screen-overview.no-routes = No active trade routes.
classic.ui.screen-overview.route-title = { $from } to { $to }
classic.ui.screen-overview.route-title-incoming = { $from } to { $to } · Incoming
classic.ui.screen-overview.you = You
classic.ui.screen-overview.route-line = { $kind } · { $recipient }: { $yields } · { $n ->
    [one] { $n } turn remaining
   *[other] { $n } turns remaining
}
classic.ui.screen-overview.route-line-other = { $kind } · { $recipient }: { $yields } · Other city: { $other } · { $n ->
    [one] { $n } turn remaining
   *[other] { $n } turns remaining
}
classic.ui.screen-overview.start-route = START A ROUTE
classic.ui.screen-overview.no-trade-units = No idle trade units. Train a Caravan or Cargo Ship when capacity is available.
classic.ui.screen-overview.idle-trader = Idle trade unit
classic.ui.screen-overview.trader-in = Trader in { $city }
classic.ui.screen-overview.no-legal-route = No legal route is available. Check range, city buildings and existing connections.
classic.ui.screen-overview.benefit = Benefit: { $yields }
classic.ui.screen-overview.benefit-partner = Benefit: { $yields } · Partner: { $partner }
classic.ui.screen-overview.start-route-button = Start route

## victory and notifications
classic.ui.screen-overview.victory-unavailable = Victory progress is unavailable without a running match.
classic.ui.screen-overview.open-victory = Open victory
classic.ui.screen-overview.victory-heading = Turn { $turn } · Victory conditions
classic.ui.screen-overview.victory-none = No victory conditions are enabled for this match.
classic.ui.screen-overview.no-notices = Nothing has happened yet.
classic.ui.screen-overview.notice-turn = Turn { $turn }
