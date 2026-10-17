//! Every user-level action is a `Cmd`. Keymaps, menus and the help screen
//! are all built from this one list.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Cmd {
    New,
    Open,
    ListFiles,
    Save,
    SaveAs,
    ExportPdf,
    PrintMenu,
    Print,
    Preview,
    Exit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Block,
    MoveMenu,
    InsertDate,
    Find,
    FindPrev,
    FindNext,
    Replace,
    Bold,
    Italic,
    Underline,
    Strike,
    Superscript,
    Subscript,
    ClearFmt,
    FontMenu,
    FontDialog,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignFull,
    Indent,
    Outdent,
    LineSpacing,
    StyleMenu,
    FormatMenu,
    BulletList,
    NumberList,
    PageBreak,
    TableMenu,
    InsertTable,
    TableAddRow,
    TableAddCol,
    TableDelRow,
    TableDelCol,
    TableDelete,
    FootnoteMenu,
    Footnote,
    EditFootnote,
    Spell,
    Thesaurus,
    WordCount,
    Settings,
    ToggleKeymap,
    ToggleTheme,
    RevealCodes,
    Help,
    MenuBar,
    InsertLink,
    FollowLink,
    RemoveLink,
    InsertComment,
    Comments,
    EditComment,
    DeleteComment,
    TrackChanges,
    NextChange,
    AcceptChange,
    RejectChange,
    AcceptAll,
    RejectAll,
    ReviewMenu,
    Endnote,
    Properties,
    InsertPicture,
    CommandPalette,
    Support,
    SelectParagraph,
    InsertCaption,
    UpdateFields,
    PaperOutline,
    SideBySide,
    EditHeader,
    EditFooter,
    PastePlain,
}

impl Cmd {
    pub fn label(self) -> &'static str {
        use Cmd::*;
        match self {
            New => "New document",
            Open => "Open...",
            ListFiles => "List files...",
            Save => "Save",
            SaveAs => "Save as...",
            ExportPdf => "Export PDF...",
            PrintMenu => "Print...",
            Print => "Print to printer",
            Preview => "Print preview",
            Exit => "Exit",
            Undo => "Undo",
            Redo => "Redo",
            Cut => "Cut",
            Copy => "Copy",
            Paste => "Paste",
            SelectAll => "Select all",
            Block => "Block (select)",
            MoveMenu => "Move/copy block...",
            InsertDate => "Date stamp (today)",
            Find => "Find...",
            FindPrev => "Find previous",
            FindNext => "Find next",
            Replace => "Replace...",
            Bold => "Bold",
            Italic => "Italic",
            Underline => "Underline",
            Strike => "Strikeout",
            Superscript => "Superscript",
            Subscript => "Subscript",
            ClearFmt => "Normal (clear formatting)",
            FontMenu => "Appearance...",
            FontDialog => "Font...",
            AlignLeft => "Align left",
            AlignCenter => "Center",
            AlignRight => "Flush right",
            AlignFull => "Justify (full)",
            Indent => "Indent",
            Outdent => "Outdent",
            LineSpacing => "Line spacing 1/2",
            StyleMenu => "Paragraph style...",
            FormatMenu => "Format (line/page/document)...",
            BulletList => "Bulleted list",
            NumberList => "Numbered list",
            PageBreak => "Hard page break",
            TableMenu => "Tables...",
            InsertTable => "Create table...",
            TableAddRow => "Insert row",
            TableAddCol => "Insert column",
            TableDelRow => "Delete row",
            TableDelCol => "Delete column",
            TableDelete => "Delete table",
            FootnoteMenu => "Footnotes...",
            Footnote => "Create footnote...",
            EditFootnote => "Edit footnote...",
            Spell => "Spell check",
            Thesaurus => "Thesaurus",
            WordCount => "Word count",
            Settings => "Settings...",
            ToggleKeymap => "Switch keyboard (Classic/Modern)",
            ToggleTheme => "Color theme...",
            RevealCodes => "Reveal codes",
            Help => "Keyboard help",
            MenuBar => "Menu bar",
            InsertLink => "Hyperlink...",
            FollowLink => "Follow hyperlink",
            RemoveLink => "Remove hyperlink",
            InsertComment => "New comment...",
            Comments => "List comments...",
            EditComment => "Edit comment...",
            DeleteComment => "Delete comment",
            TrackChanges => "Track changes on/off",
            NextChange => "Next change",
            AcceptChange => "Accept change",
            RejectChange => "Reject change",
            AcceptAll => "Accept all changes",
            RejectAll => "Reject all changes",
            ReviewMenu => "Review menu",
            Endnote => "Create endnote...",
            Properties => "Document properties...",
            InsertPicture => "Picture...",
            CommandPalette => "All commands...",
            Support => "Support WordBench (buy me a coffee)",
            SelectParagraph => "Select paragraph",
            InsertCaption => "Caption...",
            UpdateFields => "Update fields (caption numbers)",
            PaperOutline => "Paper outline on/off",
            SideBySide => "Pages side by side on/off",
            EditHeader => "Header...",
            EditFooter => "Footer...",
            PastePlain => "Paste unformatted text",
        }
    }
}

