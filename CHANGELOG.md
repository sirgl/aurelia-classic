# Changelog

## 0.4.0

What changed since 0.3.0 (15 November 2026). Saves made by 0.3.0 still load and play on with the new computer players.

Highlights: **computer opponents that plan wars, peace and the race to the spaceship as one, and win by science much
more often**, **a computer turn that takes less time**, **a new policies screen**, and **rules fixes** (one capital per
nation, nothing left behind by a destroyed nation, offers that wait for your answer).

### Computer opponents

The default computer player was replaced three times since 0.3.0, each time after test games against the one before it
(Quick speed, 400 to 800 games each; a real victory counts in full, a win on the turn limit as a quarter of the points).

- **Fewer, larger cities, and sites chosen with care.** The computer players prefer a few large cities and spread wide only
  when they have a strong reason. They pick city sites farther from rivals' borders and from enemy armies, and they do not
  send a settler on a long, unprotected walk. They win more often by science and by diplomacy, and their cities rise in
  revolt less often. Against the 0.3.0 player the new one took about 63 % of the points and won 238 games by a real
  victory against 113.
- **One mind for the big decisions.** They decide their wars, peace, deals, gifts to city-states, Congress votes and the
  defence of their cities together, from one reading of every rival: who is weak and within reach, who is massing an army
  at the border, who is about to win. They go to war for a named target and build the army for it first, try to retake
  what they lost, guard the cities a rival threatens (a city in danger now builds a defender at once), deny help and
  trade to a nation that is running away with the game, and keep their research on the way to the spaceship once they aim
  for a science victory. Against the player before it, it took about 66 % of the points and won 342 games by a real
  victory against 136.
- **A faster start and a faster race to space (the default now).** The computer players open with more cities, keep a
  garrison in a young city near a rival, buy growth, and, when they race for the spaceship, hire scientists and make
  research agreements. In 400 games against the player before it, they took about 62 % of the points and won 211 games by a
  real victory against 126; **197 of those wins were by science, against 91 for the older player.** Their science is about 10
  to 13 % higher from the middle of the game; their treasury and their happiness are leaner, the cost of a bigger empire.
- **Beliefs and ideology** are chosen by what they actually do for the nation, not by name.
- An army at the border stays a threat once the war starts: a computer player that saw a neighbour's army gather by its
  cities raised its guard, and dropped it on the turn that army declared war. It now keeps answering that army through
  the war.
- They keep the capital for the spaceship: a finished part no longer waits outside its capital behind the player's own
  workers, settlers and caravans, which step aside, and caravans go to another city.
