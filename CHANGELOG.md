# Changelog

## 0.3.0

Changes since 0.2.0 (4 November 2026), in this repository and in the [platform](https://github.com/sirgl/aurelia-platform)
it builds on. Saves made by 0.2.0 still load (the ruleset fingerprint of 0.2.0 is on the list of earlier editions).

### Rules

- **Citadel as in Unciv 4.22.10.** It can be built on any land tile next to your own land, another player's too. It takes
  over the tiles around it (radius 1, its own tile included, city centres excepted) for the nearest own city that touches
  them, and only a tile inside that city's culture radius; a city-state loses influence for each tile; units and camps on
  a taken tile are dealt with, and a camp on the site goes before the citadel is placed. A site no city of yours reaches is
  refused, with the reason told in every language. A pillaged citadel does not hurt.
- **Gold prices as in Unciv.** The percents of a gold purchase multiply, one factor per source, with no upper bound on the
  number of sources, and the price is rounded once. A unit's purchase modifier (`hurryCostModifier`) prices its purchase and
  its disband gold. The Mandate of Heaven discount applies to every item, and faith uses the same one price calculation. A
  tile costs the speed percent and the factors in one product, rounded to the nearest gold. A puppet, a razing city and a
  city in resistance buy no tile.
- **Disband a unit, sell a building**, as in Unciv, each after a confirmation. A sale removes the queued orders that needed
  the building, and the city keeps its hit points up to the new maximum.
- **Rebels** rise below -20 happiness, as in Unciv (only in a game with barbarians): a countdown, then rebels near one of the
  nation's cities, with a notice and a warning in the happiness tooltip.
- **A city in resistance gives nothing**: it runs no turn step and takes no order that builds or buys. The city screen says
  why production and buying are blocked.
- **Lost orders** leave the queue by themselves at no cost when the building they needed is gone, and the owner is told. The
  requirement "a building in every city" ignores a captured city that awaits its fate, counts a unique building for the one
  it replaces, and a city founded later drops the national wonder that wanted it.
- **Diplomacy.** Peace with a nation needs its consent and is made as a deal (with a city-state either side can make it
  alone); a refused peace says why and what it would cost. An unpaid gold per turn breaks the deal, counts against the
  debtor like a declaration of war, and is told to both sides. A new gold-per-turn item must fit the payer's income. War and
  denunciation wait for the player's yes.
- **Congress.** Every delegate is cast: the remainder of an even split over several resolutions was lost, and beside another
  proposal the World Leader vote was halved. The player is warned three turns before the session that elects the World Leader.
- **New games** start from balanced positions. The Shoshone start with a Pathfinder that sees
  3 tiles, and a nation does not build the unit its unique unit replaces.
- **Saves.** The ruleset fingerprint and the command layout moved; earlier editions keep loading.

### Computer opponents

- **The standard AI is the former bench player `next-x`** (the fourteenth promotion, 14 November). On top of `next-w`
  (below) it plays the war of `next-war-retake` (ranged units first and a press call at the walls, a muster before
  the war, a retake of a lost city when it is worth the units), the vote guard (it denies a rival the World Leader vote,
  and values an alliance by what it adds to its own chance) and a plan for a player left with no city. Against the previous
  standard (`pre-next-x`, still in the bench) at Quick speed on the pace suite it holds about 58 % of the victory-weighted
  share (59.4 % when first built, 56.6-58.8 % in four control blocks on the merged code), and wins more real victories
  (228 against 134 in the four blocks of the first build).
- **`next-w`** (the thirteenth promotion, 11 November): the strategy net chooses sharply and sees more; the tall style
  keeps the cities the rules make worth it and plays for the science victory; a wonder another nation has built is dropped
  from the queue, and a wonder race is priced by the chance to finish first.
- The AI makes peace by deals, prices cities and resources by worth, and casts all its delegates in a session.
- Fixes: no division by zero when no rival capital is left to march on, and a unit of a kind the rules do not list no longer
  keeps a landless player out of the plan.
- The bench gained the tools that measured this (the tactics exam `ai-exam`, the siege bank, the war probe, counters for
  deals, wars, rebels and purchases, and the victory-weighted share with its interval).

### Interface

- **End turn waits for your orders.** One list of what you still owe (free picks, empty production, a captured city,
  research, policy, pantheon, promotion, great person, a unit order); the button names the first item and leads to it. A
  city in resistance is not asked for production.
- **Notices** for a war declared on you, lapsed offers, starving cities, rival victory steps, answers to peace offers, broken
  deals, orders removed by the rules and rebels. They name only the nations you have met, read right with a plural nation
  name, and do not tell a city you razed as starved.
- **Colours of the powers.** Every owner of a match gets a distinct colour (a nation whose colour is close to an earlier
  one takes the first spare colour far enough away); the renderer paints up to 64 owners and the new-game screen offers up
  to 41 (16 powers, 24 city-states, the barbarians).
- **The technology tree** opens on the technology in research; without research, on the farthest available one. Coming
  back from the Encyclopedia keeps the scroll.
- **Keys of the screens** are shown in the tooltips of the top bar.
- **Cards that ask for a yes**: war, denunciation, disband and sale; Esc is a no, and an old yes declares nothing. The peace
  button offers peace to a nation.
- Every new text is in all seven languages.
- **The version of the game** and, when the build knows it, the short commit are shown in the main menu and on the settings
  screen (also in the browser and on Android).

### Builds and tests

- Launchers read their version and commit through one registration (`client::register_build!`); the three launcher crates
  share one version, and the Android version code follows it.
- The screen snapshots are recorded with the art pack, and an update without it is refused.
