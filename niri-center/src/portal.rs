//! The desktop's side of things, through xdg-desktop-portal on the session
//! bus: the file chooser behind Browse buttons, and whether the desktop
//! asks for more contrast.
//!
//! On niri the chooser usually comes from the gtk portal, or from nautilus
//! for the gnome one; when neither answers, the text field next to the
//! button still works and the status line says why the dialog did not open.

/// Ask the user for a file, or a folder when `folder`; `Ok(None)` when they
/// cancelled. The path comes back with the home folder as `~`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn choose(folder: bool) -> Result<Option<String>, String> {
    use ashpd::desktop::ResponseError;
    use ashpd::desktop::file_chooser::SelectedFiles;

    let title = if folder {
        "Choose a folder"
    } else {
        "Choose a file"
    };
    let request = SelectedFiles::open_file()
        .title(title)
        .modal(true)
        .directory(folder)
        .send()
        .await
        .map_err(|e| explain(&e))?;
    match request.response() {
        Ok(files) => Ok(files
            .uris()
            .first()
            .and_then(|uri| file_path(uri.as_str()))
            .map(|path| crate::store::tilde(std::path::Path::new(&path)))),
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(None),
        Err(e) => Err(explain(&e)),
    }
}

/// The browser demo has no file chooser to ask.
#[cfg(target_arch = "wasm32")]
pub async fn choose(_folder: bool) -> Result<Option<String>, String> {
    Err("The file chooser opens in the desktop app. In this demo, type the path.".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn explain(error: &ashpd::Error) -> String {
    format!(
        "The file chooser did not open ({error}). It needs xdg-desktop-portal with a file chooser, such as the gtk portal. You can type the path instead."
    )
}

/// `file:///home/me/My%20Pictures` → `/home/me/My Pictures`.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn file_path(uri: &str) -> Option<String> {
    let encoded = uri.strip_prefix("file://")?;
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(byte) = encoded
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Whether the desktop asks apps for more contrast (the portal's
/// `org.freedesktop.appearance` `contrast`; GNOME's high-contrast switch
/// sets it). No answer counts as no.
#[cfg(not(target_arch = "wasm32"))]
pub async fn high_contrast() -> bool {
    use ashpd::desktop::settings::{Contrast, Settings};
    match Settings::new().await {
        Ok(settings) => matches!(settings.contrast().await, Ok(Contrast::High)),
        Err(_) => false,
    }
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

/// Hand every later change of the desktop's contrast preference to `each`
/// (whether it asks for more), until `each` says stop.
#[cfg(not(target_arch = "wasm32"))]
pub async fn watch_contrast(mut each: impl FnMut(bool) -> bool) {
    use ashpd::desktop::settings::{Contrast, Settings};
    use futures_util::StreamExt;

    let Ok(settings) = Settings::new().await else {
        return;
    };
    let Ok(mut changes) = settings.receive_contrast_changed().await else {
        return;
    };
    while let Some(contrast) = changes.next().await {
        if !each(matches!(contrast, Contrast::High)) {
            break;
        }
    }
}

/// The browser's preference is read once, at start.
#[cfg(target_arch = "wasm32")]
pub async fn watch_contrast(_each: impl FnMut(bool) -> bool) {}

#[cfg(test)]
mod tests {
    #[test]
    fn portal_uris_become_paths() {
        assert_eq!(
            super::file_path("file:///home/me/My%20Pictures").as_deref(),
            Some("/home/me/My Pictures")
        );
        assert_eq!(super::file_path("https://example.org"), None);
    }
}
