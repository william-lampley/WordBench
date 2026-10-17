//! End-to-end tests: drive the App with key events and render it to an
//! in-memory terminal. Set WORDBENCH_SHOW=1 to print the rendered screens.

use crate::app::{App, Config, Mode, Theme};
use crate::commands::Keymap;
use crate::editor::Editor;
use crate::model::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const ALT: KeyModifiers = KeyModifiers::ALT;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;

fn app(km: Keymap) -> App {
    App::new(
        Editor::new(),
        Config {
            keymap: km,
            theme: Theme::Plain,
            lang: "en_US".into(),
            author: "Tester".into(),
            pictures: true,
            outline: true,
            side_by_side: true,
        },
    )
}

fn key(a: &mut App, code: KeyCode, mods: KeyModifiers) {
    a.handle(Event::Key(KeyEvent::new(code, mods)));
}

fn typ(a: &mut App, s: &str) {
    for c in s.chars() {
        match c {
            '\n' => key(a, KeyCode::Enter, NONE),
            c => key(a, KeyCode::Char(c), NONE),
        }
    }
}

fn screen(a: &mut App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| a.draw(f)).unwrap();
    let buf = t.backend().buffer().clone();
    let mut s = String::new();
    for y in 0..h {
        for x in 0..w {
            s.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
        }
        s.push('\n');
    }
    if std::env::var_os("WORDBENCH_SHOW").is_some() {
        println!("{s}");
    }
    s
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wordbench-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn wp_bold_reveal_codes_and_exit() {
    let mut a = app(Keymap::Classic);
    typ(&mut a, "Hello ");
    key(&mut a, KeyCode::F(6), NONE);
    typ(&mut a, "bold");
    key(&mut a, KeyCode::F(6), NONE);
    typ(&mut a, " world");
    key(&mut a, KeyCode::F(3), ALT); // Reveal Codes
    let s = screen(&mut a, 90, 24);
    assert!(s.contains("Hello bold world"), "{s}");
    assert!(s.contains("[BOLD]bold[bold]"), "{s}");
    assert!(s.contains("[HRt]"), "{s}");
    assert!(s.contains("Doc 1 Pg 1 Ln 1\" Pos"), "{s}");

    key(&mut a, KeyCode::F(7), NONE);
    assert!(screen(&mut a, 90, 24).contains("Save document? Yes (No)"));
    typ(&mut a, "n");
    assert!(screen(&mut a, 90, 24).contains("Exit WordBench?"));
    typ(&mut a, "y");
    assert!(a.quit);
}

#[test]
fn modern_save_and_reload_docx() {
    let path = tmp("modern.docx");
    let mut a = app(Keymap::Modern);
    typ(&mut a, "Title line\nSome ");
    key(&mut a, KeyCode::Char('b'), CTRL);
    typ(&mut a, "strong");
    key(&mut a, KeyCode::Char('b'), CTRL);
    typ(&mut a, " text.");
    key(&mut a, KeyCode::Char('s'), CTRL);
    assert!(matches!(a.mode, Mode::Prompt(_)));
    typ(&mut a, path.to_str().unwrap());
    key(&mut a, KeyCode::Enter, NONE);
    assert!(!a.ed.modified, "{:?}", a.msg);
    let d = crate::io::load(&path).unwrap();
    assert_eq!(d.to_plain_text(), "Title line\nSome strong text.\n");
    assert!(d.para(ParaRef::top(1)).cells[5].fmt.has(Fmt::BOLD));
    // Quit without a prompt now that it's saved.
    key(&mut a, KeyCode::Char('q'), CTRL);
    assert!(a.quit);
}

#[test]
fn modern_menu_and_format_form() {
    let mut a = app(Keymap::Modern);
    typ(&mut a, "centered");
    key(&mut a, KeyCode::F(10), NONE);
    let s = screen(&mut a, 100, 30);
    assert!(s.contains("New document"), "{s}");
    assert!(s.contains("Ctrl+N"), "{s}");
    key(&mut a, KeyCode::Esc, NONE);
    key(&mut a, KeyCode::Char('d'), CTRL); // Format dialog
    let s = screen(&mut a, 100, 30);
    assert!(s.contains("Justification"), "{s}");
    key(&mut a, KeyCode::Enter, NONE); // Left -> Center
    key(&mut a, KeyCode::F(10), NONE); // save
    assert_eq!(a.ed.para().props.align, Align::Center);
}

#[test]
fn tables_via_prompts() {
    let mut a = app(Keymap::Classic);
    typ(&mut a, "Before table\n");
    key(&mut a, KeyCode::F(7), ALT); // Tables menu
    typ(&mut a, "1"); // Create
    key(&mut a, KeyCode::Enter, NONE); // 3 columns (default)
    key(&mut a, KeyCode::Enter, NONE); // 2 rows (default)
    typ(&mut a, "Name");
    key(&mut a, KeyCode::Tab, NONE);
    typ(&mut a, "Qty");
    key(&mut a, KeyCode::Tab, NONE);
    typ(&mut a, "Price");
    let s = screen(&mut a, 90, 24);
    assert!(s.contains("┌"), "{s}");
    assert!(s.contains("│ Name"), "{s}");
    assert!(s.contains("│ Price"), "{s}");
    assert_eq!(a.ed.doc.table(1).unwrap().rows.len(), 2);
}

