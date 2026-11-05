//! The desktop's side of things, through xdg-desktop-portal on the session
//! bus: the file chooser behind Browse buttons, and the color scheme and
//! contrast the desktop asks for.
//!
//! On niri the chooser usually comes from the gtk portal, or from nautilus
//! for the gnome one; when neither answers, the text field next to the
//! button still works and the status line says why the dialog did not open.
//! Interfaces as documented at
//! <https://flatpak.github.io/xdg-desktop-portal/docs/>.

/// The light or dark the desktop asks apps for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    NoPreference,
    Dark,
    Light,
}

/// One thing the desktop asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Scheme(Scheme),
    /// Whether it asks for more contrast.
    HighContrast(bool),
}

/// Why the file chooser gave no path.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ChooseError {
    #[cfg(not(target_arch = "wasm32"))]
    #[error(
        "The file chooser did not open ({0}). It needs xdg-desktop-portal with a file chooser, such as the gtk portal. You can type the path instead."
    )]
    DidNotOpen(#[from] bus::Failure),
    #[cfg(not(target_arch = "wasm32"))]
    #[error(
        "The file chooser picked {uri}, which is not a path niri-center can use. You can type the path instead."
    )]
    NotAPath { uri: String },
    #[cfg(target_arch = "wasm32")]
    #[error("The file chooser opens in the desktop app. In this demo, type the path.")]
    NoDesktop,
}

#[cfg(not(target_arch = "wasm32"))]
mod bus {
    use std::collections::HashMap;

    use futures_util::StreamExt;
    use zbus::proxy::{Builder, CacheProperties, SignalStream};
    use zbus::{Connection, OwnedObjectPath, OwnedVariant, Proxy, Variant};

    use super::{Appearance, Scheme};

    const PORTAL: &str = "org.freedesktop.portal.Desktop";
    const DESKTOP: &str = "/org/freedesktop/portal/desktop";
    const APPEARANCE: &str = "org.freedesktop.appearance";
    // every `choose` has a connection of its own, whose name is in the
    // request's path, so one token cannot answer for another request
    const HANDLE_TOKEN: &str = "niri_center";

    /// What went wrong asking the portal for a file.
    #[derive(Debug, Clone, thiserror::Error)]
    pub enum Failure {
        #[error(transparent)]
        Bus(#[from] zbus::Error),
        #[error("no name on the session bus")]
        NoBusName,
        #[error("the portal closed without an answer")]
        NoAnswer,
        #[error("the chooser answered without uris")]
        NoUris,
        #[error("the chooser ended without a choice")]
        Ended,
    }

    /// The portal interfaces the app calls.
    #[derive(Debug, Clone, Copy)]
    enum Interface {
        Settings,
        FileChooser,
        /// One call's answer, at the handle the call returns.
        Request,
    }

    impl Interface {
        const fn as_str(self) -> &'static str {
            match self {
                Self::Settings => "org.freedesktop.portal.Settings",
                Self::FileChooser => "org.freedesktop.portal.FileChooser",
                Self::Request => "org.freedesktop.portal.Request",
            }
        }
    }