- They were tried against scripted human tricks (a wonder sniped at the last turn, Congress votes bought by gifts, a
  city-state taken away from its ally, an army that gathers just out of the cities' reach and then closes in) and fixed
  where they failed.
- They make peace through deals, drop a war that no longer pays, and a seat in Congress that abstains on everything casts
  no vote.
- A loaded game now plays on exactly as it would have without the save: the computer players keep no memory outside it.
- **A computer turn takes less time.** Two rounds of speed-ups, each with the same moves as before and about 15 %
  less processor time. The new default player thinks about 17 % longer per turn than
  the one it replaced, because its empires are bigger.

### Rules

- **One capital per nation.** A city you buy from a city-state, receive in a deal or liberate no longer brings its old
  palace, so a nation can no longer end up with several capitals (the game took the first one, which could be a bought
  city overseas, and the spaceship parts could not be installed). When the capital itself is lost, the palace goes to the
  largest remaining city that is not a puppet. Saves with several palaces (or none) are repaired when they load.
- **A destroyed nation leaves nothing behind.** When a nation or city-state loses its last city, every tie with it ends at
  once: wars, treaties, deals and gold per turn, offers, embassies, open borders, research agreements, its spies, its votes
  and seats in the World Congress, and the World Leader title. A city-state that was its ally picks its next ally at once.
- A holy city can no longer be razed or destroyed by a nuke; a burning city that becomes a holy city keeps its last
  citizen, and a burning city that becomes the capital stops burning. A razed city leaves no record behind.
- **World Congress:** when the host is destroyed, the league passes to the living nation with the most delegates; a
  destroyed nation can no longer win a vote.
- **Nations remember wrongs.** Taking a neighbour's land with a citadel costs its opinion of you (10 per tile). Declaring
  war on a nation you declared friendship with, on a defensive pact partner or on your own allied city-state angers the
  victim and every nation that knows you. These grudges fade over time and are shown on the diplomacy screen.
- **Offers wait for your answer.** An offer to you now stays in the diplomacy inbox for up to ten turns, and you are told
  when one lapses and why: it ran out, a war began, the other nation is gone, or its terms can no longer be met. The
  computer players no longer repeat an offer that is still waiting for you.
- A unit that may attack more than once a turn keeps its remaining moves after an attack while it has attacks left; an
  air sweep counts as an attack.
- **Spies:** when you notice a theft from your city you are told, and by whom when your people find out; when a spy is
  caught, both nations are told.
- New notices: a Congress proposal dropped from the agenda, the first meeting with a nation and the gold it brings,
  city-state quests given and done, a world wonder built, a nation destroyed.

### Interface

- **A new policies screen.** A list shows everything you can adopt this turn, with its effect and one button that names
  the price, and says when the next policy will cost more. Every branch shows what it is for and its progress, every policy
  its effect and why it is locked. Arrow keys and Enter adopt without the mouse; a short pause after each adoption stops a
  double click from buying the next card. On a phone the screen has two tabs, and a tap shows the whole text of a card.
  The keys of the screen are named in its tooltips.
- Sharper text on phones on every screen: thin letters and the "..." of a cut line no longer fade.
- Technologies you cannot research yet look locked and say what they need; the ones you can start now are marked.
- A new or loaded game opens with the map zoomed to the land you know.
- The panel of met nations lists nations only (city-states are one row that opens their screen), with the reasons in
  tooltips; notices stay inside their panel.
- An error during play, such as a failed autosave, shows as a message over the game instead of on the main menu.
- The End turn button also reminds you of Congress votes to cast, offers waiting for an answer and spies with no task.
- The arrow symbol is no longer missing from texts in any language, and the new Japanese and Chinese lines use characters
  the shipped fonts have.

### Builds and tests

- The tests run in two tiers: a fast one for every change (about half a minute on four threads) and the full one for a
  release. A test loads saves made by 0.3.0 and plays them on.

## 0.3.0

What changed since 0.2.0 (4 November 2026). Saves made by 0.2.0 still load.

Highlights: **much stronger computer opponents**, **a game version shown in the menu**, an **End turn button that tells
you what still needs an order**, **notices for what happens to you**, and rules that now follow Unciv more closely
(citadels, gold prices, diplomacy, Congress).

### Computer opponents

- **A new default AI.** It fights a smarter war: it sends ranged units first and brings siege units up to the walls,
  gathers its army before it declares war, and tries to win back a city it lost when that is worth the units. It also
  stops a rival from winning the World Leader vote, judges an alliance by what it adds to its own chances, and keeps
  playing a plan when it has lost every city.
- **How much stronger.** In 800 test games at Quick speed against the previous default AI, the new one takes about 58 %
  of the points (counting a win by a real victory in full and a win on the turn limit as a quarter) and wins 204 games by
  a real victory against 128.
- The AI makes peace through deals, prices cities and resources by what they are worth, builds the tall-empire style
  when it pays, drops a wonder another nation has already built, and always casts all its delegates in a Congress session.
- Fixes: the AI no longer breaks when no rival capital is left to march on, and a nation that lost every city is no longer
  stuck with no plan.

### Rules

- **Citadels work as in Unciv.** You can build one on land next to your territory, even next to a neighbour's. It claims
  the tiles around it for your nearest city, pushes other units off them, takes influence from a city-state for each tile
  it takes, and removes a barbarian camp on its site. If none of your cities can reach the site, the game says so.
- **Gold prices work as in Unciv.** Discounts and surcharges from several sources now multiply, and the price is rounded
  once. A puppet city, a city being razed and a city in resistance cannot buy a tile.
- **Disband a unit and sell a building**, each after a confirmation.
- **Rebels** appear when happiness falls below -20 (only in a game with barbarians). You get a notice, and the happiness
  tooltip warns you beforehand.
- **A city in resistance produces nothing.** The city screen says why production and buying are blocked.
- **Diplomacy.** Peace needs the other side's consent and is made as a deal. A refused peace tells you why and what it
  would cost. A gold-per-turn deal that is not paid breaks the deal. War and denouncing a nation ask you to confirm first.
- **Congress.** Every delegate is now counted, and you are warned three turns before the session that elects the World
  Leader.
- **New games** start from more balanced positions. The Shoshone start with a Pathfinder that sees 3 tiles.

### Interface

- **End turn tells you what is left to do.** One list of everything that still needs an order (free picks, empty
  production, a captured city, research, a policy, a pantheon, a promotion, a great person, a unit). The button names the
  first item and takes you to it.
- **Notices** for a war declared on you, an offer that ran out, a starving city, a rival close to winning, an answer to
  your peace offer, a broken deal, and rebels. They name only the nations you have met.
- **Nation colours** no longer repeat in a game with many players, including a full-size map.
- **The technology tree** opens on the technology you are researching.
- **Keys** for the screens are shown in the tooltips of the top bar.
- **Yes/no cards** for war, denouncing, disbanding and selling. Esc means no.
- **The game version** (and, when known, the build) is shown at the bottom right of the main menu and the settings screen,
  in the browser and on Android too.
- Every new text is available in all seven languages.

### Builds and tests

- Launchers read their version and commit through one registration (`client::register_build!`); the three launcher crates
  share one version, and the Android version code follows it.
- The screen snapshots are recorded with the art pack, and an update without it is refused.
