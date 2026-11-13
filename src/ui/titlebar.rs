//! Custom window chrome for the borderless transparent window.
//!
//! The OS decorations are off so the client can draw Discord's thin title strip
//! itself. Everything here is a real viewport command: dragging, double-click
//! maximize/restore, the three window buttons, and eight edge/corner resize
//! grips. The grips are separate foreground areas so they work above every panel
//! without a transparent overlay swallowing normal clicks.

use crate::app::RustcordApp;

pub const TITLEBAR_HEIGHT: f32 = 32.0;
const CONTROL_WIDTH: f32 = 38.0;
const RESIZE_GRIP: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowControl {
    Minimize,
    Maximize,
    Close,
}

fn control_rect(bar: egui::Rect, control: WindowControl) -> egui::Rect {
    let index = match control {
        WindowControl::Minimize => 0,
        WindowControl::Maximize => 1,
        WindowControl::Close => 2,
    };
    let left = bar.right() - (3 - index) as f32 * CONTROL_WIDTH;
    egui::Rect::from_min_size(
        egui::pos2(left, bar.top()),
        egui::vec2(CONTROL_WIDTH, bar.height()),
    )
}

fn window_title(app: &RustcordApp) -> String {
    if app.login.visible {
        return "Rustcord".to_string();
    }
    app.selected_guild
        .as_ref()
        .and_then(|id| app.guilds.iter().find(|guild| &guild.id == id))
        .map(|guild| guild.name.clone())
        .unwrap_or_else(|| "Rustcord".to_string())
}

fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|input| input.viewport().maximized.unwrap_or(false))
}

fn fullscreen(ctx: &egui::Context) -> bool {
    ctx.input(|input| input.viewport().fullscreen.unwrap_or(false))
}

fn paint_control(
    painter: &egui::Painter,
    bar: egui::Rect,
    control: WindowControl,
    hovered: bool,
    is_maximized: bool,
    theme: &crate::ui::theme::AppTheme,
) {
    let rect = control_rect(bar, control);
    let close = control == WindowControl::Close;
    let fill = if hovered {
        if close {
            theme.danger
        } else {
            crate::utils::lift(theme.surface_alt, 12)
        }
    } else {
        egui::Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 0.0, fill);

    let color = if hovered && close {
        egui::Color32::WHITE
    } else {
        theme.text
    };
    let center = rect.center();
    let stroke = egui::Stroke::new(1.0, color);
    match control {
        WindowControl::Minimize => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 5.0, center.y),
                    egui::pos2(center.x + 5.0, center.y),
                ],
                stroke,
            );
        }
        WindowControl::Maximize if is_maximized => {
            // Restore: two offset squares, like Discord's window chrome.
            let back = egui::Rect::from_min_size(
                egui::pos2(center.x - 5.0, center.y - 4.0),
                egui::vec2(7.0, 7.0),
            );
            let front = egui::Rect::from_min_size(
                egui::pos2(center.x - 3.0, center.y - 6.0),
                egui::vec2(7.0, 7.0),
            );
            painter.rect_stroke(back, 0.0, stroke, egui::StrokeKind::Inside);
            painter.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Inside);
        }
        WindowControl::Maximize => {
            let square = egui::Rect::from_center_size(center, egui::vec2(9.0, 9.0));
            painter.rect_stroke(square, 0.0, stroke, egui::StrokeKind::Inside);
        }
        WindowControl::Close => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 4.5, center.y - 4.5),
                    egui::pos2(center.x + 4.5, center.y + 4.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 4.5, center.y + 4.5),
                    egui::pos2(center.x + 4.5, center.y - 4.5),
                ],
                stroke,
            );
        }
    }
}

fn apply_control(ctx: &egui::Context, control: WindowControl, is_maximized: bool) {
    match control {
        WindowControl::Minimize => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
        WindowControl::Maximize => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
        }
        WindowControl::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
    }
}

/// Draw the 32 px title strip and register its drag/window-button hit areas.
pub fn show(app: &RustcordApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let theme = app.plugins.theme();
    let title = window_title(app);
    let is_maximized = maximized(&ctx);

    egui::Panel::top("window_titlebar")
        .exact_size(TITLEBAR_HEIGHT)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(theme.surface_alt))
        .show(ui, |ui| {
            let bar = ui.max_rect();
            let controls_left = bar.right() - 3.0 * CONTROL_WIDTH;
            let drag_rect =
                egui::Rect::from_min_max(bar.min, egui::pos2(controls_left, bar.bottom()));
            let drag = ui.interact(
                drag_rect,
                egui::Id::unique("window_titlebar_drag"),
                egui::Sense::click_and_drag(),
            );
            if drag.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
            }

            ui.painter().hline(
                bar.x_range(),
                bar.bottom(),
                egui::Stroke::new(1.0, crate::utils::lift(theme.surface_alt, -10)),
            );
            ui.painter().text(
                bar.center(),
                egui::Align2::CENTER_CENTER,
                title,
                egui::FontId::proportional(12.5),
                theme.muted,
            );

            for control in [
                WindowControl::Minimize,
                WindowControl::Maximize,
                WindowControl::Close,
            ] {
                let rect = control_rect(bar, control);
                let response = ui
                    .interact(
                        rect,
                        egui::Id::unique(("window_titlebar_control", control as u8)),
                        egui::Sense::click(),
                    )
                    .on_hover_text(match control {
                        WindowControl::Minimize => "Minimize",
                        WindowControl::Maximize if is_maximized => "Restore",
                        WindowControl::Maximize => "Maximize",
                        WindowControl::Close => "Close",
                    });
                paint_control(
                    ui.painter(),
                    bar,
                    control,
                    response.hovered(),
                    is_maximized,
                    theme,
                );
                if response.clicked() {
                    apply_control(&ctx, control, is_maximized);
                }
            }
        });
}

