//! Right-click or "?" over an entity anywhere in the UI opens its
//! Encyclopedia page. Widgets call `mark` with the entity's page while they
//! draw it; the game takes the request after the frame (`take`).
use crate::ui::pedia::PediaRef;

fn request_id() -> egui::Id {
    egui::Id::new("pedia_link_request")
}

/// The entity drawn in `rect` has a Encyclopedia page: a right click or "?"
/// while the pointer is over it asks for that page.
pub fn mark(ui: &egui::Ui, rect: egui::Rect, page: PediaRef) {
    if !ui.rect_contains_pointer(rect) {
        return;
    }
    let asked = ui.input(|i| i.pointer.secondary_clicked() || i.key_pressed(egui::Key::Questionmark));
    if asked {
        ui.ctx().data_mut(|d| d.insert_temp(request_id(), Some(page)));
    }
}

/// `mark` for a widget response (its rect).
pub fn mark_response(ui: &egui::Ui, response: &egui::Response, page: PediaRef) {
    mark(ui, response.rect, page);
}

/// The page asked for in this frame, once.
pub fn take(ctx: &egui::Context) -> Option<PediaRef> {
    ctx.data_mut(|d| d.remove_temp::<Option<PediaRef>>(request_id())).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::pedia::Table;

    #[test]
    fn right_click_or_question_mark_over_an_entity_asks_for_its_page() {
        let ctx = egui::Context::default();
        let page = PediaRef::new(Table::Unit, 3);
        let rect = egui::Rect::from_min_size(egui::pos2(10., 10.), egui::vec2(50., 20.));
        let frame = |events: Vec<egui::Event>| {
            let raw = egui::RawInput { events, ..Default::default() };
            // No painter applies the font atlas here: drop its upload on purpose.
            ctx.run_ui(raw, |ui| mark(ui, rect, page)).textures_delta.clear();
            take(&ctx)
        };
        let over = egui::pos2(20., 15.);
        assert_eq!(frame(vec![egui::Event::PointerMoved(over)]), None, "hover alone asks nothing");
        let press = |pressed| egui::Event::PointerButton {
            pos: over,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![press(true)]);
        assert_eq!(frame(vec![press(false)]), Some(page));
        assert_eq!(take(&ctx), None, "taken once");
        let question = egui::Event::Key {
            key: egui::Key::Questionmark,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(frame(vec![question.clone()]), Some(page));
        frame(vec![egui::Event::PointerMoved(egui::pos2(300., 300.))]);
        assert_eq!(frame(vec![question]), None, "only over the entity");
    }
}
