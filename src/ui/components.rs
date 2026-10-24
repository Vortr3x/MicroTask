use crate::model::event::MacroEvent;
use eframe::egui::{self, Color32, Response, RichText, Sense, Stroke, Ui, Vec2};

pub fn stat_card(ui: &mut Ui, label: &str, value: &str, subtext: &str, accent_color: Color32) {
    egui::Frame::new()
        .fill(Color32::from_rgb(26, 30, 42))
        .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 70)))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(label)
                    .size(9.5)
                    .strong()
                    .color(Color32::from_rgb(156, 163, 175)),
            );
            ui.add_space(1.0);
            ui.label(
                RichText::new(value)
                    .size(17.0)
                    .strong()
                    .color(accent_color),
            );
            if !subtext.is_empty() {
                ui.add_space(1.0);
                ui.label(
                    RichText::new(subtext)
                        .size(9.0)
                        .color(Color32::from_rgb(107, 114, 128)),
                );
            }
        });
}

pub fn status_pill(ui: &mut Ui, status_text: &str, color: Color32, pulsing: bool) {
    let padding = Vec2::new(8.0, 4.0);
    let corner_radius = 12;

    let font_id = egui::FontId::proportional(11.0);
    let galley = ui.painter().layout_no_wrap(
        status_text.to_string(),
        font_id,
        Color32::WHITE,
    );

    let dot_radius = 3.5;
    let dot_gap = 5.0;
    let total_width = padding.x * 2.0 + dot_radius * 2.0 + dot_gap + galley.size().x;
    let total_height = padding.y * 2.0 + galley.size().y.max(dot_radius * 2.0);

    let (rect, _response) = ui.allocate_exact_size(Vec2::new(total_width, total_height), Sense::hover());

    let bg_color = Color32::from_rgba_premultiplied(
        (color.r() as f32 * 0.25) as u8,
        (color.g() as f32 * 0.25) as u8,
        (color.b() as f32 * 0.25) as u8,
        255,
    );

    ui.painter().rect_filled(rect, corner_radius, bg_color);
    ui.painter().rect_stroke(
        rect,
        corner_radius,
        Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );

    // Draw indicator dot
    let dot_center = egui::pos2(
        rect.left() + padding.x + dot_radius,
        rect.center().y,
    );

    if pulsing {
        let time = ui.input(|i| i.time);
        let pulse = (time * 4.0).sin().abs() as f32;
        let halo_radius = dot_radius + pulse * 2.5;
        let halo_color = Color32::from_rgba_premultiplied(
            color.r(),
            color.g(),
            color.b(),
            (110.0 * (1.0 - pulse)) as u8,
        );
        ui.painter().circle_filled(dot_center, halo_radius, halo_color);
    }

    ui.painter().circle_filled(dot_center, dot_radius, color);

    // Draw text
    let text_pos = egui::pos2(
        dot_center.x + dot_radius + dot_gap,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(text_pos, galley, Color32::WHITE);
}

pub fn action_button(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    bg_color: Color32,
    active: bool,
) -> Response {
    let text = format!("{} {}", icon, label);
    let button = egui::Button::new(
        RichText::new(text)
            .size(12.0)
            .strong()
            .color(if active { Color32::WHITE } else { Color32::from_rgb(225, 230, 240) }),
    )
    .fill(bg_color)
    .stroke(if active {
        Stroke::new(1.5, Color32::WHITE)
    } else {
        Stroke::new(1.0, Color32::from_rgb(60, 68, 88))
    })
    .corner_radius(6)
    .min_size(Vec2::new(76.0, 30.0));

    ui.add(button)
}

pub fn event_row(
    ui: &mut Ui,
    index: usize,
    event: &MacroEvent,
    selected: bool,
) -> (Response, bool) {
    let mut delete_clicked = false;

    let row_bg = if selected {
        Color32::from_rgb(38, 48, 68)
    } else if index % 2 == 0 {
        Color32::from_rgb(22, 26, 36)
    } else {
        Color32::from_rgb(18, 22, 30)
    };

    let frame = egui::Frame::new()
        .fill(row_bg)
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(8, 5));

    let response = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            // Index & icon
            ui.label(
                RichText::new(format!("#{:03} {}", index + 1, event.icon()))
                    .size(11.0)
                    .monospace()
                    .color(Color32::from_rgb(107, 114, 128)),
            );

            // Timestamp
            ui.label(
                RichText::new(format!("+{:>5}ms", event.timestamp_ms))
                    .size(11.0)
                    .monospace()
                    .color(Color32::from_rgb(156, 163, 175)),
            );

            // Category badge
            let (badge_text, _) = event.category_badge();
            let badge_color = match badge_text {
                "MOVE" => Color32::from_rgb(96, 165, 250),
                "CLICK" => Color32::from_rgb(52, 211, 153),
                "RELEASE" => Color32::from_rgb(248, 113, 113),
                "WHEEL" | "H-WHEEL" => Color32::from_rgb(192, 132, 252),
                _ => Color32::from_rgb(251, 191, 36),
            };

            ui.label(
                RichText::new(format!("[{}]", badge_text))
                    .size(10.5)
                    .strong()
                    .monospace()
                    .color(badge_color),
            );

            // Description
            ui.label(
                RichText::new(event.description())
                    .size(11.5)
                    .color(Color32::from_rgb(229, 231, 235)),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(
                        RichText::new("✕")
                            .size(10.0)
                            .color(Color32::from_rgb(239, 68, 68)),
                    )
                    .on_hover_text("Delete this event")
                    .clicked()
                {
                    delete_clicked = true;
                }
            });
        });
    });

    (response.response, delete_clicked)
}
