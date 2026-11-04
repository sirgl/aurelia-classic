"""Unit model catalog of the classic module: the class models only classic draws (one per classic unit
class, classic/docs/classic/units.json) and the unique units as class variants (classic/docs/classic/unique_units.json).
The building blocks (Model, the builders, budgets, slots, shared looks) and the class models both rules
modules draw are the platform's (platform/tools/blender/units/catalog.py, imported as `catalog`).

A variant may be based on a platform class model; `unique_models` gets every class model classic can see.
The content keys of these models ("core:unit-class/<slug>" -> "unit/<slug>") are in part.py.
Facing: people turn 20 degrees to the camera's right, long things point 35 degrees right.
"""
import civilians
import machines
import mounts
import ships
from people import Look, with_

from catalog import (
    ARCHER, CREW_ANCIENT, CREW_GUNPOWDER, CREW_MODERN, ENGINE_SCALE, Model, cavalry, engine, infantry,
    long_build, person_build, plain_build, rider_build, single, vessel)


def plane(slug, fn, *args, contact="ground", figures=1, **kwargs):
    return Model("unit/" + slug, long_build(fn, *args, **kwargs), "aircraft", "air", figures, contact)


# Looks of the classic units ---------------------------------------------------------------------------
CREW_KOREAN = Look(body="team", legs="cloth", head="wide", head_mat="black")
PIKEMAN = Look(body="team", armor="steel", legs="cloth_dark", head="kettle", head_mat="steel", right="pike")
LONGSWORD = Look(body="team", armor="steel", shoulders="steel", head="dome", head_mat="steel", right="longsword",
                 left="kite")
CROSSBOW = Look(body="team", armor="leather", legs="cloth_dark", head="kettle", head_mat="iron", right="crossbow")
GREAT_WAR = Look(body="team", legs="khaki", head="brodie", head_mat="olive", right="rifle", back="pack",
                 extra=("bayonet",))
INFANTRY = Look(body="team", legs="olive", head="stahl", head_mat="olive", right="rifle", back="pack")
MARINE = Look(body="team", legs="olive", head="modern", head_mat="olive", right="assault", back="pack")