#[test]
fn lists_footnotes_and_preview() {
    let mut a = app(Keymap::Modern);
    typ(&mut a, "First");
    key(&mut a, KeyCode::Char('l'), CTRL | SHIFT);
    typ(&mut a, "\nSecond");
    key(&mut a, KeyCode::Tab, SHIFT); // promote: stays level 0
    typ(&mut a, "\n\nA claim");
    key(&mut a, KeyCode::Char('f'), CTRL | ALT);
    typ(&mut a, "Source, 1999.\n");
    let s = screen(&mut a, 90, 24);
    assert!(s.contains("•  First"), "{s}");
    assert!(s.contains("•  Second"), "{s}");
    assert!(s.contains("A claim1"), "{s}");
    assert!(s.contains("1. Source, 1999."), "{s}");
    key(&mut a, KeyCode::Char('p'), CTRL);
    typ(&mut a, "3"); // View document
    let s = screen(&mut a, 100, 70);
    assert!(s.contains("Print preview: page 1 of 1"), "{s}");
    assert!(s.contains("1. Source, 1999."), "{s}");
}

#[test]
fn find_and_replace_all() {
    let mut a = app(Keymap::Classic);
    typ(&mut a, "one fish two fish red fish");
    key(&mut a, KeyCode::Home, CTRL);
    key(&mut a, KeyCode::F(2), ALT);
    typ(&mut a, "fish\n");
    typ(&mut a, "cat\n");
    assert!(matches!(a.mode, Mode::Replace(_)));
    typ(&mut a, "a");
    assert_eq!(a.ed.doc.to_plain_text(), "one cat two cat red cat\n");
    assert_eq!(a.msg.as_deref(), Some("3 replacement(s) made"));
}

#[test]
fn mouse_click_moves_cursor() {
    let mut a = app(Keymap::Classic);
    typ(&mut a, "abcdef\nghijkl");
    screen(&mut a, 90, 24);
    let x = a.geo.text_x + 3;
    a.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: a.geo.doc_y,
        modifiers: NONE,
    }));
    assert_eq!(a.ed.cur, Pos::new(ParaRef::top(0), 3));
}

#[test]
fn spell_check_with_aspell() {
    if std::process::Command::new("aspell")
        .arg("-v")
        .output()
        .is_err()
        && std::process::Command::new("hunspell")
            .arg("-v")
            .output()
            .is_err()
    {
        eprintln!("skipping: no aspell/hunspell");
        return;
    }
    let mut a = app(Keymap::Classic);
    typ(&mut a, "This is a tset of speling.");
    key(&mut a, KeyCode::F(2), CTRL);
    let Mode::Spell(st) = &a.mode else {
        panic!("expected spell mode, msg={:?}", a.msg)
    };
    assert_eq!(st.word, "tset");
    assert!(st.sugg.iter().any(|s| s == "test"), "{:?}", st.sugg);
    let i = st.sugg.iter().position(|s| s == "test").unwrap();
    let s = screen(&mut a, 90, 24);
    assert!(s.contains("Not Found: \"tset\""), "{s}");
    typ(&mut a, &((b'a' + i as u8) as char).to_string());
    let Mode::Spell(st) = &a.mode else {
        panic!("expected second word")
    };
    assert_eq!(st.word, "speling");
    typ(&mut a, "1"); // skip once
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.ed.doc.to_plain_text(), "This is a test of speling.\n");
}

#[test]
fn undo_redo_and_keymap_switch() {
    let mut a = app(Keymap::Classic);
    typ(&mut a, "abc");
    key(&mut a, KeyCode::Char('z'), CTRL);
    assert_eq!(a.ed.doc.to_plain_text(), "\n");
    key(&mut a, KeyCode::Char('y'), CTRL);
    assert_eq!(a.ed.doc.to_plain_text(), "abc\n");
    key(&mut a, KeyCode::Char('k'), ALT);
    assert_eq!(a.cfg.keymap, Keymap::Modern);
    let s = screen(&mut a, 100, 10);
    assert!(s.contains("File") && s.contains("F10 Menu"), "{s}");
}

#[test]
fn wp_block_move_retrieve() {
    let mut a = app(Keymap::Classic);
    typ(&mut a, "move me|stay");
    key(&mut a, KeyCode::Home, CTRL);
    key(&mut a, KeyCode::F(4), ALT); // Block on
    for _ in 0..8 {
        key(&mut a, KeyCode::Right, NONE);
    }
    key(&mut a, KeyCode::F(4), CTRL); // Move menu
    typ(&mut a, "1"); // Move (cut)
    assert!(a.pending_retrieve);
    key(&mut a, KeyCode::End, NONE);
    key(&mut a, KeyCode::Enter, NONE); // retrieve
    assert_eq!(a.ed.doc.to_plain_text(), "staymove me|\n");
}

