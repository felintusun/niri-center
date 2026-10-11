//! The displays niri runs, as the Displays page shows and arranges them:
//! their modes in words, the arrangement with staged changes applied, and
//! where a dragged display lands.

use crate::settings::{Coordinate, DraftKey, Drafts};
use crate::store::{Block, Store};
use crate::words::short_number;

/// One monitor's logical position and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorInfo {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One mode a display supports, as niri reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode {
    /// As the config writes it: `2560x1440@165.001`.
    pub value: String,
    /// As people read it: `2560 × 1440, 165 Hz`.
    pub label: String,
    /// As the mode picker lists it: the label, and whether niri picks it
    /// when the config names none. Worked out once, as niri reports it.
    pub choice: String,
    pub width: u32,
    pub height: u32,
}

/// One connected output as the Displays page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputInfo {
    pub name: String,
    /// Make and model, e.g. `Dell Inc. U2720Q`.
    pub description: String,
    /// Every mode niri reports for the display, best first.
    pub modes: Vec<Mode>,
    /// The mode niri picks when the config names none.
    pub preferred: Option<usize>,
    /// The mode it runs now.
    pub current: Option<usize>,
    pub vrr_supported: bool,
    pub vrr_enabled: bool,
    /// Logical geometry as niri runs it; `None` while the output is off.
    pub rect: Option<MonitorInfo>,
    /// The scale niri runs it at.
    pub scale: f64,
}

/// `59.951` → `59.95 Hz`; whole rates lose their decimals.
fn hertz(millihertz: u32) -> String {
    format!("{} Hz", short_number(f64::from(millihertz) / 1000.0))
}

impl OutputInfo {
    /// The display's name for people: make and model, else its connector.
    pub fn title(&self) -> &str {
        if self.description.is_empty() {
            &self.name
        } else {
            &self.description
        }
    }
}

/// A display as the Windows page lists it: its mode, scale and place.
pub fn summary(output: &OutputInfo) -> String {
    let mode = output
        .current
        .and_then(|index| output.modes.get(index))
        .map_or("off", |mode| mode.label.as_str());
    let placement = output.rect.map_or_else(String::new, |rect| {
        format!(
            ", scale {}, at {}, {}",
            short_number(output.scale),
            rect.x,
            rect.y
        )
    });
    let vrr = if output.vrr_enabled { ", VRR on" } else { "" };
    format!("{}: {mode}{placement}{vrr}", output.name)
}

/// One mode as niri reports it: pixels, refresh in millihertz, preferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawMode {
    pub width: u32,
    pub height: u32,
    pub millihertz: u32,
    pub preferred: bool,
}

/// The modes niri reports, labelled; two modes that would read the same
/// keep their exact rate apart.
pub fn modes(modes: &[RawMode]) -> Vec<Mode> {
    let mut out: Vec<Mode> = Vec::new();
    for mode in modes {
        let exact = f64::from(mode.millihertz) / 1000.0;
        let value = format!("{}x{}@{exact:.3}", mode.width, mode.height);
        let mut label = format!(
            "{} × {}, {}",
            mode.width,
            mode.height,
            hertz(mode.millihertz)
        );
        if out.iter().any(|m| m.label == label) {
            label = format!("{} × {}, {exact:.3} Hz", mode.width, mode.height);
        }
        let choice = if mode.preferred {
            format!("{label}, preferred")
        } else {
            label.clone()
        };
        out.push(Mode {
            value,
            label,
            choice,
            width: mode.width,
            height: mode.height,
        });
    }
    out
}

/// The rectangles the arrangement canvas draws, as niri runs them.
pub fn rects(outputs: &[OutputInfo]) -> Vec<(String, MonitorInfo)> {
    outputs
        .iter()
        .filter_map(|output| output.rect.map(|rect| (output.name.clone(), rect)))
        .collect()
}

