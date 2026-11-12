//! Guild folders and drag-free rail ordering.
//!
//! Discord keeps a user's server folders in their own account settings, which a
//! bot token cannot read or write, so the layout lives in `settings.json`
//! (`DECISIONS.md` D32). It is deliberately a flat order plus folder membership
//! rather than a nested tree: leaving or joining a server then never requires
//! rewriting the structure, and ids that no longer exist are ignored when the
//! rail is built.
//!
//! Ordering is drag-free: the UI moves a guild (or a whole folder) one slot at a
//! time, and every operation here rebuilds the flat order so it stays canonical.
//! Pure and tested without a UI.

use crate::models::Guild;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildFolder {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub guilds: Vec<String>,
    /// `#rrggbb`, or `None` for the theme's own surface. A folder created from a
    /// server gets the next palette colour so groups are distinguishable at once.
    #[serde(default)]
    pub color: Option<String>,
}

/// Discord-ish folder colours, in the order new folders pick them up.
pub const FOLDER_COLORS: [(&str, &str); 8] = [
    ("Blurple", "#5865f2"),
    ("Green", "#3ba55d"),
    ("Yellow", "#faa81a"),
    ("Red", "#ed4245"),
    ("Pink", "#eb459e"),
    ("Purple", "#9b59b6"),
    ("Teal", "#1abc9c"),
    ("Orange", "#e67e22"),
];

impl GuildFolder {
    pub fn contains(&self, guild_id: &str) -> bool {
        self.guilds.iter().any(|id| id == guild_id)
    }
}

/// An in-progress rename in the UI. Session-only: the draft never reaches
/// `settings.json` unless it is saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRename {
    pub folder_id: String,
    pub draft: String,
    /// Ask for keyboard focus on the frame the dialog first appears.
    pub focus: bool,
}

impl FolderRename {
    pub fn new(folder: &GuildFolder) -> Self {
        Self {
            folder_id: folder.id.clone(),
            draft: folder.name.clone(),
            focus: true,
        }
    }
}

/// One tile in the rail: a guild, or a folder standing in for its members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailItem {
    Guild(String),
    Folder(GuildFolder),
}

