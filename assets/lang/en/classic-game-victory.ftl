# Game layer: victory progress, the end of the game, the Hall of Fame line (game/victory.rs).

classic.game.victory.row-domination = Domination
classic.game.victory.row-science = Science
classic.game.victory.row-culture = Culture
classic.game.victory.row-diplomacy = Diplomacy
classic.game.victory.row-time = Time
classic.game.victory.goal-domination = Hold every original capital, including a foreign capital
classic.game.victory.goal-science = Build the Apollo Program and install every spaceship part
classic.game.victory.goal-culture = Be influential on every other nation
classic.game.victory.goal-diplomacy = Win the World Leader vote of the United Nations
classic.game.victory.goal-time = Have the highest score on turn { $last }
classic.game.victory.progress-domination-lone = { $held } of { $total ->
    [one] { $total } founded capital
   *[other] { $total } founded capitals
} held · No foreign capital captured
classic.game.victory.progress-domination = { $held } of { $total ->
    [one] { $total } original capital
   *[other] { $total } original capitals
} held
classic.game.victory.progress-science-built = Apollo Program built  ·  { $have } of { $need } spaceship parts
classic.game.victory.progress-science = Apollo Program not built  ·  { $have } of { $need } spaceship parts
classic.game.victory.progress-culture = Influential on { $have } of { $need ->
    [one] { $need } nation
   *[other] { $need } nations
}
classic.game.victory.progress-diplomacy-leader = World Leader: { $name }
classic.game.victory.progress-diplomacy-no-congress = The World Congress is not founded yet
classic.game.victory.progress-diplomacy-no-un = { $have } of { $need ->
    [one] { $need } delegate
   *[other] { $need } delegates
}  ·  the United Nations is not founded yet
classic.game.victory.progress-diplomacy = { $have } of { $need ->
    [one] { $need } delegate
   *[other] { $need } delegates
}
classic.game.victory.progress-time = Turn { $turn } of { $last }
classic.game.victory.title-victory = Victory
classic.game.victory.title-defeat = Defeat
classic.game.victory.end-fallen = Your nation has fallen: no city and no unit is left.
classic.game.victory.end-won = You won a { $victory } on turn { $turn }.
classic.game.victory.end-lost = { $winner } won a { $victory } on turn { $turn }.
classic.game.victory.hall-defeat = Defeat ({ $winner }: { $victory })
classic.game.victory.hall-eliminated = Eliminated
classic.game.victory.hall-line = { $nation } · { $result } · turn { $turn } · score { $score }
classic.game.victory.one-more-turn = One more turn
classic.game.victory.kind-time = Time victory
classic.game.victory.kind-science = Science victory
classic.game.victory.kind-domination = Domination victory
classic.game.victory.kind-cultural = Cultural victory
classic.game.victory.kind-diplomatic = Diplomatic victory