/// The rectangles with the staged config applied: a new mode, scale,
/// rotation or position changes the logical size the way niri computes it,
/// and a display turned off drops out.
pub fn staged(outputs: &[OutputInfo], store: &Store) -> Vec<(String, MonitorInfo)> {
    let mut out = Vec::new();
    for output in outputs {
        let chain = [Block::with_arg("output", &output.name)];
        if store.is_set(&chain, "off") {
            continue;
        }
        let Some(running) = output.rect else {
            continue;
        };
        let current_px = output
            .current
            .and_then(|index| output.modes.get(index))
            .map(|mode| (f64::from(mode.width), f64::from(mode.height)));
        let staged_px = store
            .setting(&chain, "mode")
            .and_then(|mode| parse_mode(&mode))
            .or_else(|| {
                output
                    .preferred
                    .and_then(|index| output.modes.get(index))
                    .map(|mode| (f64::from(mode.width), f64::from(mode.height)))
            })
            .or(current_px);
        let scale = store
            .setting(&chain, "scale")
            .and_then(|scale| scale.parse::<f64>().ok())
            .filter(|scale| *scale > 0.0)
            .unwrap_or(output.scale);
        let rotated = store
            .setting(&chain, "transform")
            .is_some_and(|t| t.ends_with("90") || t.ends_with("270"));
        let (width, height) = staged_px.map_or((running.width, running.height), |(w, h)| {
            let (w, h) = if rotated { (h, w) } else { (w, h) };
            ((w / scale).round(), (h / scale).round())
        });
        let position = store.setting_node(&chain, "position").map(|node| {
            let prop = |name: &str| {
                node.props
                    .iter()
                    .find(|prop| prop.name == name)
                    .and_then(|prop| prop.value.kind.as_str().parse::<f64>().ok())
            };
            (prop("x"), prop("y"))
        });
        out.push((
            output.name.clone(),
            MonitorInfo {
                x: position.and_then(|p| p.0).unwrap_or(running.x),
                y: position.and_then(|p| p.1).unwrap_or(running.y),
                width,
                height,
            },
        ));
    }
    out
}

/// `2560x1440@165.001` (or without the rate) as pixels.
pub fn parse_mode(mode: &str) -> Option<(f64, f64)> {
    let size = mode.split('@').next()?;
    let (width, height) = size.split_once('x')?;
    Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
}

/// Scales people pick, as niri writes them.
pub const SCALES: [&str; 8] = ["1", "1.25", "1.5", "1.75", "2", "2.5", "3", "4"];

/// The scale in the list that `written` is: `1.0` in the config is the
/// list's `1`, picked by value, not text.
pub fn listed_scale(written: &str) -> Option<&'static str> {
    let value = written.parse::<f64>().ok()?;
    SCALES
        .into_iter()
        .find(|scale| scale.parse::<f64>().is_ok_and(|s| (s - value).abs() < 1e-9))
}

/// A display's position typed into its `x` or `y` field:
/// kept as typed, and written once it is a whole number of pixels, with the
/// other coordinate where the display is now.
pub fn type_position(
    store: &mut Store,
    drafts: &mut Drafts,
    outputs: &[OutputInfo],
    name: &str,
    coordinate: Coordinate,
    typed: String,
) -> Result<(), String> {
    let key = DraftKey::Position(name.to_string(), coordinate);
    let current = staged(outputs, store);
    let Some((_, rect)) = current.iter().find(|(n, _)| n == name) else {
        drafts.keep(&key, typed, None);
        return Ok(());
    };
    let Ok(value) = typed.trim().parse::<i64>() else {
        drafts.keep(
            &key,
            typed,
            Some("A position is a whole number of pixels.".into()),
        );
        return Ok(());
    };
    drafts.keep(&key, typed, None);
    let (x, y) = match coordinate {
        Coordinate::X => (value, rect.y as i64),
        Coordinate::Y => (rect.x as i64, value),
    };
    store
        .set_output_position(name, x, y)
        .map_err(|e| e.to_string())
}

/// Pairs that overlap, and displays no other display touches (the pointer
/// cannot reach them), for the hint above the canvas.
pub fn problems(rects: &[(String, MonitorInfo)]) -> (Vec<(String, String)>, Vec<String>) {
    let overlaps = |a: &MonitorInfo, b: &MonitorInfo| {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    };
    let touches = |a: &MonitorInfo, b: &MonitorInfo| {
        let close = |p: f64, q: f64| (p - q).abs() <= 1.0;
        let x_shared = a.x < b.x + b.width && b.x < a.x + a.width;
        let y_shared = a.y < b.y + b.height && b.y < a.y + a.height;
        (x_shared && (close(a.y + a.height, b.y) || close(b.y + b.height, a.y)))
            || (y_shared && (close(a.x + a.width, b.x) || close(b.x + b.width, a.x)))
    };
    let mut overlapping = Vec::new();
    let mut alone = Vec::new();
    for (i, (name, a)) in rects.iter().enumerate() {
        for (other, b) in &rects[i + 1..] {
            if overlaps(a, b) {
                overlapping.push((name.clone(), other.clone()));
            }
        }
        if rects.len() > 1
            && !rects
                .iter()
                .any(|(other, b)| other != name && (touches(a, b) || overlaps(a, b)))
        {
            alone.push(name.clone());
        }
    }
    (overlapping, alone)
}