/// Every command, for the command list and for checking that each one can
/// be reached from the keyboard.
pub const ALL: &[Cmd] = {
    use Cmd::*;
    &[
        New,
        Open,
        ListFiles,
        Save,
        SaveAs,
        ExportPdf,
        PrintMenu,
        Print,
        Preview,
        Exit,
        Undo,
        Redo,
        Cut,
        Copy,
        Paste,
        SelectAll,
        Block,
        MoveMenu,
        InsertDate,
        Find,
        FindPrev,
        FindNext,
        Replace,
        Bold,
        Italic,
        Underline,
        Strike,
        Superscript,
        Subscript,
        ClearFmt,
        FontMenu,
        FontDialog,
        AlignLeft,
        AlignCenter,
        AlignRight,
        AlignFull,
        Indent,
        Outdent,
        LineSpacing,
        StyleMenu,
        FormatMenu,
        BulletList,
        NumberList,
        PageBreak,
        TableMenu,
        InsertTable,
        TableAddRow,
        TableAddCol,
        TableDelRow,
        TableDelCol,
        TableDelete,
        FootnoteMenu,
        Footnote,
        EditFootnote,
        Spell,
        Thesaurus,
        WordCount,
        Settings,
        ToggleKeymap,
        ToggleTheme,
        RevealCodes,
        Help,
        MenuBar,
        InsertLink,
        FollowLink,
        RemoveLink,
        InsertComment,
        Comments,
        EditComment,
        DeleteComment,
        TrackChanges,
        NextChange,
        AcceptChange,
        RejectChange,
        AcceptAll,
        RejectAll,
        ReviewMenu,
        Endnote,
        Properties,
        InsertPicture,
        CommandPalette,
        Support,
        SelectParagraph,
        InsertCaption,
        UpdateFields,
        PaperOutline,
        SideBySide,
        EditHeader,
        EditFooter,
        PastePlain,
    ]
};

pub type Menu = (&'static str, &'static [Option<Cmd>]);

