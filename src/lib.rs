//! WordBench: a terminal word processor in the spirit of the classic DOS word processors,
//! for Linux, macOS and Windows.

// Layout and file-format code passes a lot of state around; these lints
// would only push it into one-off structs and aliases.
#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::explicit_counter_loop
)]

mod app;
mod clip;
mod commands;
mod editor;
mod graphics;
mod io;
mod layout;
mod model;
mod pictures;
mod platform;
mod print;
mod props;
mod reveal;
mod review;
mod styles;
mod tools;
mod ui;

#[cfg(test)]
mod tests;

use app::{App, Config, Theme};
use commands::Keymap;
use editor::Editor;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::supports_keyboard_enhancement;
use std::path::PathBuf;

/// Where people can support WordBench.
pub const SUPPORT_URL: &str = "https://buymeacoffee.com/williamlampley";

const USAGE: &str = "\
WordBench - a terminal word processor

Usage:
  wordbench [options] [FILE]           (`wb` is a short name for the same program)
  wordbench --convert INPUT OUTPUT     convert between .wbd, .docx, .txt and .pdf

Options:
  --classic   Classic function-key keyboard (F10 save, F7 exit, F3 help)
  --modern    Modern keyboard (Ctrl+S save, Ctrl+Q quit, F1 help)
  --plain     Use the terminal's own colors instead of the classic blue screen
  --theme T   Color theme: blue, dark, light, solarized, amber, green,
              contrast or plain (also in Tools > Settings, View menu)
  --keys      Print every command and its keys (Markdown)
  -h, --help  Show this help
  -V, --version

Files: .wbd (native), .docx (Word), .txt/.md (plain text); export to .pdf.
Pictures are drawn with kitty or sixel graphics when the terminal supports
them; WORDBENCH_GRAPHICS=kitty|sixel|none overrides the guess.";

/// Run WordBench with the process's command-line arguments.
pub fn cli_main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::load();
    layout::set_full_size_pictures(cfg.pictures);
    let mut file: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                println!("\nSettings folder: {}", platform::config_dir().display());
                println!("Support WordBench: {SUPPORT_URL}");
                return;
            }
            "-V" | "--version" => {
                println!("wordbench {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--keys" => {
                print!("{}", commands::key_reference());
                return;
            }
            "--classic" => cfg.keymap = Keymap::Classic,
            "--modern" => cfg.keymap = Keymap::Modern,
            "--plain" => cfg.theme = Theme::Plain,
            "--theme" if i + 1 < args.len() => {
                i += 1;
                cfg.theme = Theme::from_key(&args[i].to_lowercase());
            }
            "--convert" => {
                if args.len() < i + 3 {
                    eprintln!("--convert needs INPUT and OUTPUT");
                    std::process::exit(2);
                }
                let (inp, out) = (
                    PathBuf::from(args.remove(i + 1)),
                    PathBuf::from(args.remove(i + 1)),
                );
                match io::load(&inp).and_then(|d| io::save(&d, &out)) {
                    Ok(_) => {
                        println!("{} -> {}", inp.display(), out.display());
                        return;
                    }
                    Err(e) => {
                        eprintln!("wordbench: {e}");
                        std::process::exit(1);
                    }
                }
            }
            a if a.starts_with('-') && a.len() > 1 => {
                eprintln!("wordbench: unknown option {a}\n\n{USAGE}");
                std::process::exit(2);
            }
            a => file = Some(PathBuf::from(a)),
        }
        i += 1;
    }

    let mut app = App::new(Editor::new(), cfg);
    app.osc52 = true;
    app.sysclip = Some(clip::SysClip::new());
    app.gfx = graphics::Graphics::new(graphics::detect());
    app.gfx.query_cell_size();
    match &file {
        Some(p) => app.open_path(p),
        None => {
            app.msg = Some(match app.cfg.keymap {
                Keymap::Classic => "WordBench  —  F3 help · Alt+= or F9 menus · F10 save · F7 exit · Alt+K modern keys".into(),
                Keymap::Modern => "WordBench  —  F1 help · F10 menus · Ctrl+S save · Ctrl+Q quit · Alt+K classic keys".into(),
            });
        }
    }

    if let Err(e) = run(&mut app) {
        eprintln!("wordbench: {e}");
        std::process::exit(1);
    }
}

fn run(app: &mut App) -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let mut out = std::io::stdout();
    execute!(out, EnableMouseCapture, EnableBracketedPaste)?;
    // Lets terminals that support it report Ctrl+Enter, Ctrl+I vs Tab, etc.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    let result = (|| -> std::io::Result<()> {
        while !app.quit {
            terminal.draw(|f| app.draw(f))?;
            if app.gfx.needs_clear() {
                terminal.clear()?;
                terminal.draw(|f| app.draw(f))?;
            }
            app.gfx.flush(&mut out)?;
            let ev = event::read()?;
            if matches!(ev, event::Event::Resize(..)) {
                app.gfx.query_cell_size();
            }
            app.handle(ev);
            // Drain queued events (fast typing, pastes) before redrawing.
            while !app.quit && event::poll(std::time::Duration::ZERO)? {
                app.handle(event::read()?);
            }
        }
        Ok(())
    })();
    let _ = app.gfx.clear_all(&mut out);
    if enhanced {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(out, DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    result
}