/// Where a display dropped at `x`, `y` goes: against the nearest edge of
/// another display, top or left edges lined up when they are close, so a
/// drag ends edge to edge instead of a few pixels apart or overlapping.
pub fn snap(name: &str, x: f64, y: f64, rects: &[(String, MonitorInfo)]) -> (i64, i64) {
    let Some((_, moved)) = rects.iter().find(|(n, _)| n == name) else {
        return (x.round() as i64, y.round() as i64);
    };
    let (w, h) = (moved.width, moved.height);
    // lined up when within this much
    let near = 64.0;
    let align = |value: f64, edges: [f64; 2]| {
        edges
            .into_iter()
            .find(|edge| (value - edge).abs() <= near)
            .unwrap_or(value)
    };
    let mut best: Option<(f64, f64, f64)> = None;
    for (other_name, o) in rects {
        if other_name == name {
            continue;
        }
        let beside_y =
            align(y, [o.y, o.y + o.height - h]).clamp(o.y - h + 1.0, o.y + o.height - 1.0);
        let beside_x = align(x, [o.x, o.x + o.width - w]).clamp(o.x - w + 1.0, o.x + o.width - 1.0);
        for (cx, cy) in [
            (o.x - w, beside_y),
            (o.x + o.width, beside_y),
            (beside_x, o.y - h),
            (beside_x, o.y + o.height),
        ] {
            let distance = (cx - x).hypot(cy - y);
            if best.is_none_or(|(_, _, d)| distance < d) {
                best = Some((cx, cy, distance));
            }
        }
    }
    let (cx, cy) = best.map_or((x, y), |(cx, cy, _)| (cx, cy));
    (cx.round() as i64, cy.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> MonitorInfo {
        MonitorInfo {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn overlapping_and_unreachable_displays_are_found() {
        let side_by_side = [
            ("eDP-1".to_string(), rect(0.0, 0.0, 1920.0, 1200.0)),
            ("DP-1".to_string(), rect(1920.0, 0.0, 2560.0, 1440.0)),
        ];
        assert_eq!(problems(&side_by_side), (vec![], vec![]));
        let overlapping = [
            ("eDP-1".to_string(), rect(0.0, 0.0, 1920.0, 1200.0)),
            ("DP-1".to_string(), rect(1800.0, 0.0, 2560.0, 1440.0)),
        ];
        assert_eq!(problems(&overlapping).0.len(), 1);
        let apart = [
            ("eDP-1".to_string(), rect(0.0, 0.0, 1920.0, 1200.0)),
            ("DP-1".to_string(), rect(2000.0, 0.0, 2560.0, 1440.0)),
        ];
        assert_eq!(problems(&apart).1, ["eDP-1", "DP-1"]);
    }

    #[test]
    fn mode_strings_read_as_pixels() {
        assert_eq!(parse_mode("2560x1440@165.001"), Some((2560.0, 1440.0)));
        assert_eq!(parse_mode("1920x1080"), Some((1920.0, 1080.0)));
        assert_eq!(hertz(60_001), "60 Hz");
        assert_eq!(hertz(59_951), "59.95 Hz");
    }
}

#[cfg(test)]
mod snap_tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> MonitorInfo {
        MonitorInfo {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_drop_ends_edge_to_edge_even_left_of_zero() {
        let rects = vec![
            ("eDP-1".to_string(), rect(0.0, 0.0, 1707.0, 1067.0)),
            ("DP-1".to_string(), rect(1707.0, 0.0, 2560.0, 1440.0)),
        ];
        // dropped roughly left of the laptop: snaps to its left edge, tops lined up
        assert_eq!(snap("DP-1", -2500.0, 30.0, &rects), (-2560, 0));
        // dropped a little right of where it was: back against the laptop
        assert_eq!(snap("DP-1", 1750.0, -20.0, &rects), (1707, 0));
    }
}
