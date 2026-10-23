# The Civ V HUD of a running match: top bar, research box, rivals, notices, captured city, ruins, end of game.

## classic.ui.hud-game (top bar; the signed delta is passed already signed, e.g. "+5")
classic.ui.hud-game.top-gold = { $gold } ({ $delta }) Gold
classic.ui.hud-game.top-food = { $food } Food
classic.ui.hud-game.top-production = { $production } Production
classic.ui.hud-game.top-science = { $science } ({ $delta }) Science
classic.ui.hud-game.top-happiness = { $happiness } Happiness
classic.ui.hud-game.top-culture = { $culture } ({ $delta }) Culture
classic.ui.hud-game.top-faith = { $faith } ({ $delta }) Faith

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