#[test]
fn font_dialog_sets_font_size_and_color() {
    let path = tmp("font.docx");
    let mut a = app(Keymap::Classic);
    key(&mut a, KeyCode::F(8), CTRL); // Font dialog
    let s = screen(&mut a, 100, 30);
    assert!(s.contains("Font") && s.contains("Highlight"), "{s}");
    // 1 = Font: opens the font list; a name that isn't installed is used as
    // typed. 2 = size; 3 = colour (cycle to Dark red).
    typ(&mut a, "1");
    typ(&mut a, "Qwxz Custom\n");
    typ(&mut a, "2");
    for _ in 0..10 {
        key(&mut a, KeyCode::Backspace, NONE);
    }
    typ(&mut a, "14\n");
    typ(&mut a, "3"); // Automatic -> Black
    typ(&mut a, "3"); // -> Dark red
    key(&mut a, KeyCode::F(10), NONE);
    typ(&mut a, "Styled");
    let s = screen(&mut a, 100, 30);
    assert!(s.contains("Qwxz Custom 14pt"), "{s}");
    a.ed.path = Some(path.clone());
    crate::io::save(&a.ed.doc, &path).unwrap();
    let d = crate::io::load(&path).unwrap();
    let st = d.store.borrow();
    let c = d.para(ParaRef::top(0)).cells[0];
    let look = st.look(c.cp);
    assert_eq!(look.font.as_deref(), Some("Qwxz Custom"));
    assert_eq!(look.size, Some(28));
    assert_eq!(look.rgb(), Some(0xC00000));
}

#[test]
fn style_menu_lists_document_styles() {
    let mut a = app(Keymap::Classic);
    typ(&mut a, "A heading");
    key(&mut a, KeyCode::F(8), ALT); // Style menu
    let s = screen(&mut a, 100, 30);
    assert!(s.contains("Heading 1") && s.contains("Quote"), "{s}");
    // Pick Heading 2 by typing its initial letter twice then Enter.
    let Mode::Choice(c) = &a.mode else { panic!() };
    let idx = c
        .items
        .iter()
        .position(|(l, _)| l.contains("Heading 2"))
        .unwrap();
    for _ in 0..idx {
        key(&mut a, KeyCode::Down, NONE);
    }
    key(&mut a, KeyCode::Enter, NONE);
    assert_eq!(a.ed.para().props.style, "Heading2");
    // Enter after a heading starts a Normal paragraph.
    typ(&mut a, "\nbody");
    assert_eq!(a.ed.para().props.style, "");
}

#[test]
fn renders_libreoffice_document() {
    let doc = crate::io::docx::from_bytes(include_bytes!("../tests/fixtures/rich.docx")).unwrap();
    let mut a = App::new(
        Editor::with_doc(doc, None),
        Config {
            keymap: Keymap::Classic,
            theme: Theme::Plain,
            lang: "en_US".into(),
            author: "Tester".into(),
            pictures: true,
            outline: true,
            side_by_side: true,
        },
    );
    let s = screen(&mut a, 90, 40);
    assert!(s.contains("Introduction"), "{s}");
    assert!(s.contains("[Image"), "{s}");
    assert!(s.contains("Merged header"), "{s}");
    assert!(s.contains("•") || s.contains("◦"), "{s}");
}

#[test]
fn review_features_from_the_keyboard() {
    let mut a = app(Keymap::Modern);
    typ(&mut a, "Hello world");
    key(&mut a, KeyCode::Char('e'), CTRL | SHIFT); // track changes
    assert!(a.ed.track);
    assert_eq!(a.msg.as_deref(), Some("Tracking changes as Tester"));
    typ(&mut a, " again");
    key(&mut a, KeyCode::Backspace, NONE);
    assert_eq!(a.ed.change_count(), 1); // own insertions just shrink
    key(&mut a, KeyCode::Char('r'), ALT); // Review menu
    let s = screen(&mut a, 100, 30);
    assert!(
        s.contains("Accept all changes") && s.contains("New comment"),
        "{s}"
    );
    key(&mut a, KeyCode::Esc, NONE);
    key(&mut a, KeyCode::Char('e'), CTRL | SHIFT); // tracking off
    // Comment at the end.
    key(&mut a, KeyCode::Char('m'), CTRL | ALT);
    typ(&mut a, "Looks good\n");
    let s = screen(&mut a, 100, 30);
    assert!(s.contains("[Comment: Tester]"), "{s}");
    // Link on "Hello".
    key(&mut a, KeyCode::Home, CTRL);
    for _ in 0..5 {
        key(&mut a, KeyCode::Right, SHIFT);
    }
    key(&mut a, KeyCode::Char('k'), CTRL);
    typ(&mut a, "example.org\n");
    key(&mut a, KeyCode::Left, NONE);
    key(&mut a, KeyCode::Right, NONE);
    assert_eq!(
        a.ed.link_at_cursor().map(|l| l.1),
        Some("https://example.org".to_string())
    );
    // Accept everything: the insertion stays, as plain text.
    a.run(crate::commands::Cmd::AcceptAll);
    assert_eq!(a.ed.change_count(), 0);
    assert_eq!(a.ed.doc.to_plain_text(), "Hello world agai\n");
}