    /// `interface` of the portal object at `path`. Request objects exist only
    /// for one call, so nothing is read from them ahead.
    async fn proxy<'path>(
        connection: &Connection,
        path: &'path str,
        interface: Interface,
    ) -> zbus::Result<Proxy<'path>> {
        Builder::<Proxy>::new(connection)
            .destination(PORTAL)
            .path(path)
            .interface(interface.as_str())
            .cache_properties(CacheProperties::No)
            .build()
            .await
    }

    /// The number inside `variant`: the portal's deprecated `Read` wraps it
    /// in a second variant.
    fn number(variant: &Variant<'_>) -> Option<u32> {
        if let Variant::U32(number) = variant {
            return Some(*number);
        }
        if let Variant::Variant(inner) = variant {
            return number(inner);
        }
        None
    }

    /// `org.freedesktop.appearance` `color-scheme`: "0: No preference,
    /// 1: Prefer dark appearance, 2: Prefer light appearance".
    const fn scheme_of(value: u32) -> Scheme {
        match value {
            1 => Scheme::Dark,
            2 => Scheme::Light,
            _ => Scheme::NoPreference,
        }
    }

    /// One `org.freedesktop.appearance` setting; `None` without an answer.
    async fn appearance(settings: &Proxy<'_>, key: &str) -> Option<u32> {
        // ReadOne came with version 2 of the interface
        let read: zbus::Result<OwnedVariant> = settings.call("ReadOne", &(APPEARANCE, key)).await;
        let value = match read {
            Ok(value) => value,
            Err(_) => settings.call("Read", &(APPEARANCE, key)).await.ok()?,
        };
        number(&value)
    }

    /// `contrast`: "0: No preference (normal contrast), 1: Higher contrast".
    pub async fn watch<Handed: Future<Output = bool>>(mut each: impl FnMut(Appearance) -> Handed) {
        let Ok(connection) = Connection::session().await else {
            return;
        };
        let Ok(settings) = proxy(&connection, DESKTOP, Interface::Settings).await else {
            return;
        };
        // listening before the first read, so a change during it is not lost
        // and cannot be overwritten by what the read saw
        let Ok(mut changes) = settings
            .receive_signal_with_args("SettingChanged", &[(0, APPEARANCE)])
            .await
        else {
            return;
        };
        let scheme = appearance(&settings, "color-scheme")
            .await
            .map_or(Scheme::NoPreference, scheme_of);
        if !each(Appearance::Scheme(scheme)).await {
            return;
        }
        let high_contrast = appearance(&settings, "contrast").await == Some(1);
        if !each(Appearance::HighContrast(high_contrast)).await {
            return;
        }
        while let Some(change) = changes.next().await {
            let Ok((_, key, value)) = change
                .body()
                .deserialize::<(String, String, OwnedVariant)>()
            else {
                continue;
            };
            let appearance = match (key.as_str(), number(&value)) {
                ("color-scheme", Some(value)) => Appearance::Scheme(scheme_of(value)),
                ("contrast", Some(value)) => Appearance::HighContrast(value == 1),
                _ => continue,
            };
            if !each(appearance).await {
                break;
            }
        }
    }

    /// The `Response` signals of the request object at `request_path`.
    async fn answers_at(
        connection: &Connection,
        request_path: &str,
    ) -> zbus::Result<SignalStream<'static>> {
        proxy(connection, request_path, Interface::Request)
            .await?
            .receive_signal("Response")
            .await
    }

    /// The first file in a successful answer; none when it lists no files.
    fn first_uri(results: &HashMap<String, OwnedVariant>) -> Result<Option<String>, Failure> {
        let uris = results.get("uris").ok_or(Failure::NoUris)?;
        let uris = Vec::<String>::try_from(Variant::clone(uris))?;
        Ok(uris.into_iter().next())
    }

    /// What the chooser answered: the file's URI, or none when cancelled.
    pub async fn choose(title: &str, folder: bool) -> Result<Option<String>, Failure> {
        let connection = Connection::session().await?;
        // the request's object is known before the call, so its answer is
        // listened for first and cannot come before it
        let sender = connection
            .unique_name()
            .ok_or(Failure::NoBusName)?
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_");
        let expected = format!("{DESKTOP}/request/{sender}/{HANDLE_TOKEN}");
        let mut answers = answers_at(&connection, &expected).await?;

        let chooser = proxy(&connection, DESKTOP, Interface::FileChooser).await?;
        let options = HashMap::from([
            ("handle_token", Variant::from(HANDLE_TOKEN)),
            ("modal", Variant::from(true)),
            ("directory", Variant::from(folder)),
        ]);
        let handle: OwnedObjectPath = chooser.call("OpenFile", &("", title, options)).await?;
        // portals before the handle was predictable choose their own
        if handle.as_str() != expected {
            answers = answers_at(&connection, handle.as_str()).await?;
        }
        let answer = answers.next().await.ok_or(Failure::NoAnswer)?;
        let (response, results) = answer
            .body()
            .deserialize::<(u32, HashMap<String, OwnedVariant>)>()?;
        // "0: Success, 1: The user cancelled the interaction, 2: The user
        // interaction was ended in some other way"
        match response {
            0 => first_uri(&results),
            1 => Ok(None),
            _ => Err(Failure::Ended),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_setting_is_read_through_either_wrapping() {
            // ReadOne answers with the value; the older Read wraps it again
            assert_eq!(number(&Variant::from(1u32)), Some(1));
            let wrapped = Variant::new(Variant::new(Variant::new(2u32)));
            assert_eq!(number(&wrapped), Some(2));
            assert_eq!(number(&Variant::from("dark")), None);
        }

        fn answer_with_uris(uris: Variant<'_>) -> HashMap<String, OwnedVariant> {
            HashMap::from([(
                "uris".to_string(),
                OwnedVariant::try_from(uris).expect("no file descriptor to clone"),
            )])
        }

        #[test]
        fn a_chooser_answer_gives_its_first_file() {
            let answer = answer_with_uris(Variant::from(vec!["file:///first", "file:///second"]));
            assert_eq!(
                first_uri(&answer).ok(),
                Some(Some("file:///first".to_string()))
            );
            let no_files = answer_with_uris(Variant::from(Vec::<&str>::new()));
            assert_eq!(first_uri(&no_files).ok(), Some(None));
        }

        #[test]
        fn a_chooser_answer_without_a_list_of_uris_is_no_cancel() {
            assert!(matches!(first_uri(&HashMap::new()), Err(Failure::NoUris)));
            assert!(matches!(
                first_uri(&answer_with_uris(Variant::from(7u32))),
                Err(Failure::Bus(_))
            ));
        }

        #[test]
        fn color_scheme_numbers_are_the_portals() {
            assert_eq!(scheme_of(0), Scheme::NoPreference);
            assert_eq!(scheme_of(1), Scheme::Dark);
            assert_eq!(scheme_of(2), Scheme::Light);
            assert_eq!(scheme_of(7), Scheme::NoPreference);
        }
    }
}

