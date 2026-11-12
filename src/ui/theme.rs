use egui::{Color32, Stroke};

#[derive(Debug, Clone)]
pub struct AppTheme {
    pub name: String,
    pub background: Color32,
    pub surface: Color32,
    pub surface_alt: Color32,
    pub accent: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub danger: Color32,
    pub success: Color32,
}

impl Default for AppTheme {
    fn default() -> Self {
        Self {
            name: "Rustcord Dark".to_string(),
            background: Color32::from_rgb(0x31, 0x33, 0x38),
            surface: Color32::from_rgb(0x2b, 0x2d, 0x31),
            surface_alt: Color32::from_rgb(0x23, 0x25, 0x28),
            accent: Color32::from_rgb(0x58, 0x65, 0xf2),
            text: Color32::from_rgb(0xdb, 0xde, 0xe1),
            muted: Color32::from_rgb(0x94, 0x9b, 0xa4),
            danger: Color32::from_rgb(0xf2, 0x3f, 0x43),
            success: Color32::from_rgb(0x23, 0xa5, 0x5a),
        }
    }
}

impl AppTheme {
    /// Apply the theme to the egui context. Called every frame for now so
    /// live theme edits show up immediately; this is cheap compared to redraw.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();

        visuals.panel_fill = self.background;
        visuals.window_fill = self.surface;
        visuals.extreme_bg_color = self.surface_alt;
        visuals.faint_bg_color = self.surface_alt;
        visuals.code_bg_color = self.surface_alt;
        visuals.override_text_color = Some(self.text);
        visuals.hyperlink_color = self.accent;
        visuals.warn_fg_color = self.danger;
        visuals.error_fg_color = self.danger;

        visuals.widgets.noninteractive.bg_fill = self.surface;
        visuals.widgets.noninteractive.weak_bg_fill = self.surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.surface_alt);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.muted);

        visuals.widgets.inactive.bg_fill = self.surface_alt;
        visuals.widgets.inactive.weak_bg_fill = self.surface;
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text);

        visuals.widgets.hovered.bg_fill = self.surface_alt;
        visuals.widgets.hovered.weak_bg_fill = self.accent.gamma_multiply(0.35);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, self.text);

        visuals.widgets.active.bg_fill = self.accent;
        visuals.widgets.active.weak_bg_fill = self.accent;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);

        visuals.selection.bg_fill = self.accent.gamma_multiply(0.55);
        visuals.selection.stroke = Stroke::new(1.0, self.text);

        ctx.set_visuals(visuals);
    }
}