#[test]
fn pictures_on_screen() {
    use crate::graphics::{Graphics, Proto};
    let doc = crate::io::docx::from_bytes(include_bytes!("../tests/fixtures/rich.docx")).unwrap();
    let mut a = App::new(
        Editor::with_doc(doc, None),
        Config {
            keymap: Keymap::Classic,
            theme: Theme::Plain,
            lang: "en_US".into(),
            author: "Tester".into(),
            pictures: true,
            outline: true,
            side_by_side: true,
        },
    );
    // Without terminal graphics: the placeholder, with the line reserved
    // above it shaded.
    let s = screen(&mut a, 90, 40);
    let rows: Vec<&str> = s.lines().collect();
    let at = rows
        .iter()
        .position(|r| r.contains("picture [Image:…"))
        .expect(&s);
    assert!(rows[at - 1].contains("░░░░░░░░"), "{s}");
    // With kitty graphics: the box is blank and the picture is placed on it.
    a.gfx = Graphics::new(Some(Proto::Kitty));
    let s = screen(&mut a, 90, 40);
    assert!(!s.contains("[Image") && !s.contains('░'), "{s}");
    let p = a.gfx.pending.clone();
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].cols, p[0].rows, p[0].y as usize), (8, 2, at - 1));
    let mut out = Vec::new();
    a.gfx.flush(&mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(
        out.contains("\x1b_Ga=t,f=32,o=z,s=40,v=20,i=1,") && out.contains("_Ga=p,i=1,c=8,r=2,"),
        "{out:?}"
    );
    // Dialogs hide pictures; turning pictures off goes back to placeholders.
    a.mode = Mode::Help { scroll: 0 };
    screen(&mut a, 90, 40);
    assert!(a.gfx.pending.is_empty());
    a.mode = Mode::Normal;
    a.layout = crate::layout::Layout::build_with(
        &a.ed.doc,
        crate::layout::LayoutOpts {
            full_size_pictures: false,
        },
    );
    let s = screen(&mut a, 90, 40);
    assert!(s.contains("picture [Image: Chart picture] and"), "{s}");
    assert!(a.gfx.pending.is_empty());
}

#[test]
fn every_command_is_reachable_from_the_keyboard() {
    use crate::commands::{ALL, MENUS, item_keys};
    // Every menu item is a known command, and every item has its own letter.
    for (m, (name, items)) in MENUS.iter().enumerate() {
        let keys = item_keys(m);
        for (it, k) in items.iter().zip(&keys) {
            if let Some(cmd) = it {
                assert!(ALL.contains(cmd), "{cmd:?}");
                assert!(k.is_some(), "{name} > {} has no access letter", cmd.label());
            }
        }
        let letters: Vec<char> = keys.iter().flatten().map(|(_, c)| *c).collect();
        let mut dedup = letters.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(
            letters.len(),
            dedup.len(),
            "{name}: duplicate access letters"
        );
    }
    // The command list offers every command.
    let a = app(Keymap::Classic);
    assert_eq!(a.palette_items("").len(), ALL.len() - 1);
}

#[test]
fn keyboard_only_menus_and_command_list() {
    let mut a = app(Keymap::Modern);
    typ(&mut a, "one\ntwo");
    // Alt+O opens Format (Alt+F is File); "b" picks Bold.
    key(&mut a, KeyCode::Char('o'), ALT);
    let Mode::Menu { m, .. } = a.mode else {
        panic!("no menu")
    };
    assert_eq!(crate::commands::MENUS[m].0, "Format");
    let s = screen(&mut a, 100, 12); // short screen: the menu scrolls
    assert!(s.contains("▼"), "{s}");
    key(&mut a, KeyCode::End, NONE);
    let s = screen(&mut a, 100, 12);
    assert!(s.contains("Format (line/page/document)"), "{s}");
    key(&mut a, KeyCode::Esc, NONE);
    // Alt+X, type to search, Enter runs it.
    key(&mut a, KeyCode::Char('x'), ALT);
    typ(&mut a, "track");
    let s = screen(&mut a, 100, 30);
    assert!(s.contains("Track changes on/off  (Review)"), "{s}");
    key(&mut a, KeyCode::Enter, NONE);
    assert!(a.ed.track);
    // Ctrl+Up/Down move by paragraph.
    key(&mut a, KeyCode::Up, CTRL);
    assert_eq!(a.ed.cur, Pos::new(ParaRef::top(1), 0));
    key(&mut a, KeyCode::Up, CTRL);
    assert_eq!(a.ed.cur, Pos::new(ParaRef::top(0), 0));
    key(&mut a, KeyCode::Down, CTRL | SHIFT);
    assert_eq!(
        a.ed.sel_range().map(|r| r.1),
        Some(Pos::new(ParaRef::top(1), 0))
    );
}