pub const MENUS: &[Menu] = {
    use Cmd::*;
    &[
        (
            "File",
            &[
                Some(New),
                Some(Open),
                Some(ListFiles),
                Some(Save),
                Some(SaveAs),
                None,
                Some(ExportPdf),
                Some(Print),
                Some(Preview),
                None,
                Some(Properties),
                None,
                Some(Exit),
            ],
        ),
        (
            "Edit",
            &[
                Some(Undo),
                Some(Redo),
                None,
                Some(Cut),
                Some(Copy),
                Some(Paste),
                Some(PastePlain),
                None,
                Some(Block),
                Some(MoveMenu),
                Some(SelectParagraph),
                Some(SelectAll),
            ],
        ),
        (
            "Search",
            &[Some(Find), Some(FindNext), Some(FindPrev), Some(Replace)],
        ),
        (
            "Format",
            &[
                Some(FontDialog),
                Some(FontMenu),
                Some(Bold),
                Some(Italic),
                Some(Underline),
                Some(Strike),
                Some(Superscript),
                Some(Subscript),
                Some(ClearFmt),
                None,
                Some(AlignLeft),
                Some(AlignCenter),
                Some(AlignRight),
                Some(AlignFull),
                None,
                Some(Indent),
                Some(Outdent),
                Some(LineSpacing),
                Some(StyleMenu),
                None,
                Some(EditHeader),
                Some(EditFooter),
                Some(FormatMenu),
            ],
        ),
        (
            "Insert",
            &[
                Some(PageBreak),
                Some(BulletList),
                Some(NumberList),
                None,
                Some(InsertTable),
                Some(TableAddRow),
                Some(TableAddCol),
                Some(TableDelRow),
                Some(TableDelCol),
                Some(TableDelete),
                None,
                Some(InsertPicture),
                Some(InsertCaption),
                Some(UpdateFields),
                Some(InsertLink),
                Some(Footnote),
                Some(Endnote),
                Some(EditFootnote),
                Some(InsertDate),
            ],
        ),
        (
            "Tools",
            &[
                Some(Spell),
                Some(Thesaurus),
                Some(WordCount),
                None,
                Some(Settings),
            ],
        ),
        (
            "Review",
            &[
                Some(TrackChanges),
                Some(NextChange),
                Some(AcceptChange),
                Some(RejectChange),
                Some(AcceptAll),
                Some(RejectAll),
                None,
                Some(InsertComment),
                Some(Comments),
                Some(EditComment),
                Some(DeleteComment),
                None,
                Some(FollowLink),
                Some(RemoveLink),
            ],
        ),
        (
            "View",
            &[
                Some(RevealCodes),
                Some(Preview),
                None,
                Some(ToggleKeymap),
                Some(ToggleTheme),
                Some(PaperOutline),
                Some(SideBySide),
            ],
        ),
        (
            "Help",
            &[Some(CommandPalette), Some(Help), None, Some(Support)],
        ),
    ]
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Keymap {
    Classic,
    #[default]
    Modern,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Normalize a key event so that bindings match regardless of how the
/// terminal reports shifted control keys.
pub fn chord(ev: &KeyEvent) -> Chord {
    let m = ev.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let mut shift = m.contains(KeyModifiers::SHIFT);
    let code = match ev.code {
        KeyCode::Char(c) if ctrl || alt => {
            shift = shift || c.is_uppercase();
            KeyCode::Char(c.to_ascii_lowercase())
        }
        KeyCode::Char(c) => {
            shift = false;
            KeyCode::Char(c)
        }
        KeyCode::BackTab => {
            shift = true;
            KeyCode::Tab
        }
        c => c,
    };
    Chord {
        code,
        ctrl,
        alt,
        shift,
    }
}

const fn ch(code: KeyCode, ctrl: bool, alt: bool, shift: bool) -> Chord {
    Chord {
        code,
        ctrl,
        alt,
        shift,
    }
}
const fn f(n: u8) -> Chord {
    ch(KeyCode::F(n), false, false, false)
}
const fn sf(n: u8) -> Chord {
    ch(KeyCode::F(n), false, false, true)
}
const fn af(n: u8) -> Chord {
    ch(KeyCode::F(n), false, true, false)
}
const fn cf(n: u8) -> Chord {
    ch(KeyCode::F(n), true, false, false)
}
const fn ctrl(c: char) -> Chord {
    ch(KeyCode::Char(c), true, false, false)
}
const fn ctrl_shift(c: char) -> Chord {
    ch(KeyCode::Char(c), true, false, true)
}
const fn ctrl_alt(c: char) -> Chord {
    ch(KeyCode::Char(c), true, true, false)
}
const fn alt(c: char) -> Chord {
    ch(KeyCode::Char(c), false, true, false)
}

/// Bindings available in both keymaps.
const COMMON: &[(Chord, Cmd)] = &[
    (ctrl('z'), Cmd::Undo),
    (ctrl('y'), Cmd::Redo),
    (ctrl('c'), Cmd::Copy),
    (ctrl('x'), Cmd::Cut),
    (ctrl('v'), Cmd::Paste),
    (ch(KeyCode::Enter, true, false, false), Cmd::PageBreak),
    (alt('k'), Cmd::ToggleKeymap),
    (ctrl('k'), Cmd::InsertLink),
    (alt('x'), Cmd::CommandPalette),
    (ctrl_shift('p'), Cmd::CommandPalette),
    (ctrl_shift('e'), Cmd::TrackChanges),
];

/// The classic function-key layout of DOS-era word processors.
const CLASSIC: &[(Chord, Cmd)] = &[
    (f(2), Cmd::Find),
    (sf(2), Cmd::FindPrev),
    (af(2), Cmd::Replace),
    (f(3), Cmd::Help),
    (af(3), Cmd::RevealCodes),
    (f(11), Cmd::RevealCodes),
    (f(4), Cmd::Indent),
    (sf(4), Cmd::Outdent),
    (af(4), Cmd::Block),
    (f(12), Cmd::Block),
    (cf(4), Cmd::MoveMenu),
    (f(5), Cmd::ListFiles),
    (sf(5), Cmd::InsertDate),
    (f(6), Cmd::Bold),
    (sf(6), Cmd::AlignCenter),
    (af(6), Cmd::AlignRight),
    (f(7), Cmd::Exit),
    (sf(7), Cmd::PrintMenu),
    (af(7), Cmd::TableMenu),
    (cf(7), Cmd::FootnoteMenu),
    (cf(5), Cmd::InsertComment),
    (af(5), Cmd::ReviewMenu),
    (f(8), Cmd::Underline),
    (sf(8), Cmd::FormatMenu),
    (af(8), Cmd::StyleMenu),
    (cf(8), Cmd::FontDialog),
    (f(10), Cmd::Save),
    (sf(10), Cmd::Open),
    (cf(2), Cmd::Spell),
    (af(1), Cmd::Thesaurus),
    (sf(1), Cmd::Settings),
    (alt('='), Cmd::MenuBar),
    (f(9), Cmd::MenuBar),
    (ctrl('b'), Cmd::Bold),
    (ctrl('i'), Cmd::Italic),
    (ctrl('u'), Cmd::Underline),
];

/// Word-style layout.
const MODERN: &[(Chord, Cmd)] = &[
    (ctrl('n'), Cmd::New),
    (ctrl('o'), Cmd::Open),
    (ctrl('s'), Cmd::Save),
    (ctrl_shift('s'), Cmd::SaveAs),
    (f(12), Cmd::SaveAs),
    (ctrl('p'), Cmd::PrintMenu),
    (ctrl('q'), Cmd::Exit),
    (ctrl('a'), Cmd::SelectAll),
    (ctrl('f'), Cmd::Find),
    (f(3), Cmd::FindNext),
    (sf(3), Cmd::FindPrev),
    (ctrl('h'), Cmd::Replace),
    (ctrl('r'), Cmd::AlignRight),
    (ctrl('b'), Cmd::Bold),
    (ctrl('i'), Cmd::Italic),
    (ctrl('u'), Cmd::Underline),
    (ctrl_shift('x'), Cmd::Strike),
    (ctrl_shift('='), Cmd::Superscript),
    (ctrl('+'), Cmd::Superscript),
    (ctrl('='), Cmd::Subscript),
    (ctrl(' '), Cmd::ClearFmt),
    (ctrl('l'), Cmd::AlignLeft),
    (ctrl('e'), Cmd::AlignCenter),
    (ctrl('j'), Cmd::AlignFull),
    (ctrl('m'), Cmd::Indent),
    (ctrl_shift('m'), Cmd::Outdent),
    (ctrl('2'), Cmd::LineSpacing),
    (ctrl_shift('l'), Cmd::BulletList),
    (ctrl_shift('n'), Cmd::NumberList),
    (ctrl('t'), Cmd::InsertTable),
    (ctrl_alt('f'), Cmd::Footnote),
    (ctrl_alt('d'), Cmd::Endnote),
    (ctrl_alt('m'), Cmd::InsertComment),
    (ctrl_shift('d'), Cmd::InsertDate),
    (ctrl('d'), Cmd::FormatMenu),
    (ctrl_shift('f'), Cmd::FontDialog),
    (f(7), Cmd::Spell),
    (sf(7), Cmd::Thesaurus),
    (f(11), Cmd::RevealCodes),
    (af(3), Cmd::RevealCodes),
    (f(10), Cmd::MenuBar),
    (f(1), Cmd::Help),
    (ctrl('w'), Cmd::WordCount),
    (ctrl(','), Cmd::Settings),
];

pub fn bindings(km: Keymap) -> impl Iterator<Item = &'static (Chord, Cmd)> {
    let specific = match km {
        Keymap::Classic => CLASSIC,
        Keymap::Modern => MODERN,
    };
    specific.iter().chain(COMMON.iter())
}

pub fn lookup(km: Keymap, c: Chord) -> Option<Cmd> {
    bindings(km).find(|(k, _)| *k == c).map(|(_, cmd)| *cmd)
}

pub fn chord_label(c: &Chord) -> String {
    let mut s = String::new();
    if c.ctrl {
        s.push_str("Ctrl+");
    }
    if c.alt {
        s.push_str("Alt+");
    }
    if c.shift {
        s.push_str("Shift+");
    }
    match c.code {
        KeyCode::F(n) => s.push_str(&format!("F{n}")),
        KeyCode::Char(' ') => s.push_str("Space"),
        KeyCode::Char(ch) => s.push(ch.to_ascii_uppercase()),
        KeyCode::Enter => s.push_str("Enter"),
        KeyCode::Tab => s.push_str("Tab"),
        other => s.push_str(&format!("{other:?}")),
    }
    s
}

/// The first key bound to a command, for menus and help.
pub fn key_for(km: Keymap, cmd: Cmd) -> Option<String> {
    bindings(km)
        .find(|(_, c)| *c == cmd)
        .map(|(k, _)| chord_label(k))
}

/// Alt+letter opens the menu with that initial.
/// Access letter of each menu (Alt+letter, or the letter while the menu
/// bar is open): unique, as in Word (Alt+O for Format).
pub const MENU_KEYS: [char; 9] = ['f', 'e', 's', 'o', 'i', 't', 'r', 'v', 'h'];

pub fn menu_for_key(ch: char) -> Option<usize> {
    let ch = ch.to_ascii_lowercase();
    MENU_KEYS
        .iter()
        .position(|&k| k == ch)
        .filter(|&i| i < MENUS.len())
}

pub fn menu_for_alt(c: Chord) -> Option<usize> {
    if !c.alt || c.ctrl {
        return None;
    }
    let KeyCode::Char(ch) = c.code else {
        return None;
    };
    menu_for_key(ch)
}

/// Access letter of each item in a menu: the first letter of its label not
/// used by an earlier item (None for separators or if none is free).
pub fn item_keys(m: usize) -> Vec<Option<(usize, char)>> {
    let mut used: Vec<char> = Vec::new();
    MENUS[m]
        .1
        .iter()
        .map(|it| {
            let cmd = (*it)?;
            let label = cmd.label();
            // Prefer the first letter of a word, then any letter.
            let starts = label
                .char_indices()
                .filter(|(i, c)| c.is_alphanumeric() && (*i == 0 || label[..*i].ends_with(' ')));
            let any = label.char_indices().filter(|(_, c)| c.is_alphanumeric());
            let found = starts
                .chain(any)
                .find(|(_, c)| !used.contains(&c.to_ascii_lowercase()));
            let (i, c) = found?;
            used.push(c.to_ascii_lowercase());
            Some((label[..i].chars().count(), c.to_ascii_lowercase()))
        })
        .collect()
}

/// Every key bound to a command in one keymap, e.g. "F6, Ctrl+B".
fn keys_for(km: Keymap, cmd: Cmd) -> String {
    let mut v: Vec<String> = Vec::new();
    for (k, c) in bindings(km) {
        let l = chord_label(k);
        if *c == cmd && !v.contains(&l) {
            v.push(l);
        }
    }
    if v.is_empty() {
        "—".into()
    } else {
        v.join(", ")
    }
}

/// The full key reference as Markdown, grouped by menu (`wordbench --keys`,
/// and docs/KEYS.md, which a test keeps in step with this).
pub fn key_reference() -> String {
    let mut s = String::from(
        "# WordBench key reference\n\n\
         Generated by `wordbench --keys`. Every command is also on the menu bar \
         (F9 or Alt+= in the Classic keymap, F10 in the Modern one, or \
         Alt+the menu's letter in both) and in the command palette.\n",
    );
    let row = |s: &mut String, cmd: Cmd| {
        s.push_str(&format!(
            "| {} | {} | {} |\n",
            cmd.label().replace('|', "\\|"),
            keys_for(Keymap::Classic, cmd),
            keys_for(Keymap::Modern, cmd)
        ));
    };
    let head = "| Command | Classic keys | Modern keys |\n|---|---|---|\n";
    let mut listed: Vec<Cmd> = Vec::new();
    for (i, (name, items)) in MENUS.iter().enumerate() {
        s.push_str(&format!(
            "\n## {name} menu (Alt+{})\n\n{head}",
            MENU_KEYS[i].to_ascii_uppercase()
        ));
        for cmd in items.iter().flatten() {
            row(&mut s, *cmd);
            listed.push(*cmd);
        }
    }
    let rest: Vec<Cmd> = ALL
        .iter()
        .copied()
        .filter(|c| !listed.contains(c))
        .collect();
    if !rest.is_empty() {
        s.push_str(&format!("\n## Other commands\n\n{head}"));
        for cmd in rest {
            row(&mut s, cmd);
        }
    }
    s
}
