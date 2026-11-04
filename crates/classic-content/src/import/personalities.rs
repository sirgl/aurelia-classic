//! The leader personalities of the classic ruleset (`classic_rules::rules::content::
//! Personality`, classic/docs/ai-leaders.md): this project's own judgement, 0 to 10
//! with 5 neutral, from public descriptions of each leader and of the
//! nation's ability (its name in `nations.ron`) — never from
//! any game files. Written to `personalities.ron`; `why` says what each
//! leaning stands on.

use super::ron_out::Record;

/// One leader: the nation key, then expansion, wonders, science,
/// culture, gold, religion, aggression, war tolerance, diplomacy, and why.
type Row = (&'static str, [u8; 9], &'static str);

const LEADERS: [Row; 43] = [
    ("core:nation/america", [7, 5, 6, 5, 6, 4, 5, 4, 5], "Manifest Destiny: land and growth; a republic slow to start wars"),
    ("core:nation/arabia", [5, 5, 6, 5, 8, 7, 4, 5, 5], "Ships of the Desert: caravans and trade; a caliph of faith and learning"),
    ("core:nation/assyria", [5, 5, 7, 5, 4, 4, 7, 7, 3], "Treasures of Nineveh: knowledge taken by conquest; siege warfare"),
    ("core:nation/austria", [4, 5, 5, 6, 7, 5, 3, 3, 9], "Diplomatic Marriage: city-states won by gold and marriage, not war"),
    ("core:nation/aztecs", [6, 4, 4, 5, 4, 6, 9, 8, 3], "Sacrificial Captives: culture from kills; a warrior empire"),
    ("core:nation/babylon", [4, 6, 9, 5, 5, 4, 4, 4, 5], "Ingenuity: great scientists; a builder of science"),
    ("core:nation/brazil", [5, 6, 5, 9, 5, 4, 3, 3, 6], "Carnival: tourism and golden ages; a peaceful culture"),
    ("core:nation/byzantium", [5, 6, 4, 6, 5, 9, 4, 5, 6], "Patriarchate of Constantinople: an extra belief; a religious empire"),
    ("core:nation/carthage", [6, 5, 5, 4, 8, 4, 5, 5, 5], "Phoenician Heritage: harbours on every coast; a trading power"),
    ("core:nation/celts", [5, 4, 4, 5, 4, 9, 6, 6, 4], "Druidic Lore: faith from forests; warlike tribes"),
    ("core:nation/china", [5, 7, 7, 6, 5, 4, 6, 5, 5], "Art of War: great generals; an empire of builders and scholars"),
    ("core:nation/denmark", [5, 4, 4, 4, 5, 5, 8, 7, 3], "Viking Fury: raiders from the sea"),
    ("core:nation/egypt", [5, 10, 5, 7, 5, 6, 4, 4, 5], "Monument Builders: wonders built faster; the pyramids"),
    ("core:nation/england", [7, 5, 6, 5, 6, 4, 6, 5, 5], "Sun Never Sets: a navy and colonies"),
    ("core:nation/ethiopia", [4, 5, 5, 6, 5, 9, 3, 3, 6], "Spirit of Adwa: faith and defence against larger empires"),
    ("core:nation/france", [5, 7, 5, 8, 5, 4, 7, 7, 4], "City of Light: culture and tourism; Napoleon's conquests"),
    ("core:nation/germany", [5, 4, 7, 4, 6, 4, 6, 6, 6], "Furor Teutonicus: barbarians join its army; Bismarck's industry and diplomacy"),
    ("core:nation/greece", [5, 5, 5, 6, 4, 3, 8, 7, 8], "Hellenic League: city-state friends; Alexander's conquests"),
    ("core:nation/huns", [7, 2, 3, 2, 4, 3, 10, 10, 2], "Scourge of God: cities razed and taken; Attila's raids"),
    ("core:nation/inca", [7, 6, 5, 5, 6, 5, 4, 4, 4], "Great Andean Road: mountain roads and terraces; a builder empire"),
    ("core:nation/india", [3, 6, 5, 7, 5, 8, 1, 1, 7], "Population Growth: tall cities; Gandhi's non-violence"),
    ("core:nation/indonesia", [7, 5, 5, 5, 6, 7, 5, 5, 4], "Spice Islanders: new islands bring luxuries; a sea of faiths"),
    ("core:nation/iroquois", [6, 4, 5, 5, 5, 5, 5, 4, 6], "The Great Warpath: forest roads; the Confederacy's diplomacy"),
    ("core:nation/japan", [5, 5, 5, 5, 4, 4, 9, 8, 3], "Bushido: units fight at full strength while hurt; Nobunaga's wars"),
    ("core:nation/korea", [4, 6, 10, 6, 5, 4, 3, 3, 5], "Scholars of the Jade Hall: science from specialists and wonders"),
    ("core:nation/maya", [5, 7, 7, 6, 4, 7, 4, 5, 4], "The Long Count: great people by the calendar; temples and astronomy"),
    ("core:nation/mongolia", [7, 3, 4, 3, 5, 3, 10, 9, 2], "Mongol Terror: cavalry against city-states; Genghis Khan's conquests"),
    ("core:nation/morocco", [5, 5, 4, 6, 9, 5, 4, 5, 6], "Gateway to Africa: gold and culture from trade routes"),
    ("core:nation/netherlands", [5, 5, 5, 6, 9, 4, 3, 3, 7], "Dutch East India Company: luxuries traded; a merchant republic"),
    ("core:nation/ottomans", [6, 6, 5, 5, 6, 5, 7, 6, 4], "Barbary Corsairs: captured ships; Suleiman's expanding empire"),
    ("core:nation/persia", [6, 6, 5, 7, 7, 5, 5, 5, 5], "Achaemenid Legacy: long golden ages; a rich and tolerant empire"),
    ("core:nation/poland", [6, 5, 5, 7, 5, 5, 6, 5, 5], "Solidarity: extra policies; a land of horsemen"),
    ("core:nation/polynesia", [9, 6, 4, 6, 4, 5, 5, 5, 4], "Wayfinding: early ocean voyages; islands settled"),
    ("core:nation/portugal", [6, 4, 5, 5, 9, 4, 4, 4, 6], "Mare Clausum: rich trade routes; a sea-trading empire"),
    ("core:nation/rome", [8, 6, 5, 5, 5, 4, 7, 7, 4], "The Glory of Rome: provinces built like the capital; legions"),
    ("core:nation/russia", [8, 5, 5, 5, 5, 5, 6, 6, 4], "Siberian Riches: strategic resources; a vast expanding land"),
    ("core:nation/shoshone", [9, 4, 4, 4, 5, 5, 5, 4, 4], "Great Expanse: large founding borders; a people of the open land"),
    ("core:nation/siam", [5, 6, 5, 6, 5, 6, 4, 4, 9], "Father Governs Children: more from city-state friends"),
    ("core:nation/songhai", [5, 5, 4, 4, 7, 6, 7, 7, 4], "River Warlord: gold from conquest and camps; river raiders"),
    ("core:nation/spain", [7, 5, 5, 5, 7, 8, 6, 6, 3], "Seven Cities of Gold: natural wonders; conquistadors and missions"),
    ("core:nation/sweden", [5, 5, 7, 5, 5, 4, 6, 5, 8], "Nobel Prize: great people from friendships; Gustavus Adolphus's army"),
    ("core:nation/venice", [0, 6, 5, 5, 10, 4, 3, 4, 8], "Serenissima: no settlers, merchants buy city-states; a trading republic"),
    ("core:nation/zulu", [6, 3, 4, 3, 4, 3, 10, 9, 2], "Iklwa: cheap, strong warriors; Shaka's impis"),
];

/// The field names of a personality, in the order of `LEADERS` values.
const FIELDS: [&str; 9] =
    ["expansion", "wonders", "science", "culture", "gold", "religion", "aggression", "war_tolerance", "diplomacy"];

/// Every leader's record, sorted by nation key.
pub fn personalities() -> Vec<Record> {
    let mut rows = LEADERS.to_vec();
    rows.sort_by_key(|(nation, _, _)| *nation);
    rows.into_iter()
        .map(|(nation, values, why)| {
            let record = Record::new().str("nation", nation);
            let record = FIELDS.iter().zip(values).fold(record, |r, (field, value)| r.raw(*field, value.to_string()));
            record.str("why", why)
        })
        .collect()
}