impl RailItem {
    pub fn contains(&self, guild_id: &str) -> bool {
        match self {
            Self::Guild(id) => id == guild_id,
            Self::Folder(folder) => folder.contains(guild_id),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildRail {
    /// Guild ids in rail order. Missing ids are appended in received order.
    pub order: Vec<String>,
    pub folders: Vec<GuildFolder>,
}

impl GuildRail {
    pub fn is_default(&self) -> bool {
        self.order.is_empty() && self.folders.is_empty()
    }

    /// Stored order with unknown ids dropped and new guilds appended, so the rail
    /// always shows every server exactly once.
    fn normalized_order(&self, guilds: &[Guild]) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        for id in &self.order {
            if guilds.iter().any(|guild| &guild.id == id) && !order.contains(id) {
                order.push(id.clone());
            }
        }
        for guild in guilds {
            if !order.contains(&guild.id) {
                order.push(guild.id.clone());
            }
        }
        order
    }

    /// The rail as it should be drawn: a folder appears where its first member
    /// sits, and only once.
    pub fn items(&self, guilds: &[Guild]) -> Vec<RailItem> {
        let order = self.normalized_order(guilds);
        let mut items = Vec::new();
        let mut emitted: Vec<String> = Vec::new();

        for id in &order {
            let Some(folder) = self.folders.iter().find(|folder| folder.contains(id)) else {
                items.push(RailItem::Guild(id.clone()));
                continue;
            };
            if emitted.contains(&folder.id) {
                continue;
            }
            emitted.push(folder.id.clone());

            // Members that left the guild disappear from the folder tile.
            let members: Vec<String> = order
                .iter()
                .filter(|candidate| folder.contains(candidate))
                .cloned()
                .collect();
            if members.is_empty() {
                continue;
            }
            let mut shown = folder.clone();
            shown.guilds = members;
            items.push(RailItem::Folder(shown));
        }

        items
    }

    pub fn folder_of(&self, guild_id: &str) -> Option<&GuildFolder> {
        self.folders.iter().find(|folder| folder.contains(guild_id))
    }

    /// Move a guild, or the folder that holds it, one slot up or down.
    pub fn move_guild(&mut self, guilds: &[Guild], guild_id: &str, delta: i32) -> bool {
        let mut items = self.items(guilds);
        let Some(index) = items.iter().position(|item| item.contains(guild_id)) else {
            return false;
        };
        let target = index as i32 + delta;
        if target < 0 || target >= items.len() as i32 {
            return false;
        }
        items.swap(index, target as usize);
        self.order = flatten(&items);
        true
    }

    pub fn move_folder(&mut self, guilds: &[Guild], folder_id: &str, delta: i32) -> bool {
        let Some(member) = self
            .folders
            .iter()
            .find(|folder| folder.id == folder_id)
            .and_then(|folder| folder.guilds.first().cloned())
        else {
            return false;
        };
        self.move_guild(guilds, &member, delta)
    }

    /// Remove a guild from whatever folder holds it, dropping folders that end up
    /// empty so nothing invisible lingers in the config.
    fn detach(&mut self, guild_id: &str) {
        for folder in &mut self.folders {
            folder.guilds.retain(|id| id != guild_id);
        }
        self.folders.retain(|folder| !folder.guilds.is_empty());
    }

    /// Start a folder named after `name` holding just this guild. Returns its id.
    pub fn new_folder(&mut self, guild_id: &str, name: &str) -> String {
        self.detach(guild_id);

        let mut index = self.folders.len();
        let id = loop {
            let candidate = format!("f{index}");
            if !self.folders.iter().any(|folder| folder.id == candidate) {
                break candidate;
            }
            index += 1;
        };

        let name = name.trim();
        let name = if name.is_empty() { "Folder" } else { name };
        let color = FOLDER_COLORS[self.folders.len() % FOLDER_COLORS.len()]
            .1
            .to_string();
        self.folders.push(GuildFolder {
            id: id.clone(),
            name: name.to_string(),
            guilds: vec![guild_id.to_string()],
            color: Some(color),
        });
        // `order` is left alone: `normalized_order` appends guilds it does not
        // mention in received order, so a folder never jumps to the top.
        id
    }

    /// Move a guild into `folder_id`, out of any folder it was in before.
    pub fn add_to_folder(&mut self, guild_id: &str, folder_id: &str) -> bool {
        if !self.folders.iter().any(|folder| folder.id == folder_id) {
            return false;
        }
        for folder in self.folders.iter_mut().filter(|f| f.id != folder_id) {
            folder.guilds.retain(|id| id != guild_id);
        }
        self.folders
            .retain(|folder| folder.id == folder_id || !folder.guilds.is_empty());

        let Some(folder) = self.folders.iter_mut().find(|f| f.id == folder_id) else {
            return false;
        };
        if !folder.contains(guild_id) {
            folder.guilds.push(guild_id.to_string());
        }
        true
    }

    pub fn remove_from_folder(&mut self, guild_id: &str) -> bool {
        let before = self.folders.len();
        let held = self.folder_of(guild_id).is_some();
        self.detach(guild_id);
        held || self.folders.len() != before
    }

    /// Rename a folder. Blank names are refused rather than silently defaulted,
    /// so the dialog can keep the field open and explain itself.
    pub fn rename_folder(&mut self, folder_id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let Some(folder) = self
            .folders
            .iter_mut()
            .find(|folder| folder.id == folder_id)
        else {
            return false;
        };
        folder.name = name.to_string();
        true
    }

    /// Set or clear a folder's colour. Values are normalised to `#rrggbb`; an
    /// unparsable value is refused so a typo in `settings.json` cannot paint a
    /// folder black by accident.
    pub fn set_folder_color(&mut self, folder_id: &str, color: Option<&str>) -> bool {
        let parsed = match color {
            Some(value) => match crate::utils::parse_css_color(value) {
                Some(color) => Some(crate::utils::color_to_hex(color)),
                None => return false,
            },
            None => None,
        };
        let Some(folder) = self
            .folders
            .iter_mut()
            .find(|folder| folder.id == folder_id)
        else {
            return false;
        };
        folder.color = parsed;
        true
    }

    /// Ungroup: members return to the rail as individual servers in place.
    pub fn delete_folder(&mut self, folder_id: &str) -> bool {
        let before = self.folders.len();
        self.folders.retain(|folder| folder.id != folder_id);
        self.folders.len() != before
    }
}

fn flatten(items: &[RailItem]) -> Vec<String> {
    items
        .iter()
        .flat_map(|item| match item {
            RailItem::Guild(id) => vec![id.clone()],
            RailItem::Folder(folder) => folder.guilds.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guilds(ids: &[&str]) -> Vec<Guild> {
        ids.iter()
            .map(|id| Guild {
                id: id.to_string(),
                name: format!("guild {id}"),
                icon: None,
            })
            .collect()
    }

    fn ids(items: &[RailItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                RailItem::Guild(id) => id.clone(),
                RailItem::Folder(folder) => format!("[{}]", folder.name),
            })
            .collect()
    }

    #[test]
    fn an_unconfigured_rail_is_received_order() {
        let rail = GuildRail::default();
        let guilds = guilds(&["1", "2", "3"]);
        assert!(rail.is_default());
        assert_eq!(ids(&rail.items(&guilds)), vec!["1", "2", "3"]);
    }

    #[test]
    fn stale_and_duplicate_ids_are_dropped() {
        let rail = GuildRail {
            order: vec!["2".into(), "gone".into(), "1".into(), "2".into()],
            folders: Vec::new(),
        };
        let guilds = guilds(&["1", "2", "3"]);
        assert_eq!(ids(&rail.items(&guilds)), vec!["2", "1", "3"]);
    }

    #[test]
    fn a_folder_replaces_its_members_once_at_the_first_member() {
        let mut rail = GuildRail::default();
        let guilds = guilds(&["1", "2", "3", "4"]);
        let folder = rail.new_folder("2", "Games");
        assert!(rail.add_to_folder("4", &folder));

        let items = rail.items(&guilds);
        assert_eq!(ids(&items), vec!["1", "[Games]", "3"]);
        match &items[1] {
            RailItem::Folder(shown) => {
                assert_eq!(shown.guilds, vec!["2".to_string(), "4".to_string()])
            }
            _ => panic!("expected a folder"),
        }
    }

    #[test]
    fn moving_a_member_moves_the_whole_folder_block() {
        let mut rail = GuildRail::default();
        let guilds = guilds(&["1", "2", "3"]);
        let folder = rail.new_folder("2", "Games");
        rail.add_to_folder("3", &folder);

        assert!(rail.move_guild(&guilds, "3", -1));
        assert_eq!(ids(&rail.items(&guilds)), vec!["[Games]", "1"]);
        assert_eq!(rail.order, vec!["2", "3", "1"]);

        assert!(!rail.move_guild(&guilds, "2", -1), "already at the top");
        assert!(!rail.move_guild(&guilds, "1", 1), "1 is last");
    }

    #[test]
    fn folders_can_be_renamed_but_not_blanked() {
        let mut rail = GuildRail::default();
        let folder = rail.new_folder("1", "Old");

        assert!(rail.rename_folder(&folder, "  New name  "));
        assert_eq!(rail.folder_of("1").unwrap().name, "New name");

        assert!(!rail.rename_folder(&folder, "   "), "blank is refused");
        assert_eq!(rail.folder_of("1").unwrap().name, "New name");

        assert!(!rail.rename_folder("missing", "x"));
        assert_eq!(
            rail.items(&guilds(&["1"]))[0],
            RailItem::Folder(rail.folders[0].clone())
        );
    }

    #[test]
    fn a_rename_draft_starts_from_the_current_name() {
        let mut rail = GuildRail::default();
        rail.new_folder("1", "Games");
        let draft = FolderRename::new(&rail.folders[0]);
        assert_eq!(draft.folder_id, "f0");
        assert_eq!(draft.draft, "Games");
        assert!(draft.focus);
    }

    #[test]
    fn a_guild_moves_between_folders_and_empty_ones_vanish() {
        let mut rail = GuildRail::default();
        let first = rail.new_folder("1", "First");
        let second = rail.new_folder("2", "Second");
        assert_eq!(rail.folders.len(), 2);

        assert!(rail.add_to_folder("1", &second));
        assert_eq!(rail.folder_of("1").unwrap().id, second);
        assert_eq!(rail.folder_of("2").unwrap().id, second);
        assert_eq!(rail.folders.len(), 1, "the emptied First folder is gone");

        assert!(rail.remove_from_folder("2"));
        assert!(rail.folder_of("2").is_none());
        assert_eq!(rail.folders.len(), 1, "the folder still holds 1");
        assert!(rail.remove_from_folder("1"));
        assert!(rail.folders.is_empty());
        assert!(!rail.add_to_folder("2", &first), "folder no longer exists");
    }

    #[test]
    fn folder_ids_do_not_repeat_after_deletion() {
        let mut rail = GuildRail::default();
        let first = rail.new_folder("1", "a");
        let second = rail.new_folder("2", "b");
        assert_ne!(first, second);

        assert!(rail.delete_folder(&first));
        assert!(!rail.delete_folder(&first));
        let third = rail.new_folder("3", "c");
        assert_ne!(third, first);
        assert_ne!(third, second);
    }

    #[test]
    fn folder_names_default_and_trim() {
        let mut rail = GuildRail::default();
        let blank = rail.new_folder("1", "   ");
        assert_eq!(rail.folder_of("1").unwrap().name, "Folder");
        let named = rail.new_folder("2", "  Games  ");
        assert_eq!(rail.folder_of("2").unwrap().name, "Games");
        assert_ne!(blank, named);
    }

    #[test]
    fn empty_folders_are_not_drawn_and_survive_a_missing_guild() {
        let mut rail = GuildRail::default();
        let folder = rail.new_folder("1", "Solo");
        let guilds = guilds(&["2", "3"]);
        assert_eq!(ids(&rail.items(&guilds)), vec!["2", "3"]);
        assert_eq!(rail.folders.len(), 1, "the folder is kept, not deleted");
        assert_eq!(rail.folders[0].id, folder);
    }

    #[test]
    fn new_folders_get_distinct_palette_colours() {
        let mut rail = GuildRail::default();
        rail.new_folder("1", "One");
        rail.new_folder("2", "Two");

        let first = rail.folder_of("1").unwrap().color.clone().expect("colour");
        let second = rail.folder_of("2").unwrap().color.clone().expect("colour");
        assert!(FOLDER_COLORS.iter().any(|(_, hex)| *hex == first));
        assert_ne!(first, second, "neighbouring folders are distinguishable");

        // Wrapping past the palette repeats rather than running out.
        for index in 3..12 {
            rail.new_folder(&index.to_string(), "More");
        }
        assert!(rail.folders.iter().all(|folder| folder.color.is_some()));
    }

    #[test]
    fn folder_colours_can_be_set_cleared_and_validated() {
        let mut rail = GuildRail::default();
        let folder = rail.new_folder("1", "One");

        assert!(rail.set_folder_color(&folder, Some("#5865F2")));
        assert_eq!(
            rail.folder_of("1").unwrap().color.as_deref(),
            Some("#5865f2"),
            "stored normalised"
        );

        assert!(rail.set_folder_color(&folder, None));
        assert_eq!(rail.folder_of("1").unwrap().color, None);

        assert!(!rail.set_folder_color(&folder, Some("not a colour")));
        assert_eq!(
            rail.folder_of("1").unwrap().color,
            None,
            "invalid left it alone"
        );
        assert!(!rail.set_folder_color("missing", Some("#3ba55d")));
    }

    #[test]
    fn layout_roundtrips_through_json() {
        let mut rail = GuildRail::default();
        let folder = rail.new_folder("2", "Games");
        rail.add_to_folder("1", &folder);
        rail.move_guild(&guilds(&["1", "2", "3"]), "3", -1);

        let text = serde_json::to_string(&rail).expect("serialize");
        let loaded: GuildRail = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(loaded, rail);
        assert!(serde_json::from_str::<GuildRail>("{}")
            .unwrap()
            .is_default());
    }
}
