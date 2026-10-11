//! What is installed on this machine, for pickers instead of free text:
//! xkb layouts and options, cursor themes and desktop apps.
//!
//! Everything here only reads files, which the sandbox allows anywhere, and
//! everything degrades to an empty list: the plain text field stays usable.
//! The browser demo has no system to read and uses `demo::system` instead.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// One xkb layout or option: `de` / German.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XkbItem {
    pub code: String,
    pub description: String,
    /// For options: what the group is about (`Switching to another layout`).
    pub group: String,
}

impl std::fmt::Display for XkbItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.group.is_empty() {
            write!(f, "{} ({})", self.description, self.code)
        } else {
            write!(f, "{}: {} ({})", self.group, self.description, self.code)
        }
    }
}

/// An installed app from its `.desktop` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    /// Its desktop file's name without `.desktop`: usually the app ID its
    /// windows have (`dev.zed.Zed`).
    pub id: String,
    pub name: String,
    /// `Exec` without the `%f`-style field codes.
    pub command: String,
}

impl std::fmt::Display for InstalledApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.command)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SystemData {
    pub layouts: Vec<XkbItem>,
    pub options: Vec<XkbItem>,
    pub cursor_themes: Vec<String>,
    pub apps: Vec<InstalledApp>,
    /// The names of all `.desktop` files, hidden ones too, in lower case:
    /// an app's Wayland app ID is usually its desktop file's name.
    pub desktop_ids: BTreeSet<String>,
}

impl SystemData {
    pub fn load() -> Self {
        let (layouts, options) = std::fs::read_to_string(xkb_rules())
            .map(|xml| parse_xkb_rules(&xml))
            .unwrap_or_default();
        let (apps, desktop_ids) = desktop_files(&application_dirs());
        Self {
            layouts,
            options,
            cursor_themes: cursor_themes(&icon_dirs()),
            apps,
            desktop_ids,
        }
    }

    /// Whether an app with this app ID (or desktop file name) is installed.
    /// What people call the app whose windows have `app_id`, from its
    /// desktop file.
    pub fn app_name(&self, app_id: &str) -> Option<&str> {
        self.apps
            .iter()
            .find(|app| app.id.eq_ignore_ascii_case(app_id))
            .map(|app| app.name.as_str())
    }

    pub fn has_app(&self, id: &str) -> bool {
        self.desktop_ids.contains(&id.to_ascii_lowercase())
    }

    pub fn layout_name(&self, code: &str) -> Option<&str> {
        self.layouts
            .iter()
            .find(|item| item.code == code)
            .map(|item| item.description.as_str())
    }

    pub fn option_name(&self, code: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|item| item.code == code)
            .map(|item| item.description.as_str())
    }
}

fn xkb_rules() -> PathBuf {
    std::env::var_os("XKB_CONFIG_ROOT")
        .map_or_else(|| PathBuf::from("/usr/share/X11/xkb"), PathBuf::from)
        .join("rules/evdev.xml")
}

/// Layouts and options from xkeyboard-config's `evdev.xml`.
pub fn parse_xkb_rules(xml: &str) -> (Vec<XkbItem>, Vec<XkbItem>) {
    let mut layouts = Vec::new();
    let mut options = Vec::new();
    // tag names from the root to the current element
    let mut stack: Vec<String> = Vec::new();
    let mut name = String::new();
    let mut group = String::new();
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        let text = &rest[..open];
        rest = &rest[open..];
        let Some(close) = rest.find('>') else { break };
        let tag = &rest[1..close];
        rest = &rest[close + 1..];
        if !text.trim().is_empty() {
            let text = unescape(text.trim());
            let path: Vec<&str> = stack.iter().map(String::as_str).collect();
            match path.as_slice() {
                [.., "layout", "configItem", "name"]
                | [.., "option", "configItem", "name"]
                | [.., "group", "configItem", "name"] => name = text,
                [.., "layout", "configItem", "description"] => layouts.push(XkbItem {
                    code: name.clone(),
                    description: text,
                    group: String::new(),
                }),
                [.., "group", "configItem", "description"] => group = text,
                [.., "option", "configItem", "description"] => options.push(XkbItem {
                    code: name.clone(),
                    description: text,
                    group: group.clone(),
                }),
                _ => {}
            }
        }
        if tag.starts_with('?') || tag.starts_with('!') || tag.ends_with('/') {
            continue;
        }
        if let Some(closing) = tag.strip_prefix('/') {
            if let Some(index) = stack.iter().rposition(|open| open == closing) {
                stack.truncate(index);
            }
        } else {
            let element = tag.split_whitespace().next().unwrap_or_default();
            stack.push(element.to_string());
        }
    }
    layouts.sort_by(|a, b| a.description.cmp(&b.description));
    (layouts, options)
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Where cursor themes live, in libXcursor's search order.
fn icon_dirs() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("XCURSOR_PATH") {
        return std::env::split_paths(&path).collect();
    }
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".local/share/icons"));
        dirs.push(home.join(".icons"));
    }
    dirs.extend(data_dirs().into_iter().map(|dir| dir.join("icons")));
    dirs.push(PathBuf::from("/usr/share/pixmaps"));
    dirs
}

fn data_dirs() -> Vec<PathBuf> {
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    std::env::split_paths(&dirs).collect()
}

/// Theme folders that hold a `cursors` folder, by name.
pub fn cursor_themes(dirs: &[PathBuf]) -> Vec<String> {
    let mut themes = BTreeSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().join("cursors").is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                themes.insert(name.to_string());
            }
        }
    }
    themes.into_iter().collect()
}

fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.extend(crate::xdg::base("XDG_DATA_HOME", ".local/share"));
    dirs.extend(data_dirs());
    dirs.into_iter()
        .map(|dir| dir.join("applications"))
        .collect()
}

/// What the `.desktop` files in `dirs` offer, in one pass over them: the
/// apps that show in launchers, by name (an earlier folder wins), and the
/// names of all the files, hidden ones too, without the extension.
pub fn desktop_files(dirs: &[PathBuf]) -> (Vec<InstalledApp>, BTreeSet<String>) {
    let mut ids = BTreeSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "desktop"))
            .collect();
        files.sort();
        for file in files {
            let Some(id) = file.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if !ids.insert(id.to_string()) {
                continue;
            }
            if let Some(app) = std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| parse_desktop_entry(&text))
            {
                apps.push(InstalledApp {
                    id: id.to_string(),
                    ..app
                });
            }
        }
    }
    apps.sort_by_cached_key(|app| app.name.to_lowercase());
    let ids = ids.iter().map(|id| id.to_ascii_lowercase()).collect();
    (apps, ids)
}

/// The `[Desktop Entry]` of a launcher file, unless it hides itself.
pub fn parse_desktop_entry(text: &str) -> Option<InstalledApp> {
    let mut in_entry = false;
    let (mut name, mut exec) = (None, None);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Name" => name = Some(value.trim().to_string()),
            "Exec" => exec = Some(value.trim().to_string()),
            "Type" if value.trim() != "Application" => return None,
            "NoDisplay" | "Hidden" if value.trim() == "true" => return None,
            _ => {}
        }
    }
    let command = strip_field_codes(&exec?);
    (!command.is_empty()).then(|| InstalledApp {
        id: String::new(),
        name: name.unwrap_or_else(|| command.clone()),
        command,
    })
}

/// `firefox %u` → `firefox`: drop the codes launchers fill in.
fn strip_field_codes(exec: &str) -> String {
    let mut words = Vec::new();
    for word in exec.split_whitespace() {
        if word.len() == 2 && word.starts_with('%') && word != "%%" {
            continue;
        }
        words.push(word.replace("%%", "%"));
    }
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"<?xml version="1.0"?>
<!DOCTYPE xkbConfigRegistry SYSTEM "xkb.dtd">
<xkbConfigRegistry version="1.1">
  <layoutList>
    <layout>
      <configItem>
        <name>de</name>
        <shortDescription>de</shortDescription>
        <description>German</description>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>nodeadkeys</name>
            <description>German (no dead keys)</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem>
        <name>us</name>
        <description>English (US)</description>
      </configItem>
    </layout>
  </layoutList>
  <optionList>
    <group allowMultipleSelection="true">
      <configItem>
        <name>grp</name>
        <description>Switching to another layout</description>
      </configItem>
      <option>
        <configItem>
          <name>grp:caps_toggle</name>
          <description>Caps Lock</description>
        </configItem>
      </option>
    </group>
    <group>
      <configItem>
        <name>ctrl</name>
        <description>Ctrl position</description>
      </configItem>
      <option>
        <configItem>
          <name>ctrl:nocaps</name>
          <description>Caps Lock as Ctrl &amp; more</description>
        </configItem>
      </option>
    </group>
  </optionList>
</xkbConfigRegistry>"#;

    #[test]
    fn layouts_and_options_come_from_the_rules_file() {
        let (layouts, options) = parse_xkb_rules(RULES);
        let codes: Vec<&str> = layouts.iter().map(|l| l.code.as_str()).collect();
        // variants are not layouts
        assert_eq!(codes, ["us", "de"]);
        assert_eq!(layouts[1].to_string(), "German (de)");
        assert_eq!(options.len(), 2);
        assert_eq!(
            options[0].to_string(),
            "Switching to another layout: Caps Lock (grp:caps_toggle)"
        );
        assert_eq!(options[1].description, "Caps Lock as Ctrl & more");
    }

    #[test]
    fn an_app_is_named_by_the_desktop_file_its_app_id_names() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("dev.zed.Zed.desktop"),
            "[Desktop Entry]\nType=Application\nName=Zed\nExec=zeditor %U\n",
        )
        .unwrap();
        let (apps, desktop_ids) = desktop_files(&[dir.path().to_path_buf()]);
        let system = SystemData {
            apps,
            desktop_ids,
            ..SystemData::default()
        };
        assert_eq!(system.app_name("dev.zed.Zed"), Some("Zed"));
        assert_eq!(system.app_name("DEV.ZED.ZED"), Some("Zed"));
        assert_eq!(system.app_name("kitty"), None);
    }

    #[test]
    fn launchers_drop_field_codes_and_hidden_entries() {
        let app = parse_desktop_entry(
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n[Desktop Action new]\nName=New\nExec=firefox --new-window %u\n",
        )
        .unwrap();
        assert_eq!(app.name, "Firefox");
        assert_eq!(app.command, "firefox");
        assert!(
            parse_desktop_entry("[Desktop Entry]\nName=Helper\nExec=helper\nNoDisplay=true\n")
                .is_none()
        );
    }

    #[test]
    fn cursor_themes_are_folders_with_cursors() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("Adwaita/cursors")).unwrap();
        std::fs::create_dir_all(dir.path().join("hicolor/48x48")).unwrap();
        assert_eq!(cursor_themes(&[dir.path().to_path_buf()]), ["Adwaita"]);
    }
}
