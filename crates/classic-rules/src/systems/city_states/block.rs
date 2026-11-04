//! Why a city-state command is refused. The English reasons are the rules'
//! text (logs and tests); the interface words them in `reasons_rules`.

use crate::command::CommandError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CityStateBlock {
    NotANation,
    NotACityState,
    NotMet,
    AtWar,
    NotEnoughGold,
    UnitNotGiftable,
    NotTheirLand,
    NoFreeTile,
    NoResource,
    NoImprovementNeeded,
    ImprovementUnknown,
    NotAfraid,
    NoWorker,
    NoRoomForWorker,
    NoPledge,
    PledgedRecently,
    AlreadyPledged,
    NotOnGoodTerms,
    PledgeBrokenRecently,
    NotTheAlly,
    CannotAnnex,
    AlliedTooRecently,
}

impl CityStateBlock {
    pub const ALL: [CityStateBlock; 22] = [
        CityStateBlock::NotANation,
        CityStateBlock::NotACityState,
        CityStateBlock::NotMet,
        CityStateBlock::AtWar,
        CityStateBlock::NotEnoughGold,
        CityStateBlock::UnitNotGiftable,
        CityStateBlock::NotTheirLand,
        CityStateBlock::NoFreeTile,
        CityStateBlock::NoResource,
        CityStateBlock::NoImprovementNeeded,
        CityStateBlock::ImprovementUnknown,
        CityStateBlock::NotAfraid,
        CityStateBlock::NoWorker,
        CityStateBlock::NoRoomForWorker,
        CityStateBlock::NoPledge,
        CityStateBlock::PledgedRecently,
        CityStateBlock::AlreadyPledged,
        CityStateBlock::NotOnGoodTerms,
        CityStateBlock::PledgeBrokenRecently,
        CityStateBlock::NotTheAlly,
        CityStateBlock::CannotAnnex,
        CityStateBlock::AlliedTooRecently,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            CityStateBlock::NotANation => "only nations deal with city-states",
            CityStateBlock::NotACityState => "not a city-state",
            CityStateBlock::NotMet => "not met",
            CityStateBlock::AtWar => "at war",
            CityStateBlock::NotEnoughGold => "not enough gold",
            CityStateBlock::UnitNotGiftable => "this unit cannot be gifted",
            CityStateBlock::NotTheirLand => "not on the city-state's land",
            CityStateBlock::NoFreeTile => "no free tile of the city-state",
            CityStateBlock::NoResource => "no resource to improve",
            CityStateBlock::NoImprovementNeeded => "no improvement needed",
            CityStateBlock::ImprovementUnknown => "improvement not known",
            CityStateBlock::NotAfraid => "the city-state is not afraid",
            CityStateBlock::NoWorker => "no worker to take",
            CityStateBlock::NoRoomForWorker => "no room for the worker",
            CityStateBlock::NoPledge => "no pledge",
            CityStateBlock::PledgedRecently => "pledged too recently",
            CityStateBlock::AlreadyPledged => "already pledged",
            CityStateBlock::NotOnGoodTerms => "not on good terms",
            CityStateBlock::PledgeBrokenRecently => "a broken pledge is too recent",
            CityStateBlock::NotTheAlly => "not the ally",
            CityStateBlock::CannotAnnex => "cannot annex city-states",
            CityStateBlock::AlliedTooRecently => "allied too recently",
        }
    }
}

impl From<CityStateBlock> for CommandError {
    fn from(block: CityStateBlock) -> Self {
        CommandError::RuleViolation(block.reason().into())
    }
}