#[test]
fn font_stays_after_making_a_table() {
    let mut a = app(Keymap::Modern);
    let edit = crate::props::CharEdit {
        font: Some(Some("Georgia".into())),
        ..Default::default()
    };
    a.ed.apply_char_edit(&edit);
    typ(&mut a, "Before\n");
    a.ed.insert_table(2, 2);
    typ(&mut a, "A");
    key(&mut a, KeyCode::Tab, NONE);
    typ(&mut a, "B");
    key(&mut a, KeyCode::Down, CTRL); // leave the table
    key(&mut a, KeyCode::Down, CTRL);
    key(&mut a, KeyCode::Down, CTRL);
    typ(&mut a, "After");
    let st = a.ed.doc.store.borrow();
    for pr in a.ed.doc.refs() {
        for c in a.ed.doc.para(pr).cells.iter() {
            assert_eq!(
                st.look(c.cp).font.as_deref(),
                Some("Georgia"),
                "{:?} in {pr:?}",
                c.ch
            );
        }
    }
}

#[test]
fn caption_dialog_numbers_tables_and_figures() {
    let path = tmp("captions.docx");
    let mut a = app(Keymap::Modern);
    typ(&mut a, "Intro\n");
    a.ed.insert_table(2, 2);
    typ(&mut a, "x");
    // Insert > Caption from the command list; the dialog defaults to a
    // table caption above the table.
    a.run(crate::commands::Cmd::InsertCaption);
    let s = screen(&mut a, 100, 30);
    assert!(
        s.contains("Insert Caption") && s.contains("Preview:  Table 1"),
        "{s}"
    );
    typ(&mut a, "1"); // caption text
    typ(&mut a, "Quarterly sales\n");
    let s = screen(&mut a, 100, 30);
    assert!(s.contains("Preview:  Table 1: Quarterly sales"), "{s}");
    key(&mut a, KeyCode::F(10), NONE);
    assert_eq!(
        a.ed.doc.para(ParaRef::top(1)).plain_text(),
        "Table 1: Quarterly sales"
    );
    assert!(
        a.ed.doc
            .styles
            .get(&a.ed.doc.para(ParaRef::top(1)).props.style)
            .is_some_and(|s| s.name == "caption")
    );
    // Two figure captions, the second inserted above the first: numbers follow
    // document order.
    a.ed.move_to(a.ed.doc.end_pos(), false);
    typ(&mut a, "picture two");
    a.ed.insert_caption("Figure", "Second", false, "ARABIC", false);
    a.ed.move_to(Pos::new(ParaRef::top(0), 0), false);
    a.ed.insert_caption("Figure", "First", true, "ARABIC", false);
    let text = a.ed.doc.to_plain_text();
    assert!(text.starts_with("Figure 1: First\nIntro\n"), "{text}");
    assert!(text.contains("Figure 2: Second"), "{text}");
    crate::io::save(&a.ed.doc, &path).unwrap();
    let d = crate::io::load(&path).unwrap();
    assert!(d.to_plain_text().contains("Table 1: Quarterly sales"));
    let xml = {
        use std::io::Read;
        let mut z = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut s = String::new();
        z.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        s
    };
    assert!(
        xml.contains(r#"<w:instrText xml:space="preserve"> SEQ Table \* ARABIC </w:instrText>"#),
        "{xml}"
    );
    assert!(xml.contains(r#"<w:pStyle w:val="Caption"/>"#), "{xml}");
}

#[test]
fn font_picker_finds_misspelled_fonts() {
    let mut a = app(Keymap::Classic);
    key(&mut a, KeyCode::F(8), CTRL); // Font dialog
    key(&mut a, KeyCode::Enter, NONE); // on Font: opens the list
    let Mode::FontPick(fp) = &a.mode else {
        panic!("no font list")
    };
    let has_dejavu = fp.fonts.iter().any(|f| f == "DejaVu Sans");
    let s = screen(&mut a, 100, 30);
    assert!(
        s.contains("Choose a font") && s.contains("(Default font of the style)"),
        "{s}"
    );
    typ(&mut a, "dejavu sanz");
    key(&mut a, KeyCode::Enter, NONE);
    let Mode::Form(f) = &a.mode else {
        panic!("back to the dialog")
    };
    let chosen = f.fields.iter().find(|x| x.key == "font").unwrap().display();
    if has_dejavu {
        assert!(chosen.starts_with("DejaVu Sans"), "{chosen}");
    }
    key(&mut a, KeyCode::F(10), NONE);
    typ(&mut a, "x");
    let st = a.ed.doc.store.borrow();
    assert_eq!(
        st.look(a.ed.doc.para(ParaRef::top(0)).cells[0].cp)
            .font
            .as_deref(),
        Some(chosen.as_str())
    );
}

#[test]
fn saving_without_extension_asks_for_type() {
    let base = tmp("no_ext_doc");
    let mut a = app(Keymap::Modern);
    typ(&mut a, "hello");
    key(&mut a, KeyCode::Char('s'), CTRL);
    typ(&mut a, base.to_str().unwrap());
    key(&mut a, KeyCode::Enter, NONE);
    let s = screen(&mut a, 120, 30);
    assert!(
        s.contains("Save as type") && s.contains("Word document") && s.contains("no_ext_doc.docx"),
        "{s}"
    );
    typ(&mut a, "1");
    assert!(base.with_extension("docx").exists());
    assert_eq!(
        a.ed.path.as_deref(),
        Some(base.with_extension("docx").as_path())
    );
    assert!(!a.ed.modified);
}

#[test]
fn every_theme_draws_tables_and_captions() {
    for t in Theme::ALL {
        let mut a = App::new(
            Editor::new(),
            Config {
                keymap: Keymap::Classic,
                theme: t,
                lang: "en_US".into(),
                author: "Tester".into(),
                pictures: true,
                outline: true,
                side_by_side: true,
            },
        );
        typ(&mut a, "Text\n");
        a.ed.insert_table(2, 2);
        typ(&mut a, "cell");
        a.ed.insert_caption("Table", "Totals", true, "ARABIC", false);
        let s = screen(&mut a, 90, 20);
        assert!(
            s.contains("┌") && s.contains("Table 1: Totals"),
            "{t:?}\n{s}"
        );
    }
}

#[test]
fn paper_outline_and_theme_preview() {
    let mut a = app(Keymap::Classic);
    a.cfg.theme = Theme::Blue;
    typ(&mut a, "Text on the page");
    // Letter paper (85 columns) is outlined, with the text 10 columns in.
    let s = screen(&mut a, 100, 12);
    let row = s.lines().find(|l| l.contains("Text on the page")).unwrap();
    let cols: Vec<char> = row.chars().collect();
    let left = cols.iter().position(|&c| c == '│').expect("left edge");
    let text = cols.iter().position(|&c| c == 'T').unwrap();
    let right = cols.iter().rposition(|&c| c == '│').expect("right edge");
    assert_eq!(text - left, 11, "{row}");
    assert_eq!(right - left, 86, "{row}");
    // The sheet starts above the first line; its bottom edge is where the
    // page ends (54 lines down), not under the text.
    assert!(s.lines().next().unwrap().contains('╭'), "{s}");
    assert!(!s.contains('╰'), "{s}");
    let tall = screen(&mut a, 100, 64);
    let rows: Vec<&str> = tall.lines().collect();
    let bottom = rows
        .iter()
        .position(|l| l.contains('╰'))
        .expect("bottom edge");
    assert_eq!(bottom, 1 + 54, "{tall}");
    // The theme dialog previews as you move and reverts on Esc...
    a.run(crate::commands::Cmd::ToggleTheme);
    let s = screen(&mut a, 100, 20);
    assert!(
        s.contains("Color Theme") && s.contains("Amber screen"),
        "{s}"
    );
    key(&mut a, KeyCode::Down, NONE);
    assert_eq!(a.cfg.theme, Theme::Dark);
    key(&mut a, KeyCode::Char('o'), NONE);
    assert!(!a.cfg.outline);
    key(&mut a, KeyCode::Esc, NONE);
    assert_eq!(a.cfg.theme, Theme::Blue);
    assert!(a.cfg.outline);
    // ...and keeps the choice on Enter.
    a.run(crate::commands::Cmd::ToggleTheme);
    typ(&mut a, "5");
    assert_eq!(a.cfg.theme, Theme::Amber);
    key(&mut a, KeyCode::Enter, NONE);
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.cfg.theme, Theme::Amber);
    // Turning the outline off removes the edges.
    a.run(crate::commands::Cmd::PaperOutline);
    let s = screen(&mut a, 100, 12);
    assert!(!s.contains('│') && !s.contains('╰'), "{s}");
}

#[test]
#[ignore]
fn show_outline_page_break() {
    let mut a = app(Keymap::Classic);
    for i in 0..60 {
        typ(&mut a, &format!("Line {i}\n"));
    }
    a.ed.move_to(Pos::new(ParaRef::top(54), 0), false);
    screen(&mut a, 100, 14);
    a.run(crate::commands::Cmd::ToggleTheme);
    screen(&mut a, 100, 16);
}

#[test]
#[ignore]
fn show_short_document_sheet() {
    let mut a = app(Keymap::Classic);
    typ(&mut a, "Dear reader,\n\nThis is a short letter.");
    screen(&mut a, 100, 16);
}

#[test]
fn pages_side_by_side_when_there_is_room() {
    let mut a = app(Keymap::Classic);
    for i in 0..120 {
        typ(&mut a, &format!("Line {i}\n"));
    }
    // 100 columns: one sheet at a time.
    let s = screen(&mut a, 100, 20);
    assert!(
        !s.contains("Page 2 ") || s.lines().filter(|l| l.contains('╭')).count() <= 1,
        "{s}"
    );
    // 200 columns: two sheets side by side, labelled.
    a.ed.move_to(a.ed.doc_start(), false);
    let s = screen(&mut a, 200, 20);
    let top = s.lines().next().unwrap();
    assert_eq!(top.matches('╭').count(), 2, "{s}");
    assert!(top.contains("Page 1") && top.contains("Page 2"), "{s}");
    let row = s.lines().nth(1).unwrap();
    assert!(row.contains("Line 0") && row.contains("Line 54"), "{s}");
    // Clicking on the right-hand sheet puts the cursor on that page.
    let x = row
        .chars()
        .collect::<Vec<_>>()
        .iter()
        .collect::<String>()
        .find("Line 54")
        .unwrap();
    let col = row[..x].chars().count() as u16;
    a.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col + 2,
        row: 1,
        modifiers: NONE,
    }));
    assert_eq!(a.ed.cur, Pos::new(ParaRef::top(54), 2));
    // Moving down past the bottom of a row of pages scrolls to the next row.
    a.ed.move_to(Pos::new(ParaRef::top(115), 0), false);
    let s = screen(&mut a, 200, 20);
    assert!(s.contains("Line 115"), "{s}");
    // It can be turned off.
    a.run(crate::commands::Cmd::SideBySide);
    let s = screen(&mut a, 200, 20);
    assert!(s.lines().all(|l| l.matches('│').count() <= 2), "{s}");
}