def class_models():
    """The class models only classic draws, by slug (classic/docs/classic/units.json `class`)."""
    c = {}

    def add(slug, model):
        c[slug] = model
    add("composite-bowman", infantry("composite-bowman", with_(ARCHER, armor="leather", head="cone",
                                                                head_mat="iron")))
    add("pikeman", infantry("pikeman", PIKEMAN))
    add("landsknecht", infantry("landsknecht", Look(body="team", legs="red", head="beret", head_mat="red",
                                                    right="longsword", extra=("beard",))))
    add("longswordsman", infantry("longswordsman", LONGSWORD))
    add("crossbowman", infantry("crossbowman", CROSSBOW))
    add("great-war-infantry", infantry("great-war-infantry", GREAT_WAR))
    add("infantry", infantry("infantry", INFANTRY))
    add("marine", infantry("marine", MARINE))
    add("paratrooper", infantry("paratrooper", Look(body="team", legs="olive", head="stahl", head_mat="khaki",
                                                    right="smg", back="chute")))
    add("bazooka", infantry("bazooka", Look(body="team", legs="olive", head="modern", head_mat="olive",
                                            right="bazooka", back="pack")))
    add("xcom-squad", infantry("xcom-squad", Look(body="team", legs="dark", head="modern", head_mat="dark",
                                                  armor="dark", right="assault", back="jetpack")))
    # Crewed guns in small formations -------------------------------------------
    add("machine-gun", engine("machine-gun", machines.mg_nest, CREW_MODERN, figures=2))
    add("gatlinggun", engine("gatlinggun", machines.gatling, CREW_GUNPOWDER, figures=2))
    add("anti-tank-gun", engine("anti-tank-gun", machines.field_gun, CREW_MODERN, barrel=0.6, figures=2))
    add("anti-aircraft-gun", engine("anti-aircraft-gun", machines.aa_gun, CREW_MODERN, figures=2))
    add("chariot-archer", Model("unit/chariot-archer", long_build(mounts.chariot, CREW_ANCIENT,
                                                                  with_(ARCHER, back="")), "mounted", "military", 2))
    knight = Look(body="team", armor="steel", legs="steel", head="dome", head_mat="steel", plume="team",
                  right="lance", left="kite")
    add("knight", cavalry("knight", "barded", knight))
    lancer = Look(body="team", legs="white", head="shako", head_mat="team", plume="white", right="lance")
    add("lancer", cavalry("lancer", "horse", lancer, coat="horse_dark"))
    add("trebuchet", engine("trebuchet", machines.trebuchet, CREW_ANCIENT))
    add("rocket-artillery", engine("rocket-artillery", machines.rocket_truck, CREW_MODERN))
    # Armor and vehicles --------------------------------------------------------------
    add("wwi-tank", engine("wwi-tank", machines.landship, figures=2, budget="vehicle"))
    add("modern-armor", engine("modern-armor", machines.tank, color="khaki", turret="angular", barrel=0.7,
                               length=0.6, width=0.24, figures=3, budget="vehicle"))
    add("mobile-sam", engine("mobile-sam", machines.mobile_sam, figures=2, budget="vehicle"))
    add("mech", engine("mech", machines.robot, budget="siege"))
    add("helicopter-gunship", plane("helicopter-gunship", machines.helicopter, contact="hover", figures=2))
    # Aircraft and missiles ---------------------------------------------------------------
    add("triplane", plane("triplane", machines.triplane))
    add("wwi-bomber", plane("wwi-bomber", machines.biplane_bomber))
    add("fighter", plane("fighter", machines.prop_fighter))
    add("bomber", plane("bomber", machines.heavy_bomber))
    add("jet-fighter", plane("jet-fighter", machines.jet))
    add("stealth-bomber", plane("stealth-bomber", machines.stealth))
    add("guided-missile", plane("guided-missile", machines.missile, length=0.8, radius=0.06))
    add("nuclear-missile", plane("nuclear-missile", machines.missile, length=1.3, radius=0.11, body="white",
                                 tip="red"))
    add("atomic-bomb", plane("atomic-bomb", machines.bomb))
    add("galleass", vessel("galleass", ships.galleass))
    add("privateer", vessel("privateer", ships.privateer))
    add("ironclad", vessel("ironclad", ships.ironclad))
    add("battleship", vessel("battleship", ships.battleship))
    add("submarine", vessel("submarine", ships.submarine))
    add("nuclear-submarine", vessel("nuclear-submarine", ships.submarine, nuclear=True))
    add("carrier", vessel("carrier", ships.carrier))
    add("missile-cruiser", vessel("missile-cruiser", ships.missile_cruiser))
    add("cargo-ship", vessel("cargo-ship", ships.cargo_ship, category="civilian"))
    add("great-admiral", vessel("great-admiral", ships.flagship, category="great-person"))
    add("worker", single("worker", plain_build(civilians.worker), "civilian", "civilian"))
    add("caravan", single("caravan", plain_build(civilians.caravan), "civilian", "civilian"))
    add("missionary", single("missionary", plain_build(civilians.missionary), "great", "religious"))
    add("inquisitor", single("inquisitor", plain_build(civilians.inquisitor), "great", "religious"))
    add("prophet", single("prophet", plain_build(civilians.prophet), "great", "great-person"))
    for slug in ("artist", "scientist", "merchant", "engineer", "writer", "musician"):
        look, prop = civilians.GREAT[slug]
        add(slug, single(slug, plain_build(civilians.great_person, look, prop), "great", "great-person"))
    look, prop = civilians.GREAT["archaeologist"]
    add("archaeologist", single("archaeologist", plain_build(civilians.great_person, look, prop), "great",
                                "civilian"))
    add("great-general", single("great-general", plain_build(civilians.great_general), "mounted",
                                "great-person"))
    # Spaceship parts -------------------------------------------------------------------------
    for slug, fn in (("ss-booster", machines.ss_booster), ("ss-cockpit", machines.ss_cockpit),
                     ("ss-engine", machines.ss_engine), ("ss-stasis-chamber", machines.ss_stasis)):
        add(slug, Model("unit/" + slug, long_build(fn), "vehicle", "spaceship"))
    return c


def variant(base, slug, build=None, look=None, **changes):
    """Unique unit as a variant of its class model: new look or new build, same budget."""
    if look is not None:
        build = person_build(look)
    budget = changes.pop("budget", base.budget)
    k = ENGINE_SCALE if budget == "siege" and base.budget != "siege" else base.scale
    return Model("unit/" + slug, build, budget, base.category, changes.pop("figures", base.figures), base.contact,
                 k, base.id)


