//! The `niri-center` command: its arguments, then the window
//! (`niri_center_ui::run`).

use niri_center::page::Page;

const USAGE: &str = "usage: niri-center [--pane <name>]

  --pane <name>  open one page alone, for example from a niri binding:
                 spawn \"niri-center\" \"--pane\" \"displays\"
  -h, --help     show this help";

/// `None` = the full app; `Some(page)` = `--pane <slug>`. `Err` carries the
/// text to print and the exit code (0 for `--help`).
fn parse_args() -> Result<Option<Page>, (String, i32)> {
    let mut args = std::env::args().skip(1);
    let Some(flag) = args.next() else {
        return Ok(None);
    };
    let panes = Page::ALL.map(Page::slug).join(", ");
    match flag.as_str() {
        "-h" | "--help" => Err((format!("{USAGE}\n\npages: {panes}"), 0)),
        "--pane" => {
            let slug = args.next().unwrap_or_default();
            Page::from_slug(&slug)
                .map(Some)
                .ok_or_else(|| (format!("unknown page `{slug}`; one of: {panes}"), 2))
        }
        other => Err((format!("unknown argument `{other}`\n\n{USAGE}"), 2)),
    }
}

fn main() -> iced::Result {
    let pane = match parse_args() {
        Ok(pane) => pane,
        Err((message, 0)) => {
            println!("{message}");
            std::process::exit(0);
        }
        Err((message, code)) => {
            eprintln!("{message}");
            std::process::exit(code);
        }
    };
    niri_center_ui::run(pane)
}
