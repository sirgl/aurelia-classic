//! The classic AI advisors: one per game system that brings its own AI
//! decisions (PLAN M6 item 7). The kernel's registry asks them at their
//! moments of an AI turn (`ai::system`).

use crate::ai::system::SystemAdvisor;

/// The classic systems with an AI advisor. On equal priority their proposals
/// come in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClassicSystem {
    Economy,
    Policies,
    Religion,
    Culture,
    Military,
    Diplomacy,
    CityStates,
    Congress,
    Espionage,
    /// Science victory: Apollo, spaceship parts.
    Victory,
}

/// Every system's advisor.
pub(crate) fn advisors() -> Vec<(ClassicSystem, Box<dyn SystemAdvisor>)> {
    use crate::ai;
    vec![
        (ClassicSystem::Economy, Box::new(ai::economy::EconomyAdvisor)),
        (ClassicSystem::Policies, Box::new(ai::policies::PolicyAdvisor)),
        (ClassicSystem::Religion, Box::new(ai::religion::ReligionAdvisor)),
        (ClassicSystem::Culture, Box::new(ai::tourism::TourismAdvisor)),
        (ClassicSystem::Military, Box::new(ai::combat::MilitaryAdvisor)),
        (ClassicSystem::Diplomacy, Box::new(ai::diplomacy::DiplomacyAdvisor::default())),
        (ClassicSystem::CityStates, Box::new(ai::city_states::CityStatesAdvisor)),
        (ClassicSystem::Congress, Box::new(ai::congress::CongressAdvisor)),
        (ClassicSystem::Espionage, Box::new(ai::espionage::EspionageAdvisor)),
        (ClassicSystem::Victory, Box::new(ai::victory::VictoryAdvisor)),
    ]
}
