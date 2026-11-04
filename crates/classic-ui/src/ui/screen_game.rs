//! Game screens keep core-backed requests separate from presentation.
use super::*;
use crate::ui::model::{Availability, ResearchView, ScreenModel, TechCard, TechStatus};
#[path = "screen_city_game.rs"]
pub(super) mod city_screen;
#[path = "screen_research.rs"]
mod research_screen;
pub(super) fn city(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, model: &ScreenModel) {
    city_screen::show(ui, r, state, model);
}
pub(super) fn research(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, view: &ResearchView) {
    research_screen::show(ui, r, state, view);
}