fn resize_cursor(direction: egui::ResizeDirection) -> egui::CursorIcon {
    use egui::ResizeDirection as Direction;
    match direction {
        Direction::North | Direction::South => egui::CursorIcon::ResizeVertical,
        Direction::East | Direction::West => egui::CursorIcon::ResizeHorizontal,
        Direction::NorthEast | Direction::SouthWest => egui::CursorIcon::ResizeNeSw,
        Direction::NorthWest | Direction::SouthEast => egui::CursorIcon::ResizeNwSe,
    }
}

fn resize_handles(screen: egui::Rect) -> Vec<(egui::ResizeDirection, egui::Rect)> {
    use egui::ResizeDirection as Direction;
    let n = screen.left_top();
    let s = screen.right_bottom();
    let t = RESIZE_GRIP;
    let mut handles = vec![
        (
            Direction::North,
            egui::Rect::from_min_max(n, egui::pos2(s.x, n.y + t)),
        ),
        (
            Direction::South,
            egui::Rect::from_min_max(egui::pos2(n.x, s.y - t), s),
        ),
        (
            Direction::West,
            egui::Rect::from_min_max(n, egui::pos2(n.x + t, s.y)),
        ),
        (
            Direction::East,
            egui::Rect::from_min_max(egui::pos2(s.x - t, n.y), s),
        ),
    ];
    // Corners go last so they sit above the edge strips and take diagonal drags.
    handles.extend([
        (
            Direction::NorthWest,
            egui::Rect::from_min_max(n, egui::pos2(n.x + t, n.y + t)),
        ),
        (
            Direction::NorthEast,
            egui::Rect::from_min_max(egui::pos2(s.x - t, n.y), egui::pos2(s.x, n.y + t)),
        ),
        (
            Direction::SouthWest,
            egui::Rect::from_min_max(egui::pos2(n.x, s.y - t), egui::pos2(n.x + t, s.y)),
        ),
        (
            Direction::SouthEast,
            egui::Rect::from_min_max(egui::pos2(s.x - t, s.y - t), s),
        ),
    ]);
    handles
}

/// Register the borderless window's resize grips. Called last so the foreground
/// strips sit above the panels and modal elements.
pub fn resize_edges(ctx: &egui::Context) {
    if maximized(ctx) || fullscreen(ctx) {
        return;
    }
    let screen = ctx.content_rect();
    for (index, (direction, rect)) in resize_handles(screen).into_iter().enumerate() {
        let response = egui::Area::new(egui::Id::unique(("window_resize", index)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .default_size(rect.size())
            .constrain(false)
            .sense(egui::Sense::drag())
            .show(ctx, |ui| {
                ui.set_min_size(rect.size());
            })
            .response;
        if response.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
        if response.hovered() {
            ctx.set_cursor_icon(resize_cursor(direction));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(900.0, TITLEBAR_HEIGHT))
    }

    #[test]
    fn window_controls_sit_flush_against_the_right_edge() {
        let bar = bar();
        let minimize = control_rect(bar, WindowControl::Minimize);
        let maximize = control_rect(bar, WindowControl::Maximize);
        let close = control_rect(bar, WindowControl::Close);
        assert_eq!(minimize.left(), bar.right() - 3.0 * CONTROL_WIDTH);
        assert_eq!(maximize.left(), minimize.right());
        assert_eq!(close.left(), maximize.right());
        assert_eq!(close.right(), bar.right());
        assert_eq!(close.height(), TITLEBAR_HEIGHT);
    }

    #[test]
    fn resize_handles_cover_every_edge_and_corner() {
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 700.0));
        let handles = resize_handles(screen);
        assert_eq!(handles.len(), 8);
        for (_, rect) in &handles {
            assert!(screen.contains_rect(*rect));
        }
        let north = handles[0].1;
        let south = handles[1].1;
        let west = handles[2].1;
        let east = handles[3].1;
        let north_west = handles[4].1;
        let south_east = handles[7].1;
        assert_eq!(north.top(), screen.top());
        assert_eq!(south.bottom(), screen.bottom());
        assert_eq!(west.left(), screen.left());
        assert_eq!(east.right(), screen.right());
        assert_eq!(north_west.min, screen.min);
        assert_eq!(south_east.max, screen.max);
    }
}
