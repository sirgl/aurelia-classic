# Own design of the expansion-only systems

Unciv (the only external source of this project) has no rule set for trade units, cultural influence, great works,
ideologies, the World Congress or archaeology. For these systems the functions and numbers are our own: the same kind
of play (a player who knows such a game recognises the loop), but our own shapes and our own constants. This file is
the design record; [BNW-AUDIT.md](BNW-AUDIT.md) lists every rule and constant and where it comes from; the generated
[rules.md](rules.md) lists the constants with their source line; the data rows live in
`classic/tools/own-design.json` (applied to the catalogs by `classic/tools/apply-own-design.py`).

How the design was made, for every system:

1. The loop is kept (what the player does and sees). The functions and numbers are written new, from the economy
   around them, not from any other game.
2. Shapes are smooth: a few named constants per system, caps where a runaway is possible, per-era growth that is
   linear or a square root, never a table copied from somewhere.
3. Every system has a reference economy (a developed city of each era: gold, food) taken from the headless AI census
   (`classic/crates/classic-rules/examples/expansion_census.rs`, 4 seeds x 500 rounds, per-city mean by era) and
   extended with the same growth for the eras the AI games do not reach. A unit test per system checks that the new
   numbers stay inside a playable share of that economy.
4. Tuning of a system is its block of named constants (top of the file, doc comment each) plus the rows of
   `own-design.json`. Nothing is tuned by a literal inside a function.
5. Source of every number: "own design" (this file explains the reasoning) or "Unciv <file>" where Unciv has the rule.

## Reference economy