#[test]
#[ignore]
fn show_side_by_side() {
    let mut a = app(Keymap::Classic);
    typ(&mut a, "Report\n\n");
    for i in 0..70 {
        typ(&mut a, &format!("Paragraph {i} of the report text.\n"));
    }
    a.ed.move_to(a.ed.doc_start(), false);
    screen(&mut a, 190, 12);
}

#[test]
fn multi_line_header_and_footer() {
    let path = tmp("headers.docx");
    let mut a = app(Keymap::Modern);
    typ(&mut a, "Body text");
    a.run(crate::commands::Cmd::EditHeader);
    typ(&mut a, "ACME Corporation");
    key(&mut a, KeyCode::Enter, NONE);
    typ(&mut a, "Quarterly report");
    key(&mut a, KeyCode::Tab, NONE); // centre this line
    key(&mut a, KeyCode::Enter, NONE);
    typ(&mut a, "Page ");
    key(&mut a, KeyCode::Char('p'), CTRL);
    typ(&mut a, " of ");
    key(&mut a, KeyCode::Char('n'), CTRL);
    key(&mut a, KeyCode::Tab, NONE); // new lines copy the line above (centre): now right
    let s = screen(&mut a, 100, 30);
    assert!(
        s.contains("[Ctr]") && s.contains("[Rgt]") && s.contains("Page 1 of 1"),
        "{s}"
    );
    key(&mut a, KeyCode::F(10), NONE);
    assert_eq!(
        a.ed.doc.header,
        "ACME Corporation\n{center}Quarterly report\n{right}Page {page} of {pages}"
    );
    // Three lines plus a blank one are taken from the page.
    assert_eq!(crate::layout::reserved_lines(&a.ed.doc), 4);
    let pages = crate::print::compose(&a.ed.doc, &a.layout);
    let row = |r: usize| pages[0].rows[r].iter().map(|g| g.ch).collect::<String>();
    assert_eq!(row(0), "ACME Corporation");
    assert!(row(1).contains("Quarterly report") && pages[0].rows[1][0].x > 20);
    assert_eq!(
        pages[0].rows[2].last().map(|g| g.x),
        Some(a.layout.width - 1)
    );
    // A two-line footer from the Format dialog.
    a.run(crate::commands::Cmd::FormatMenu);
    let Mode::Form(f) = &a.mode else { panic!() };
    let fi = f.fields.iter().position(|x| x.key == "footer").unwrap();
    for _ in 0..30 {
        if let Mode::Form(f) = &a.mode
            && f.sel == fi
        {
            break;
        }
        key(&mut a, KeyCode::Down, NONE);
    }
    key(&mut a, KeyCode::Enter, NONE);
    assert!(matches!(a.mode, Mode::HfEdit(_)));
    typ(&mut a, "Confidential\nDraft");
    key(&mut a, KeyCode::F(10), NONE);
    let Mode::Form(f) = &a.mode else {
        panic!("back to Format")
    };
    assert_eq!(
        f.fields[fi].display(),
        "{center}Confidential  (+1 more line)".replace("{center}", "")
    );
    key(&mut a, KeyCode::F(10), NONE);
    assert_eq!(a.ed.doc.footer, "{center}Confidential\n{center}Draft");
    // Word round trip.
    crate::io::save(&a.ed.doc, &path).unwrap();
    let d = crate::io::load(&path).unwrap();
    assert_eq!(d.header, a.ed.doc.header);
    assert_eq!(d.footer, a.ed.doc.footer);
}