/// Ask the user for a file, or a folder when `folder`; `Ok(None)` when they
/// cancelled, an error when what they picked has no path here. The path comes
/// back with the home folder as `~`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn choose(folder: bool) -> Result<Option<String>, ChooseError> {
    let title = if folder {
        "Choose a folder"
    } else {
        "Choose a file"
    };
    let Some(uri) = bus::choose(title, folder).await? else {
        return Ok(None);
    };
    file_path(&uri)
        .map(|path| Some(crate::store::tilde(std::path::Path::new(&path))))
        .ok_or(ChooseError::NotAPath { uri })
}

/// The browser demo has no file chooser to ask.
#[cfg(target_arch = "wasm32")]
pub async fn choose(_folder: bool) -> Result<Option<String>, ChooseError> {
    Err(ChooseError::NoDesktop)
}

/// `file:///home/me/My%20Pictures` → `/home/me/My Pictures`.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn file_path(uri: &str) -> Option<String> {
    let encoded = uri.strip_prefix("file://")?;
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%'
            && let Some(decoded) = encoded
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(decoded);
            index += 3;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The browser's `prefers-contrast: more`, for the demo.
#[cfg(target_arch = "wasm32")]
pub async fn high_contrast() -> bool {
    web_sys::window()
        .and_then(|window| {
            window
                .match_media("(prefers-contrast: more)")
                .ok()
                .flatten()
        })
        .is_some_and(|query| query.matches())
}

/// Hand the desktop's color scheme and contrast (GNOME's high-contrast switch
/// sets it) to `each`, then every later change of them, until `each` says
/// stop. Without a portal's answer: no preference and no more contrast; without
/// a session bus, nothing.
#[cfg(not(target_arch = "wasm32"))]
pub async fn watch_appearance<Handed: Future<Output = bool>>(
    each: impl FnMut(Appearance) -> Handed,
) {
    bus::watch(each).await;
}

/// The browser's preferences are read once, at start.
#[cfg(target_arch = "wasm32")]
pub async fn watch_appearance<Handed: Future<Output = bool>>(
    _each: impl FnMut(Appearance) -> Handed,
) {
}

#[cfg(test)]
mod tests {
    use super::{ChooseError, bus};

    #[test]
    fn the_status_line_says_why_the_chooser_gave_no_path() {
        assert_eq!(
            ChooseError::from(bus::Failure::NoAnswer).to_string(),
            "The file chooser did not open (the portal closed without an answer). It needs xdg-desktop-portal with a file chooser, such as the gtk portal. You can type the path instead."
        );
        assert_eq!(
            ChooseError::NotAPath {
                uri: "https://example.org".to_string()
            }
            .to_string(),
            "The file chooser picked https://example.org, which is not a path niri-center can use. You can type the path instead."
        );
    }

    #[test]
    fn portal_uris_become_paths() {
        assert_eq!(
            super::file_path("file:///home/me/My%20Pictures").as_deref(),
            Some("/home/me/My Pictures")
        );
        assert_eq!(super::file_path("https://example.org"), None);
    }
}