Mean per city of the AI census (not a human player's: the AI cities are small, so these numbers are a floor):

| Era index | cities | pop | gold | science | production | food | culture |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 115 | 3.0 | 2.3 | 4.5 | 5.8 | 8.3 | 2.3 |
| 2 | 307 | 2.7 | 2.1 | 4.5 | 5.7 | 8.0 | 2.2 |
| 3 | 486 | 2.8 | 4.2 | 4.8 | 6.8 | 7.7 | 2.2 |
| 4 | 592 | 2.8 | 7.5 | 5.0 | 9.1 | 7.6 | 2.6 |
| 5 | 636 | 3.2 | 11.6 | 7.0 | 9.1 | 8.7 | 3.5 |

The unit tests use a developed city of 3 / 5 / 8 / 12 / 18 / 26 / 35 / 46 gold for the eight eras (the census values
extended with the growth of the last rows).

## 1. Trade routes (`derived/trade.rs`, `derived/trade_path.rs`, `systems/trade.rs`)

Loop kept: a trade unit (Caravan on land, Cargo Ship at sea) is spent to start a route from its city to a destination
in range; it walks the path back and forth for a number of circuits; both ends are paid every turn; the unit comes back
to its city when the route ends; war, capture and plunder end a route. International routes pay gold and science,
internal routes feed food or production to an own city.

New in our design (formulas in the module header, constants at the top of `derived/trade.rs`):

| What | Function | Why this shape |
|---|---|---|
| Route slots | technology and wonder slots (data) x the player's percent, plus one per 4 cities; none at all while no technology gives a slot | an empire that has grown fields more traders; a nation that cannot build a trade unit yet gets nothing from size |
| Range | land 12 / sea 18 tiles, plus one tile per two eras, plus effects, then the city's percent | traders reach further as the eras pass; effects stay additive |
| Path cost | plain 100, rough ground 115, road or rail 60; sea: coast 100, open water 120, +500 in enemy water; valid while cost <= range x 100 | rough ground and open water slow a trader without deciding the route; roads pay |
| Duration | circuits = round(24 x speed / (2 L)) between 2 and 8; speed 3 land, 5 sea | the route lasts about 24 turns whatever its length; neither a 3-turn churn nor an 80-turn lock |
| International gold, sender | (80 + 25 era + 18 % of sqrt(g1 g2) + 25 per differing luxury/strategic resource (<= 5) + effect gold) x (100 + 3 min(L, 15)) % x 100 (land) or 150 (sea) % | base grows with the era; the geometric mean rewards a rich pair but a single huge city adds only its square root; a far partner pays up to 45 % more; the sea pays half as much again because it costs more and needs two coasts |
| International gold, recipient | (25 + 10 era + half the city term + effect gold) x the same factor | receiving pays about a third of sending, so sending is the better deal and receiving is never worthless |
| Science | 0.3 per technology the partner knows that the receiver lacks, at most 4.5 | knowledge flows towards the backward partner; capped so a tech leader does not feed the laggard without limit |
| Internal route | (200 + 45 era + 15 per citizen of the destination) x (100 + 60 at sea + effects) % | a route feeds a growing city a visible share of its own yield (10 - 60 % of a reference city's food) |
| Plunder | (60 + 10 era of the route's owner) gold, x1.5 at sea | what the route carried grows with the era |

Data rows (`own-design.json`, system "trade routes"): route slots come from Animal Husbandry, Sailing, Currency, Guilds,
Economics, Industrialization, Railroad and Computers (one each, one per era from the Classical on after the two
starting technologies), range percent from Compass, Refrigeration and Combustion (+15 %), Caravansary (+40 % land
range, +1 gold on land routes) and Harbor (+40 % sea range); policy route gold: Wagon Trains +1, Merchant Confederacy +1,
Treasure Fleets +2, Economic Union +2, Iron Curtain +40 % internal. Rule for the flat bonuses: +1 for an ordinary
building, +2 for a wonder, a policy about half of the base route of its era.

Sanity checks (tests in `derived/trade.rs`): a land route is 15 - 70 % of the sending city's gold in every era (66 % in
the first era where the base dominates, 30 % in the last), a sea route is at most 1.6 times a land route, the route gold
grows with the era and stays below the city's whole income, internal routes bring 10 - 60 % of a reference city's food,
circuits fit the target length for every route length 1 - 30.

AI check (`expansion_census`, 24 seeds, 4 players on 40 x 28, 150 rounds; measured by me): rounds with a route running
372 before the redesign, 175 after (the AI values a trade unit by the route's yield, and our early yields are lower
than the previous ones).

## 2. Tourism, cultural influence and public opinion (`derived/tourism.rs`, `systems/tourism.rs`)

Loop kept: cities make tourism from great works, theming, landmarks and flat sources; the tourism of a nation is turned
into influence on every nation it has met; influence levels give small rewards; being Embraced on every other living
nation wins the culture victory; foreign influence presses on the people's opinion of their ideology.

| What | Function | Why this shape |
|---|---|---|
| Value of a work | culture 1 + era/3, tourism 2 + era/2 (era of the work) | a later work is worth a little more; tourism always above culture |
| Ties (religion, open borders, trade route) | percent by number of ties: 0 / 12 / 21 / 27 | each tie helps less than the one before; no tripling |
| Ideology friction | -25 %, a diplomat in the capital +15 % | the diplomat softens the penalty, never removes it |
| Influence stock | stock' = stock - 2 % + tourism x (100 + ties %) | what a nation stops spreading is forgotten (half-life 34 turns), so levels follow the present effort instead of piling up for ever |
| Level | stock per mille of the target's culture of its last 8 turns: (825 l^2 + 675 l) / 2 = 750 / 2325 / 4725 / 7950 / 12000 | a smooth quadratic; in steady state the levels ask for tourism of 0.12 / 0.37 / 0.76 / 1.27 / 1.92 times the target's culture per turn |
| Concert | musician's `one_shot` (7) turns of tourism, never less than one level of the audience; 12 % of it reaches the others | a musician always makes the audience Noticed; a strong culture turns the visit into a real step |
| Trend | the stock against the stock before the last turn | the stock already forgets, so rising means the effort beat the forgetting |
| Opinion | pressure of a nation = how much further its influence on us reaches than ours on it (points of 1000 per mille, at most 4 per nation); dissatisfaction = other ideologies minus own, rounded up; steps 1 / 3 / 6 name Murmurs, Unrest, Upheaval| continuous inputs, three named steps |
| Opinion costs happiness (was not wired) | unhappy citizens = (3 + 3 d) % of the population, at most 30 % | visible, never crushing; reference empire of 60 citizens: 4 / 8 / 13 / 18 unhappy at 1 / 3 / 6 / 9 points |
| Switching ideology | lose a quarter of the tenets (at least one), the culture pool burns | |
| Influence helps trade and conquest | route science 0.5 per level from Admired; a conquered city keeps 10 % more of its citizens per level from Admired (was not wired for conquest) | |

Data rows: Hotel and Airport +40 % tourism from works and landmarks, National Visitor Center +80 %, Media Culture +25 %,
The Internet +80 %, Cultural Exchange +12 % per tie, Futurism 200 %, Religious Art 4, Eiffel Tower 10 flat tourism, theming
bonuses +60 % (Aesthetics), the Great Musician's `one_shot_tourism` 7 and 12 % for the others, guild points 2 each.

Sanity checks (tests in `derived/tourism.rs`): values per era, thresholds widen at each level, tourism needed to hold each
level on a target with 40 culture a turn (5 / 15 / 31 / 51 / 77), half-life of the forgetting 33 - 36 turns, steady
stock = 50 x tourism, unhappiness table, opinion steps.

## 3. Great works and theming (`derived/tourism.rs`, data)

Slots per building are one per genre building (Amphitheater, Opera House, Broadcast Tower, Cathedral, Palace), two for
the Museum, and for wonders by era: ancient and classical 1, medieval and renaissance 2, industrial and modern 3 -
atomic and information 4 (Great Library 1, Hermitage and Uffizi 2, Louvre 3, Sydney Opera House 4). Theming is one
scheme for every building with two or more slots, with `n` slots: "diverse" (every era and every author different) n + 1,
"of one age" n, "of one hand" max(1, n - 1); the Hermitage asks for art only. The best matching theme counts; the bonus
adds to culture and tourism alike.

## 4. Ideologies (`derived/culture.rs`, `own-design.json` system "ideologies")

Available from the Industrial era or when two Factories stand (was three); the first adopter of an ideology gets three
free tenets, the second one, later ones none; a tier opens when the player holds three tenets of the tier below. The 48
tenets keep their names and roles; their numbers follow a budget per tier (tier 1 about +1 yield or 15 - 25 %, tier 2 about
+2 yields or 25 - 35 %, tier 3 about +3 yields or 30 - 40 %): purchase discounts -25 %, great person rate +20 %, espionage
rates +75 %, defence +25 %, golden age +35 %, building happiness +1 (tier 1) / +2 (tier 2), production of a building class
+60 / +75 %, military production +15 / +20 %, tourism toward same ideology +20 %, less happy +30 %, common enemy +35 %,
Party Leadership +1 production and science (was five yields), Five-Year Plan +1 production, Resettlement +2 citizens,
Third Alternative +50 % strategic resources, Universal Healthcare six wonders, Volunteer Army five free upkeep slots and
four units. All rows are in `own-design.json`.

## 5. World Congress (`systems/congress.rs`, `systems/league_projects.rs`)

Founded when the first nation with the founding technology has met at least half of the other civilizations (was:
all). Three special sessions by game era: Assembly (Renaissance; every nation 2 delegates, host +1, every 28 rounds),
Council (Industrial; 3, host +2, an allied city-state +1, every 21 rounds), United Nations (Atomic; 4, host +2,
city-state +1, every 14 rounds, World Leader vote). Three proposers (host and the two members with the most delegates).
A failed World Leader vote gives each of the two leading candidates one delegate for good. The delegates needed for
the World Leader use Unciv's formula (`VictoryManager.kt`), unchanged.

Resolutions, all ten now do something: Standing Army Tax +20 % unit upkeep; Scholars in Residence +12 % science on a
technology another civilization knows; Cultural Heritage Sites +2 culture per world wonder; Nuclear Non-Proliferation
bans nuclear weapons; City-State Embargo and Player Embargo forbid international routes and end the running ones (new);
Ban Luxury strips the chosen luxury of its happiness for everybody (new); Arts Funding and Sciences Funding move the
great person points of the arts (writers, artists, musicians) and of the industries (scientists, engineers, merchants) by
25 % in opposite directions (new); International Project starts a project.

Projects: cost per nation 150 x (era index of the project - 1): 300 (World's Fair, Renaissance), 600 (International
Games, Modern), 900 (Space Station, Information); tiers at 40 %, 90 % of the share and the best contribution; rewards World's Fair 360 golden age points /
a free policy / 15 turns of +75 % culture; Games happiness 2 / happiness 2 and 25 influence with every city-state met /
a free policy and 15 turns of +75 % tourism; Station 5 turns of research / a Great Scientist / the Station wonder.

## 6. Archaeology (`derived/archaeology.rs`, `systems/archaeology.rs`)

About five sites per nation (4 + two draws), 25 % hidden until a policy reveals them, 40 % of the hidden ones hold a
writing, at most ten writings. Site weight: 6 plus 5 per rank of the tile's history plus 2 per era back from the seventh,
plus a noise of 0 - 9, then 150 % on desert and jungle, 85 % inside borders, 60 % when improved or on a road, 10 % under a
city; a chosen site divides the weight around it by `5 - distance` within 4 tiles. A dig takes the work time of the dig
improvement; the choices after a dig are unchanged.

## 7. Spies and diplomats, city-state quests, great people, religion

Unciv has these in other forms and the constants are Unciv's (listed with their file in the audit). Our own parts: the
diplomat's introduction takes 4 rounds; an extra spy for each nation entering the Industrial era or later (Unciv grants a
spy at the Renaissance only); Writers', Artists' and Musicians' Guilds give 2 great person points each; trade units cost
what a Worker costs (Caravan 70, Cargo Ship 90), the Archaeologist 175; the Hotel costs 500, the median of its era.