/// Uses the real desktop clipboard (restores it afterwards):
/// cargo test real_clipboard -- --ignored
#[test]
#[ignore]
fn real_clipboard_round_trip() {
    use crate::clip::SysClip;
    let mut probe = SysClip::new();
    let saved = probe.get().map(|(t, _)| t);
    // "Copy from a website": HTML with formatting and a link.
    let web = r#"<meta charset="utf-8"><h2>Release notes</h2><p>The <b>new</b> version adds <a href="https://example.com/">links</a>.</p><ul><li>Fast</li><li>Small</li></ul>"#;
    let mut browser = arboard::Clipboard::new().expect("desktop clipboard");
    browser
        .set_html(
            web,
            Some("Release notes\nThe new version adds links.\nFast\nSmall"),
        )
        .unwrap();
    let mut a = app(Keymap::Modern);
    a.sysclip = Some(SysClip::new());
    key(&mut a, KeyCode::Char('v'), CTRL);
    let text = a.ed.doc.to_plain_text();
    eprintln!("pasted: {text:?}");
    assert!(
        text.contains("Release notes\nThe new version adds links.\nFast\nSmall"),
        "{text}"
    );
    assert!(
        a.ed.doc
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Para(p) if p.props.list.is_some()))
    );
    // Copy from WordBench: other programs see the text (Klipper too).
    a.ed.select_all();
    key(&mut a, KeyCode::Char('c'), CTRL);
    let mut other = SysClip::new();
    let (got, html) = other.get().expect("clipboard readable");
    eprintln!(
        "other program sees text {got:?}, html {}",
        html.as_deref().map(|h| h.len()).unwrap_or(0)
    );
    assert!(got.contains("The new version adds links."));
    assert!(html.is_some_and(|h| h.contains("<b>new</b>") && h.contains("https://example.com/")));
    // Put the original clipboard back.
    if let Some(t) = saved {
        let _ = std::process::Command::new("qdbus6")
            .args(["org.kde.klipper", "/klipper", "setClipboardContents", &t])
            .status();
        let mut cb = arboard::Clipboard::new().unwrap();
        let _ = cb.set_text(t);
    }
}

