//! The world state of the classic module: every mechanic the kernel does not
//! know. Owner of each part: the system named in its type's module
//! (`systems::trade` owns `trade`, ...). Authoritative: saved and hashed.

use crate::classic::Classic;
use crate::classic::change::ClassicChange;
use crate::classic::save::SavedClassic;
use crate::persistence::decode::DecodeError;
use crate::rules::CompiledRuleset;
use crate::validation::world::ValidationError;
use crate::world::barbarians::BarbarianState;
use crate::world::city_states::CityStatesState;
use crate::world::congress::CongressState;
use crate::world::culture::CultureState;
use crate::world::espionage::EspionageState;
use crate::world::military::MilitaryState;
use crate::world::module::{HashPoint, ModuleId, ModuleWorld, NewModuleWorld};
use crate::world::relations::RelationsState;
use crate::world::religion::ReligionState;
use crate::world::state::AuthWorld;
use crate::world::tourism::TourismState;
use crate::world::trade::TradeState;
use crate::world::victory::VictoryState;

#[derive(Debug, Clone)]
pub struct ClassicWorld {
    /// Running trade routes.
    pub(crate) trade: TradeState,
    /// Great general and admiral points (combat system).
    pub(crate) military: MilitaryState,
    /// Barbarian option, camps, cleared camps and their random stream.
    pub(crate) barbarians: BarbarianState,
    /// Influence, allies, quests of the city-states.
    pub(crate) city_states: CityStatesState,
    /// Victory options, projects, the outcome.
    pub(crate) victory: VictoryState,
    /// The World Congress / United Nations.
    pub(crate) congress: CongressState,
    /// Spies and steal rights.
    pub(crate) espionage: EspionageState,
    /// Culture pools and social policies.
    pub(crate) culture: CultureState,
    /// Faith, pantheons, religions and religious pressure.
    pub(crate) religion: ReligionState,
    /// Great works, cultural influence, public opinion, archaeology.
    pub(crate) tourism: TourismState,
    /// Treaties, embassies, friendships, warmonger memory, deals, offers
    /// (war and open borders are the kernel's `DiplomacyState`).
    pub(crate) relations: RelationsState,
}

impl ModuleWorld<Classic> for ClassicWorld {
    const ID: ModuleId = ModuleId {
        name: "classic",
        version: 1,
    };
    type Saved = SavedClassic;
    type Player = crate::classic::player::PlayerEconomy;
    type PlayerKind = crate::world::player::ClassicKind;
    type City = crate::classic::city::ClassicCity;
    type Unit = crate::classic::unit::ClassicUnit;
    type Change = ClassicChange;

    fn new(game: NewModuleWorld<'_>) -> Self {
        let players = game.players;
        Self {
            trade: Default::default(),
            military: MilitaryState::new(players),
            barbarians: BarbarianState::new(game.seed),
            city_states: CityStatesState::new(players),
            victory: VictoryState::new(players, game.rules.module.projects.len()),
            congress: CongressState::new(players),
            espionage: EspionageState::new(players),
            culture: CultureState::new(players),
            religion: ReligionState::new(players, game.seed),
            tourism: TourismState::new(players, game.seed),
            relations: RelationsState::new(players),
        }
    }

    fn hash(&self, point: HashPoint, auth: &AuthWorld, mix: &mut dyn FnMut(u64)) {
        crate::classic::hash::hash(self, point, auth, mix);
    }

    /// Every per-player part has one entry per player of the world.
    /// The religion a unit carries and its dig go with it.
    fn unit_removed(&mut self, unit: crate::ids::EntityId) {
        self.religion.remove_unit(unit);
        self.tourism.archaeology_mut().digs.retain(|d| d.unit != unit);
    }

    fn validate(&self, auth: &AuthWorld, _rules: &CompiledRuleset) -> Result<(), ValidationError> {
        crate::systems::cargo::validate_cargo(auth)?;
        let players = auth.players().count();
        let sizes = [
            ("military", self.military.all().len()),
            ("culture", self.culture.players().len()),
            ("religion", self.religion.players().len()),
            ("tourism", self.tourism.players().len()),
            ("city-states", self.city_states.players()),
        ];
        match sizes.iter().find(|(_, size)| *size != players) {
            Some((part, size)) => Err(ValidationError::Module(format!(
                "{part}: {size} players of {players}"
            ))),
            None => Ok(()),
        }
    }

    fn encode(&self, auth: &AuthWorld) -> SavedClassic {
        crate::classic::save::encode(self, auth)
    }

    fn decode(
        saved: &SavedClassic,
        auth: &AuthWorld,
        rules: &CompiledRuleset,
    ) -> Result<Self, DecodeError> {
        crate::classic::save::decode(saved, auth, rules)
    }
}

impl ClassicWorld {
    pub fn trade(&self) -> &TradeState {
        &self.trade
    }
    pub fn military(&self) -> &MilitaryState {
        &self.military
    }
    pub fn barbarians(&self) -> &BarbarianState {
        &self.barbarians
    }
    pub fn city_states(&self) -> &CityStatesState {
        &self.city_states
    }
    pub fn victory(&self) -> &VictoryState {
        &self.victory
    }
    pub fn congress(&self) -> &CongressState {
        &self.congress
    }
    pub fn espionage(&self) -> &EspionageState {
        &self.espionage
    }
    pub fn culture(&self) -> &CultureState {
        &self.culture
    }
    pub fn religion(&self) -> &ReligionState {
        &self.religion
    }
    pub fn tourism(&self) -> &TourismState {
        &self.tourism
    }
    pub fn relations(&self) -> &RelationsState {
        &self.relations
    }
}
