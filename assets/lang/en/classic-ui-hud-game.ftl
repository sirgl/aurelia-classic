# The classic HUD of a running match: top bar, research box, rivals, notices, captured city, ruins, end of game.

## classic.ui.hud-game (top bar; the signed delta is passed already signed, e.g. "+5")
classic.ui.hud-game.top-gold = { $gold } ({ $delta }) Gold
classic.ui.hud-game.top-food = { $food } Food
classic.ui.hud-game.top-production = { $production } Production
classic.ui.hud-game.top-science = { $science } ({ $delta }) Science
classic.ui.hud-game.top-happiness = { $happiness } Happiness
classic.ui.hud-game.top-culture = { $culture } ({ $delta }) Culture
classic.ui.hud-game.top-faith = { $faith } ({ $delta }) Faith

## top bar: culture and faith toward the next step ("24/45"), the click hint and the tooltip sentences
classic.ui.hud-game.top-culture-goal = { $have }/{ $cost } Culture
classic.ui.hud-game.top-faith-goal = { $have }/{ $cost } Faith
classic.ui.hud-game.click-to-open = Click to open: { $screen }
classic.ui.hud-game.goal-research-ready = A technology can be chosen now: nothing is being researched
classic.ui.hud-game.goal-policy-ready = A policy can be adopted now
classic.ui.hud-game.goal-policy-turns = { $missing } more culture for the next policy: { $turns ->
    [one] 1 turn
   *[other] { $turns } turns
} at +{ $rate } per turn
classic.ui.hud-game.goal-policy-stalled = { $missing } more culture for the next policy: — (no culture income)
classic.ui.hud-game.goal-pantheon-ready = A pantheon can be founded now
classic.ui.hud-game.goal-pantheon-turns = { $missing } more faith for the pantheon: { $turns ->
    [one] 1 turn
   *[other] { $turns } turns
} at +{ $rate } per turn
classic.ui.hud-game.goal-pantheon-stalled = { $missing } more faith for the pantheon: — (no faith income)
classic.ui.hud-game.goal-reformation-ready = A Reformation belief can be added now
classic.ui.hud-game.goal-prophet-turns = { $missing } more faith for the next Great Prophet: { $turns ->
    [one] 1 turn
   *[other] { $turns } turns
} at +{ $rate } per turn
classic.ui.hud-game.goal-prophet-stalled = { $missing } more faith for the next Great Prophet: — (no faith income)

## research box
classic.ui.hud-game.open-research = Open research
classic.ui.hud-game.research-kicker = RESEARCH
classic.ui.hud-game.research-choose = Choose research
classic.ui.hud-game.research-idle = Nothing is being studied
classic.ui.hud-game.research-detail = { $stored }/{ $cost }  ·  { $n ->
    [one] { $n } turn
   *[other] { $n } turns
}
classic.ui.hud-game.research-detail-stalled = { $stored }/{ $cost }  ·  no science yet

## rivals
classic.ui.hud-game.rival-war = { $name } · War
classic.ui.hud-game.rival-peace = { $name } · Peace
classic.ui.hud-game.make-peace = Make peace
classic.ui.hud-game.open-borders = Open borders
classic.ui.hud-game.blocked-reason = { $action }: { $reason }

## notices
classic.ui.hud-game.notice-line = T{ $turn }  { $text }
classic.ui.hud-game.notice-hint = Show on the map · F7 for the full log

## captured city
classic.ui.hud-game.capture-title = { $city } is yours
classic.ui.hud-game.capture-line = Founded by { $founder }. Decide what becomes of it.
classic.ui.hud-game.unavailable = Unavailable

## ancient ruins
classic.ui.hud-game.ruins-title = Ancient Ruins
classic.ui.hud-game.ruins-line = Your unit found ruins. Choose one of three rewards.
classic.ui.hud-game.ruins-later = Later
classic.ui.hud-game.ruins-later-hint = The ruins stay on the map

## end of the game
classic.ui.hud-game.victory = Victory
classic.ui.hud-game.victory-line = Your nation has won the game.
classic.ui.hud-game.defeat = Defeat
classic.ui.hud-game.defeat-line = Your nation has lost the game.
classic.ui.hud-game.see-results = See the results

## city-state card over the map
classic.ui.hud-game.city-state-gold = Treasury: { $gold } gold
classic.ui.hud-game.city-state-close = Close (Esc)