#[test]
fn common_fonts_are_offered_even_if_not_installed() {
    let mut a = app(Keymap::Classic);
    key(&mut a, KeyCode::F(8), CTRL);
    key(&mut a, KeyCode::Enter, NONE);
    typ(&mut a, "times new roman");
    let Mode::FontPick(fp) = &a.mode else {
        panic!()
    };
    assert_eq!(
        fp.items().first().copied().flatten().map(String::as_str),
        Some("Times New Roman")
    );
    let s = screen(&mut a, 100, 30);
    eprintln!(
        "{}",
        s.lines()
            .find(|l| l.contains("Times New Roman"))
            .unwrap_or("")
    );
    key(&mut a, KeyCode::Enter, NONE);
    key(&mut a, KeyCode::F(10), NONE);
    typ(&mut a, "x");
    let st = a.ed.doc.store.borrow();
    assert_eq!(
        st.look(a.ed.doc.para(ParaRef::top(0)).cells[0].cp)
            .font
            .as_deref(),
        Some("Times New Roman")
    );
}

#[test]
fn key_reference_doc_is_current() {
    let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/KEYS.md"))
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        doc == crate::commands::key_reference(),
        "docs/KEYS.md is out of date: run `cargo run -- --keys > docs/KEYS.md`"
    );
}

/// The screen shown in the README.
#[test]
#[ignore]
fn show_readme_screen() {
    let mut a = app(Keymap::Classic);
    key(&mut a, KeyCode::F(6), KeyModifiers::SHIFT);
    key(&mut a, KeyCode::F(6), NONE);
    typ(&mut a, "Quarterly Report");
    key(&mut a, KeyCode::F(6), NONE);
    typ(
        &mut a,
        "\n\nSales grew in every region this quarter. North led with a ",
    );
    key(&mut a, KeyCode::F(8), NONE);
    typ(&mut a, "twenty percent");
    key(&mut a, KeyCode::F(8), NONE);
    typ(
        &mut a,
        " rise, and the new office opened on time.\n\nNext steps:\n",
    );
    a.run(crate::commands::Cmd::RevealCodes);
    screen(&mut a, 80, 20);
}