def unique_models(classes):
    """Variant per unique unit slug (classic/docs/classic/unique_units.json). `classes`: every class model
    classic can see, the platform's and its own."""
    u = {}
    k = classes

    def foot(slug, cls, look, **changes):
        u[slug] = variant(k[cls], slug, look=look, **changes)

    def built(slug, cls, build, **changes):
        u[slug] = variant(k[cls], slug, build=build, **changes)

    # Foot -------------------------------------------------------------------------------
    foot("jaguar", "warrior", Look(body="jaguar", legs="jaguar", head="jaguar", right="club", left="round",
                                   skirt="team"))
    foot("maori-warrior", "warrior", Look(body="skin_dark", skin="skin_dark", legs="skin_dark", skirt="team",
                                          head="bare", right="club"))
    foot("hoplite", "spearman", Look(body="team", armor="bronze", skirt="linen", head="crest", head_mat="bronze",
                                     plume="red", right="spear", left="hoplon"))
    foot("immortal", "spearman", Look(body="purple", skirt="team", head="turban", head_mat="linen", right="spear",
                                      left="wicker"))
    foot("pictish-warrior", "spearman", Look(body="team", skirt="green", head="bare", right="spear",
                                             left="buckler", extra=("beard",)))
    built("battering-ram", "spearman", long_build(machines.ram, CREW_ANCIENT), budget="siege", figures=1)
    foot("bowman", "archer", with_(ARCHER, armor="bronze", head="cone", head_mat="bronze", extra=("beard",)))
    foot("slinger", "archer", Look(body="team", skirt="cloth", head="band", head_mat="red", right="sling"))
    foot("atlatlist", "archer", Look(body="skin_dark", skin="skin_dark", skirt="team", head="band",
                                     head_mat="team", plume="feather", right="atlatl"))
    foot("pathfinder", "scout", Look(body="leather", skirt="team", head="band", head_mat="team", left="bow",
                                     back="quiver"))
    foot("legion", "swordsman", Look(body="team", armor="steel", skirt="leather", head="crest", head_mat="steel",
                                     plume="red", right="sword", left="scutum"))
    foot("mohawk-warrior", "swordsman", Look(body="team", skirt="leather", legs="leather", head="band",
                                             head_mat="red", right="axe"))
    foot("kris-swordsman", "swordsman", Look(body="team", skirt="red", head="turban", head_mat="red",
                                             right="kris", left="oval"))
    foot("impi", "pikeman", Look(body="skin_dark", skin="skin_dark", legs="skin_dark", skirt="fur", head="band",
                                 head_mat="team", plume="white", right="spear", left="hide"))
    foot("samurai", "longswordsman", Look(body="team", armor="dark", shoulders="dark", skirt="dark",
                                          head="kabuto", head_mat="dark", right="katana"))
    foot("berserker", "longswordsman", Look(body="fur", skirt="fur", head="horned", head_mat="iron", right="axe",
                                            left="round", extra=("beard",)))
    foot("chu-ko-nu", "crossbowman", Look(body="team", armor="red", head="wide", head_mat="wood",
                                          right="repeater"))
    foot("longbowman", "crossbowman", Look(body="team", legs="green", head="cap", head_mat="green",
                                           left="longbow", back="quiver"))
    foot("minuteman", "musketman", Look(body="cloth", legs="cloth_dark", cape="team", head="tricorne",
                                        right="musket"))
    foot("musketeer", "musketman", Look(body="team", cape="team", legs="cloth_dark", head="wide",
                                        head_mat="black", right="musket", extra=("beard",)))
    foot("janissary", "musketman", Look(body="red", legs="white", cape="team", head="top", head_mat="white",
                                        right="musket"))
    foot("tercio", "musketman", Look(body="team", armor="steel", legs="red", head="morion", head_mat="steel",
                                     right="pike"))
    foot("mehal-sefari", "rifleman", Look(body="white", skirt="white", cape="team", head="bare", right="rifle",
                                          legs="white"))
    foot("norwegian-ski-infantry", "rifleman", Look(body="white", legs="white", cape="team", head="fur",
                                                    head_mat="white", right="rifle", back="pack"))
    foot("carolean", "rifleman", Look(body="team", legs="gold", head="tricorne", right="musket",
                                      extra=("bayonet",)))
    foot("foreign-legion", "great-war-infantry", with_(GREAT_WAR, head="kepi", head_mat="white", legs="red"))
    foot("pracinha", "infantry", with_(INFANTRY, head="brodie", head_mat="khaki", legs="khaki"))
    # Mounted ------------------------------------------------------------------------------
    built("war-chariot", "chariot-archer", long_build(mounts.chariot, with_(CREW_ANCIENT, head="cap",
                                                                            head_mat="gold"),
                                                      with_(ARCHER, back="", head="cap", head_mat="gold"),
                                                      coat="horse_light"))
    built("war-elephant", "chariot-archer", rider_build("elephant", with_(ARCHER, back=""), tower=True),
          figures=2)
    built("horse-archer", "chariot-archer", rider_build("horse", Look(body="team", head="pointed",
                                                                      head_mat="fur", left="bow",
                                                                      back="quiver"), coat="horse_dark"),
          figures=3)
    built("companion-cavalry", "horseman", rider_build("horse", Look(body="team", armor="bronze", head="crest",
                                                                     head_mat="bronze", plume="team",
                                                                     right="lance", cape="team"),
                                                       coat="horse_dark"))
    built("cataphract", "horseman", rider_build("barded", Look(body="team", armor="steel", head="cone",
                                                               head_mat="steel", right="lance", left="round"),
                                                armor="steel"))
    built("african-forest-elephant", "horseman", rider_build("elephant", Look(body="team", head="bare",
                                                                              right="spear"), tower=False),
          figures=2)
    built("camel-archer", "knight", rider_build("camel", Look(body="team", head="turban", head_mat="linen",
                                                              left="bow", back="quiver")))
    built("naresuan-s-elephant", "knight", rider_build("elephant", Look(body="team", armor="gold", head="cone",
                                                                        head_mat="gold", right="halberd"),
                                                       tower=False), figures=2)
    built("mandekalu-cavalry", "knight", rider_build("horse", Look(body="team", head="turban", head_mat="white",
                                                                   right="spear", left="oval"),
                                                     coat="horse_light"))
    built("keshik", "knight", rider_build("horse", Look(body="team", head="pointed", head_mat="red", left="bow",
                                                        back="quiver"), coat="horse_dark"))
    built("conquistador", "knight", rider_build("horse", Look(body="team", armor="steel", head="morion",
                                                              head_mat="steel", right="sabre", left="round"),
                                                coat="horse_light"))
    built("sipahi", "lancer", rider_build("horse", Look(body="team", head="turban", head_mat="white",
                                                        right="lance", cape="red"), coat="horse_light"))
    built("winged-hussar", "lancer", rider_build("horse", Look(body="team", armor="steel", head="dome",
                                                               head_mat="steel", right="lance", cape="fur",
                                                               extra=("wings",)), coat="horse_dark"))
    built("hakkapeliitta", "lancer", rider_build("horse", Look(body="team", armor="leather", head="wide",
                                                               head_mat="black", right="sabre"),
                                                 coat="horse"))
    built("cossack", "cavalry", rider_build("horse", Look(body="team", legs="dark", head="fur", head_mat="black",
                                                          right="sabre"), coat="horse_dark"))
    built("berber-cavalry", "cavalry", rider_build("horse", Look(body="white", cape="team", head="turban",
                                                                 head_mat="white", right="rifle"),
                                                   coat="horse_light"))
    built("comanche-riders", "cavalry", rider_build("horse", Look(body="leather", skirt="team", head="band",
                                                                  head_mat="red", right="rifle"),
                                                    coat="horse_light"))
    built("hussar", "cavalry", rider_build("horse", Look(body="team", cape="team", legs="red", head="fur",
                                                         head_mat="black", plume="white", right="sabre"),
                                           coat="horse"))
    # Siege, armor and air ------------------------------------------------------------------
    built("ballista", "catapult", long_build(machines.ballista, CREW_ANCIENT))
    built("siege-tower", "catapult", long_build(machines.siege_tower, CREW_ANCIENT))
    built("hwach-a", "trebuchet", long_build(machines.hwacha, CREW_KOREAN))
    built("panzer", "tank", long_build(machines.tank, color="field_grey", turret="angular", barrel=0.62))
    built("b17", "bomber", long_build(machines.heavy_bomber, body="steel"))
    built("zero", "fighter", long_build(machines.prop_fighter, body="white", wing="team"))
    # Ships ------------------------------------------------------------------------------------
    built("dromon", "trireme", long_build(ships.trireme, ram="fire", rig="lateen", yaw=-30))
    built("quinquereme", "trireme", long_build(ships.trireme, banks=2, length=2.2, eye=False, tower=True,
                                               yaw=-30))
    built("nau", "caravel", long_build(ships.caravel, rig="square", yaw=-30))
    built("turtle-ship", "caravel", long_build(ships.turtle_ship, yaw=-30))
    built("great-galleass", "galleass", long_build(ships.galleass, length=2.6, stripe="gold", yaw=-30))
    built("ship-of-the-line", "frigate", long_build(ships.frigate, hull_mat="wood_dark", stripe="white", yaw=-30))
    built("sea-beggar", "privateer", long_build(ships.privateer, hull_mat="hull", pennant="fire", yaw=-30))
    # Great people --------------------------------------------------------------------------
    built("khan", "great-general", plain_build(civilians.great_general, khan=True))
    look, prop = civilians.GREAT["merchant-of-venice"]
    built("merchant-of-venice", "merchant", plain_build(civilians.great_person, look, prop))
    return u
