# WordBench User Guide

WordBench is a word processor that runs in a terminal window. It looks and
works like the classic DOS word processors and reads and writes Microsoft Word files. This
guide covers everything from installing it to tracked changes. For a
one-page list of keys, see [KEYS.md](KEYS.md) or run `wordbench --keys`.

## Contents

1. [Installing](#installing)
2. [Starting WordBench](#starting-wordbench)
3. [The screen](#the-screen)
4. [Two keyboards](#two-keyboards)
5. [Menus, the command palette and help](#menus-the-command-palette-and-help)
6. [Typing and moving around](#typing-and-moving-around)
7. [Selecting, cutting, copying and pasting](#selecting-cutting-copying-and-pasting)
8. [Undo and redo](#undo-and-redo)
9. [Find and replace](#find-and-replace)
10. [Character formatting](#character-formatting)
11. [Paragraph formatting](#paragraph-formatting)
12. [Reveal Codes](#reveal-codes)
13. [Lists](#lists)
14. [Tables](#tables)
15. [Pictures and captions](#pictures-and-captions)
16. [Pages: paper, margins and breaks](#pages-paper-margins-and-breaks)
17. [Headers, footers and page numbers](#headers-footers-and-page-numbers)
18. [Footnotes and endnotes](#footnotes-and-endnotes)
19. [Hyperlinks](#hyperlinks)
20. [Comments and tracked changes](#comments-and-tracked-changes)
21. [Spell check and thesaurus](#spell-check-and-thesaurus)
22. [Files: opening, saving and converting](#files-opening-saving-and-converting)
23. [Printing and PDF](#printing-and-pdf)
24. [Color themes and the view](#color-themes-and-the-view)
25. [Settings](#settings)
26. [Terminal notes](#terminal-notes)
27. [Troubleshooting](#troubleshooting)

---

## Installing

### From a release download

Download the archive for your system from the [Releases page](https://github.com/william-lampley/WordBench/releases/latest):

- **Windows:** unzip `wordbench-…-x86_64-pc-windows-msvc.zip`. Move
  `wordbench.exe` and `wb.exe` into a folder on your `PATH`, or run them
  from where you unzipped them. Run WordBench in **Windows Terminal**,
  which comes with Windows 11 and is in the Microsoft Store for Windows 10.
- **macOS:** unpack the `.tar.gz` for your Mac. Use `aarch64` for Apple
  silicon (M1 and later) and `x86_64` for Intel. Then move the programs
  onto your path:

  ```sh
  tar xzf wordbench-*-apple-darwin.tar.gz
  cd wordbench-*-apple-darwin
  xattr -d com.apple.quarantine wordbench wb   # allow the downloaded programs
  sudo mv wordbench wb /usr/local/bin/
  ```

- **Linux:** unpack the `.tar.gz`. Copy `wordbench` and `wb` to
  `~/.local/bin` or `/usr/local/bin`. To get a menu entry, copy
  `wordbench.desktop` to `~/.local/share/applications/`. It opens
  WordBench in your default terminal.

### From source

Install Rust from <https://rustup.rs> (version 1.88 or newer). Then, in a
copy of the WordBench source:

```sh
cargo install --path .
```

This builds and installs `wordbench` and `wb` into Cargo's `bin` folder.
That folder is `~/.cargo/bin`, or `%USERPROFILE%\.cargo\bin` on Windows,
and rustup adds it to your `PATH`.

---

## Starting WordBench

```sh
wordbench                  # start with a new, empty document
wordbench letter.docx      # open a file
wb letter.docx             # `wb` is a shorter name for the same program
```

Command-line options:

| Option | Meaning |
|---|---|
| `--classic` | Start with the Classic (function-key) keyboard |
| `--modern` | Start with the Modern (Word-style) keyboard |
| `--theme NAME` | Color theme: `blue`, `dark`, `light`, `solarized`, `amber`, `green`, `contrast` or `plain` |
| `--plain` | Use the terminal's own colors |
| `--convert IN OUT` | Convert a file without opening the editor, e.g. `wordbench --convert report.docx report.pdf` |
| `--keys` | Print every command and its keys |
| `-h`, `--help` | Show help and the settings folder |
| `-V`, `--version` | Show the version |

Options given on the command line apply to that session. To change the
defaults, use Tools > Settings (see [Settings](#settings)).

---

## The screen

- **The document area** shows your text as it will print. The page width
  comes from the paper size and margins. A faint outline marks the edges of
  the sheet and where each page ends. When the window is wide enough,
  pages appear **side by side**.
- **The menu bar** (top line) appears when you open it, or all the time
  with the Modern keyboard.
- **The status line** (bottom line) shows the file name, the font at the
  cursor, and where you are. With the Classic keyboard it reads like a
  classic word processor: `Doc 1 Pg 1 Ln 1" Pos 1"`, where Ln and Pos are distances in
  inches from the top and left of the page. With the Modern keyboard it
  shows page, line, column and word count. `TRK` appears while changes are
  being tracked.
- **Reveal Codes** (Alt+F3 or F11) splits the screen to show formatting
  codes under the text. See [Reveal Codes](#reveal-codes).

---

## Two keyboards

WordBench has two complete sets of keys. Press **Alt+K** at any time to
switch between them. Your choice is remembered.

**Classic** uses the function keys the way the classic DOS word
processors did:

| Key | Alone | Shift | Alt | Ctrl |
|---|---|---|---|---|
| F1 | — | Setup | Thesaurus | — |
| F2 | Search | Search backward | Replace | Spell |
| F3 | Help | — | Reveal Codes | — |
| F4 | Indent | Outdent | Block | Move/copy block |
| F5 | List files | Date stamp | Review menu | New comment |
| F6 | Bold | Center | Flush right | — |
| F7 | Exit | Print | Tables | Footnotes |
| F8 | Underline | Format | Paragraph style | Font |
| F9 | Menu bar | — | — | — |
| F10 | Save | Open | — | — |
| F11 | Reveal Codes | | | |
| F12 | Block | | | |

**Modern** uses the keys of today's word processors: Ctrl+S save, Ctrl+O
open, Ctrl+Q quit, Ctrl+B/I/U bold/italic/underline, Ctrl+E/L/R/J
alignment, Ctrl+F find, Ctrl+H replace, Ctrl+P print, F7 spelling, F10 the
menu bar and F1 help.

Some keys work the same in both: Ctrl+Z undo, Ctrl+Y redo, Ctrl+X/C/V cut,
copy and paste, Ctrl+K hyperlink, Ctrl+Enter page break, Shift+Enter line
break, Alt+X command palette, and Alt+K to switch keyboards.

---

## Menus, the command palette and help

Every command is on the **menu bar**, so you never need to remember a key.

- Open the menu bar with **F9** or **Alt+=** (Classic), **F10**
  (Modern), or **Alt+the menu's letter** in either: Alt+F File, Alt+E
  Edit, Alt+S Search, Alt+O Format, Alt+I Insert, Alt+T Tools, Alt+R
  Review, Alt+V View, Alt+H Help.
- Move with the arrow keys and press **Enter**, or press the highlighted
  letter of a menu or item. **Esc** closes the menu.
- Menus show each command's key, so you learn the keys as you go.
- You can also click menus and items with the mouse.

The **command palette** (**Alt+X** or **Ctrl+Shift+P**) lists every
command. Type part of a name ("caption", "margin", "theme"), choose with
Up/Down, and press Enter.

**Help** (F3 with the Classic keyboard, F1 with Modern) shows all the
keys for the keyboard you are using.

**Help > Support WordBench** opens the project's
[Buy Me a Coffee](https://buymeacoffee.com/williamlampley) page, if you'd
like to support its development.

In dialogs, use Up/Down (or a field's number) to choose a field and
Enter or Space to change it. Type a value and press Enter to set it. F10
(or F7) accepts the dialog, and Esc cancels.

---

## Typing and moving around

Type normally. Lines wrap by themselves, and **Enter** ends a paragraph.
**Shift+Enter** starts a new line in the same paragraph, and **Tab**
moves to the next tab stop.

| Move | Both keyboards |
|---|---|
| Character, line | Arrow keys |
| Word | Ctrl+Left / Ctrl+Right |
| Paragraph | Ctrl+Up / Ctrl+Down |
| Start / end of line | Home / End |
| Start of document | Ctrl+Home |

| Move | Classic | Modern |
|---|---|---|
| Previous / next page | PgUp / PgDn | — |
| Screen up / down | Home, Up / Home, Down | PgUp / PgDn |
| Start / end of line | Home, Left / Home, Right (or End) | Home / End |
| Start / end of document | Home, Home, Up / Home, Home, Down | Ctrl+Home / Ctrl+End |
| Delete to end of line | Ctrl+End | — |
| Delete word to the left | Ctrl+Backspace | Ctrl+Backspace |

The Classic keyboard keeps the classic **Home** prefix. Press Home
before an arrow to go to the edge of the screen or line, or press Home
twice before Up or Down to go to the top or bottom of the document.

Text is always inserted. There is no typeover mode.

With the mouse, click to place the cursor, drag to select, and use the
wheel to scroll.

---

## Selecting, cutting, copying and pasting

**Selecting.**

- *Modern keyboard:* hold **Shift** while moving (Shift+arrows,
  Shift+Ctrl+arrows, Shift+End). **Ctrl+A** selects everything.
- *Classic keyboard:* press **Alt+F4** or **F12** to turn **Block**
  on (the status line says so). Move the cursor to extend the block, and
  press Alt+F4 again to turn it off.
- Edit > Select paragraph and Edit > Select all work with either keyboard,
  and so does dragging with the mouse.

**Cut, copy and paste.** Ctrl+X, Ctrl+C and Ctrl+V. In the Classic
keyboard, **Ctrl+F4** (Move/copy block) asks whether to move or copy the
block, as the classic word processors did.

**Sharing with other programs.** Copying puts both plain text and
formatted text on your system clipboard. Pasting into a browser, an email
or LibreOffice keeps bold, italics, headings, lists, tables and links.
Pasting into WordBench from a web page or another word processor turns
that formatting into document formatting. To paste only the text, use
**Edit > Paste unformatted text**. Text you copy inside WordBench pastes
back with everything, including footnotes and pictures.

The clipboard works on Windows, macOS, and Linux (Wayland and X11). Over
SSH, WordBench also sends copies to your local terminal's clipboard when
the terminal allows it (OSC 52).

---

## Undo and redo

**Ctrl+Z** undoes and **Ctrl+Y** redoes, as many steps as you like.

---

## Find and replace

| | Classic | Modern |
|---|---|---|
| Find | F2 | Ctrl+F |
| Find next | F2, Enter | F3 |
| Find previous | Shift+F2 | Shift+F3 |
| Replace | Alt+F2 | Ctrl+H |

Replace asks for the text to find and the replacement. Then, at each
match, press **Y** to replace, **N** to skip, or **A** to replace all the
rest. Esc stops.

---

## Character formatting

| | Classic | Modern |
|---|---|---|
| Bold | F6 | Ctrl+B |
| Italic | Ctrl+I | Ctrl+I |
| Underline | F8 | Ctrl+U |
| Strikeout | Format menu | Ctrl+Shift+X |
| Superscript / subscript | Format menu | Ctrl+Shift+= / Ctrl+= |
| Back to normal | Format menu | Ctrl+Space |
| Font dialog | Ctrl+F8 | Ctrl+Shift+F |

With a selection, these change the selected text. Without one, they turn
the style on or off for what you type next. In classic fashion, F6
before typing starts bold, and F6 after stops it.

**Font dialog.** Choose the font name, size, color and highlight. Press
Enter on the font name to open a **searchable list** of fonts, with the
fonts already in the document first. Type any part of a name. Misspellings
are fine: "gergia" finds Georgia. The list includes common document fonts
such as Times New Roman, Arial and Calibri even when they are not
installed. Those show which installed font stands in for them on screen
and in PDF. If nothing matches, Enter uses the name as you typed it.

The font you choose stays in effect as you keep typing, including after
tables and other inserted items.

The terminal shows all text in its own font, so sizes and font faces
don't change the screen. They are saved in `.docx` files and appear in
Word or LibreOffice.

---

## Paragraph formatting

| | Classic | Modern |
|---|---|---|
| Center | Shift+F6 | Ctrl+E |
| Flush right | Alt+F6 | Ctrl+R |
| Left | Format menu | Ctrl+L |
| Justify (full) | Format menu | Ctrl+J |
| Indent / outdent | F4 / Shift+F4 | Ctrl+M / Ctrl+Shift+M |
| Line spacing 1 / 2 | Format menu | Ctrl+2 |
| Paragraph style | Alt+F8 | Format menu |
| Format dialog | Shift+F8 | Ctrl+D |

The **Format** dialog has three parts:

- **Line** (the current paragraph, or every paragraph in a selection):
  justification, line spacing, blank lines before and after, left indent,
  first-line indent and paragraph style.
- **Page:** paper size, orientation and margins. See
  [Pages](#pages-paper-margins-and-breaks).
- **Document:** header, footer and page numbering.

**Paragraph styles** include Title, Heading 1–3 and the styles of a Word
document you opened. Headings keep their style when saved to `.docx`, so
Word's navigation pane and tables of contents see them.

---

## Reveal Codes

**Alt+F3** or **F11** splits the screen. The lower half shows your text
with its formatting codes, and a ruler shows the margins and tab stops
(▲). Codes look like this:

| Code | Meaning |
|---|---|
| `[BOLD]…[bold]` | Start and end of bold (also `[ITALC]`, `[UND]`, `[STKOUT]`…) |
| `[HRt]` | Hard return: the end of a paragraph |
| `[SRt]` | Soft return: where a line wraps |
| `[Tab]` | Tab |
| `[Just:Center]` | Paragraph alignment |
| `[HPg]` | Hard page break |
| `[Tbl Def:3x2]`…`[Tbl Off]` | A table (columns × rows) |
| `[Font:14pt]`, `[Color:Dark red]` | Font changes |
| `[Style:Heading 1]` | Paragraph style |
| `[Footnote:1]`, `[Link:…]`, `[Comment:…]` | Notes, links and comments |

The cursor moves through both halves together, so you can see exactly
which codes surround it. To remove formatting, select the text and turn
the format off again (for example F6 or Ctrl+B for bold), or use Format >
Normal to clear all character formatting. Press Alt+F3 or F11 again to
close Reveal Codes.

---

## Lists

- **Bulleted list:** Insert > Bulleted list (Modern: Ctrl+Shift+L).
- **Numbered list:** Insert > Numbered list (Modern: Ctrl+Shift+N).

Press Enter for the next item, and Enter on an empty item to end the list.
**Tab** at the start of an item nests it one level deeper, and
**Shift+Tab** brings it back. Numbers update by themselves. Lists are saved
as real Word lists.

---

## Tables

**Create a table** with Insert > Create table (Classic: Alt+F7 opens
the Tables menu; Modern: Ctrl+T). WordBench asks for the number of
columns and rows.

- **Tab** moves to the next cell and **Shift+Tab** to the previous one.
  Tab in the last cell adds a row.
- Insert > Insert row / Insert column / Delete row / Delete column /
  Delete table change the table at the cursor.
- Text in a cell wraps within the cell, and cells can hold formatting.

Tables are drawn with lines on screen and saved as real tables in `.docx`
and PDF. Tables cannot contain other tables.

---

## Pictures and captions

**Insert > Picture** asks for an image file and adds it at the cursor.
PNG and JPEG pictures take their real size on the page. GIF pictures are
kept and shown as placeholders.

- In terminals that can show images (kitty, Ghostty, WezTerm, iTerm2,
  foot, mlterm), the picture is drawn in the document.
- In other terminals, a placeholder such as `[Image: logo.png]` shows
  where it is. It still prints and exports to PDF.
- Tools > Settings > Pictures > *Placeholders only* shows one-line
  placeholders everywhere.

**Insert > Caption** opens a caption dialog like Word's:

- **Caption text**, e.g. "Sales by region".
- **Label:** Figure, Table or Equation. Or type a **new label** of your
  own.
- **Position:** above or below the selected item.
- **Numbering:** 1, 2, 3 / a, b, c / A, B, C / i, ii, iii / I, II, III.
- **Exclude label from caption** leaves only the number.

A preview shows the result, e.g. "Table 1: Sales by region". Numbers are
Word fields. They renumber in document order when you save, or right away
with Insert > Update fields.

---

## Pages: paper, margins and breaks

Open the Format dialog (Shift+F8 / Ctrl+D) and go to **Page**:

- **Paper size:** Letter, A4 or Legal, or the custom size of an opened
  document.
- **Orientation:** portrait or landscape.
- **Margins:** left, right, top and bottom, in inches.

**Ctrl+Enter** inserts a hard page break (`[HPg]`).

**Print preview** (File > Print preview) shows whole pages as they will
print. PgUp and PgDn turn pages, the arrows scroll, and Esc closes it.

WordBench lays out pages on a typewriter grid: 10 characters per inch and
6 lines per inch. That is 12-point Courier, the way classic word
processors showed pages in text mode. A Letter page with 1-inch margins holds 65 characters
by 54 lines. The screen, preview and PDF use the same layout, so page
breaks are the same everywhere.

---

## Headers, footers and page numbers

Choose **Format > Header…** or **Format > Footer…**, or press Enter on
Header or Footer in the Format dialog. A small editor opens:

- Type as many lines as you like. **Enter** starts a new line.
- **Tab** moves the rest of the line to the center, and Tab again to the
  right edge. One line can have left, centered and right parts, e.g. a
  title on the left and the page number on the right.
- **Ctrl+P** inserts the page number, **Ctrl+N** the number of pages
  ("Page 3 of 10"), and **Ctrl+D** the date.
- A preview shows how it will look. **F10** saves and **Esc** cancels.

**Page numbering** in the Format dialog adds a plain page number (bottom
center, bottom right or top right) if you don't want a full header or
footer.

In `.docx` files, headers and footers are saved as Word headers and
footers, with real page-number fields.

---

## Footnotes and endnotes

| | Classic | Modern |
|---|---|---|
| Footnote | Ctrl+F7 (Footnotes menu) | Ctrl+Alt+F |
| Endnote | Insert menu | Ctrl+Alt+D |
| Edit a footnote | Insert > Edit footnote | Insert > Edit footnote |

Type the note's text and press Enter. A number appears at the cursor, and
the note is printed at the bottom of its page (footnotes) or at the end of
the document (endnotes). Notes renumber when you add or delete one.

---

## Hyperlinks

Select the text to link and press **Ctrl+K**. Then type a web address,
or `#name` to link to a bookmark in the document. **Review > Follow hyperlink** opens the link in your web browser,
and **Review > Remove hyperlink** turns it back into plain text. Links are
saved as real hyperlinks in `.docx` and PDF.

---

## Comments and tracked changes

Both are on the **Review** menu (Alt+R; Classic also Alt+F5).

**Comments**

- New comment: Ctrl+F5 (Classic) or Ctrl+Alt+M (Modern). Type the
  comment and press Enter. It is attached to the selected text, or to the
  cursor position.
- List comments shows every comment with its author. Choose one to jump
  to it.
- Edit comment and Delete comment act on the comment at the cursor.

Comments from Word files appear as `[Comment: name]` in the text.
Comments you add carry your name. Set it in [Settings](#settings).

**Tracked changes**

- **Ctrl+Shift+E** turns tracking on or off. `TRK` shows in the status
  line while tracking is on.
- While tracking, insertions are underlined (green) and deletions are shown
  struck through (red) instead of disappearing.
- **Next change** jumps to the next change. **Accept change** or
  **Reject change** handles that one, and **Accept all** or **Reject all**
  handles every change at once.

Changes made in Word appear the same way and can be accepted or rejected
in WordBench, and Word sees the changes you track in WordBench.

---

## Spell check and thesaurus

| | Classic | Modern |
|---|---|---|
| Spell check | Ctrl+F2 | F7 |
| Thesaurus | Alt+F1 | Shift+F7 |
| Word count | Tools menu | Ctrl+W |

**Spell check** goes through the document from the top, or from the start
of the selection. It stops at each unknown word and lists suggestions,
each with a letter. Press the letter to use that suggestion, or:

- **1 Skip once** goes past this occurrence.
- **2 Skip** ignores the word everywhere in this session.
- **3 Add** adds it to your personal dictionary, `personal.dic` in the
  settings folder.
- **4 Edit** lets you type the correction.
- **Esc** (or F7) stops.

**Word count** shows the number of words, characters and pages.

**Thesaurus** shows synonyms for the word at the cursor, grouped by
meaning. Press Enter to replace the word, or **L** to look up the selected
synonym.

WordBench uses spelling and thesaurus programs that may already be on your
computer:

**Linux**

```sh
sudo apt install aspell-en mythes-en-us      # Debian, Ubuntu
sudo dnf install aspell-en mythes-en         # Fedora
```

Hunspell (`hunspell-en-us`) or WordNet (`wordnet`) work too.

**macOS** (with [Homebrew](https://brew.sh)):

```sh
brew install aspell wordnet
```

**Windows:** install Hunspell with `scoop install hunspell` or `choco install
hunspell.portable`. Put `en_US.dic` and `en_US.aff` where Hunspell can find
them: its own dictionary folder, or the folder named in the `DICPATH`
environment variable. Without a spell checker, WordBench can also check
against a plain list of words, one per line, saved as `words.txt` in the
settings folder.

**Thesaurus on any system:** copy a MyThes thesaurus (`th_en_US_v2.dat`
and `th_en_US_v2.idx`) into the settings folder. LibreOffice's English
dictionary extension includes these files. You can also point the
`WORDBENCH_THESAURUS` environment variable at a `.dat` file.

The spelling language (default `en_US`) is set in Tools > Settings.

---

## Files: opening, saving and converting

| | Classic | Modern |
|---|---|---|
| Open | Shift+F10 | Ctrl+O |
| List files | F5 | File menu |
| Save | F10 | Ctrl+S |
| Save as | File menu | Ctrl+Shift+S or F12 |
| New document | File menu | Ctrl+N |
| Exit | F7 | Ctrl+Q |

**List files** (F5) shows the files in the document's folder. Move to a
file and press Enter to open it. Choose a folder, or `..` for the parent
folder, to look in another folder.

File names can start with `~` for your home folder (also `~\` on Windows).

**Saving.** When you type a file name without an extension, WordBench asks
which type to save:

| Type | Extension | Keeps |
|---|---|---|
| Word document | `.docx` | Everything. Best for sharing |
| WordBench document | `.wbd` | WordBench's own format (JSON) |
| Plain text | `.txt` | Text only |
| Markdown | `.md` | Text only |
| PDF | `.pdf` | The printed pages |

When you exit with unsaved changes, WordBench asks whether to save them.

**Word files.** WordBench opens `.docx` files with their styles, lists,
fonts, colors, spacing, tab stops, tables, pictures, headers and footers,
footnotes, comments and tracked changes. It does not throw away anything
it doesn't edit. Charts, equations, text boxes, content controls,
bookmarks, fields, themes and macros are all kept and written back as they
were. If you open a Word file and save it without changes, nothing in it
is lost. Word or LibreOffice may still lay out the file a little
differently from WordBench's typewriter grid.

**Converting without opening.** `--convert` converts between any of the
types above:

```sh
wordbench --convert report.docx report.pdf
wordbench --convert notes.txt notes.docx
```

**Document properties** (File menu) shows and edits the title, subject,
author, keywords and comments stored in the file.

---

## Printing and PDF

**File > Export PDF** saves a PDF of the document. **Print** (Shift+F7 /
Ctrl+P) opens the print menu: *Print to printer*, *Export PDF* or *Print
preview*.

Printing to a printer:

- **Linux:** through CUPS (`lp` or `lpr`). Most desktop systems already
  have it.
- **macOS:** through the built-in `lp` command to your default printer.
- **Windows:** WordBench makes a PDF and asks Windows to print it with the
  program that opens PDFs (for example Edge or Acrobat).

---

## Color themes and the view

**View > Color theme** opens the theme picker. Moving up and down
previews each theme on your document right away. **Enter** keeps the one
you're on, and **Esc** goes back to the theme you had. **O** turns the
paper outline on or off.

| Theme | Look |
|---|---|
| Classic blue | The classic white-on-blue DOS screen |
| Dark | Light text on a dark gray background |
| Light (paper) | Dark text on white |
| Solarized dark | The Solarized palette |
| Amber screen | An amber monochrome monitor |
| Green screen | A green monochrome monitor |
| High contrast | Maximum contrast for low vision |
| Terminal colors | Your terminal's own colors and font colors |

Every theme keeps tables, captions, notes, links, tracked changes and
codes readable.

Also on the View menu:

- **Reveal codes** (Alt+F3 / F11)
- **Print preview**
- **Switch keyboard** (Alt+K)
- **Paper outline on/off:** faint edges around the sheet
- **Pages side by side on/off:** show two or more pages next to each other
  when the window is wide enough

---

## Settings

**Tools > Settings** (Classic: Shift+F1, Modern: Ctrl+,):

| Setting | Choices |
|---|---|
| Keyboard | Classic (function keys) or Modern (Ctrl keys) |
| Colors | Any theme |
| Spelling language | e.g. `en_US`, `en_GB`, `de_DE` (a dictionary must be installed) |
| Your name | Used for comments and tracked changes |
| Paper outline | On or off |
| Pages | Side by side when there's room, or one at a time |
| Pictures | Full size, or placeholders only |

Settings are saved in a settings folder, along with your personal
dictionary:

| System | Settings folder |
|---|---|
| Windows | `%APPDATA%\WordBench` |
| macOS | `~/Library/Application Support/WordBench` |
| Linux | `~/.config/wordbench` (or `$XDG_CONFIG_HOME/wordbench`) |

`wordbench --help` prints the folder's location.

Environment variables:

| Variable | Effect |
|---|---|
| `WORDBENCH_GRAPHICS` | `kitty`, `sixel` or `none`: how to draw pictures, overriding the automatic guess |
| `WORDBENCH_THESAURUS` | Path to a MyThes `.dat` thesaurus file |

---

## Terminal notes

WordBench needs a terminal of at least 80×24 characters. Larger is better,
and wide windows show pages side by side.

**Windows.** Use **Windows Terminal**. It passes through all the function
keys, Alt and Ctrl combinations, the mouse and 24-bit color. The old
console window (conhost) works, but with fewer key combinations. Windows
Terminal grabs a few keys for itself (Alt+Enter, Ctrl+Shift+P,
Ctrl+Shift+F, F11). Use the menu bar or command palette for those
commands, or remove those shortcuts in Windows Terminal's settings.

**macOS.**

- *Alt keys:* in **Terminal.app**, turn on Settings > Profiles > Keyboard >
  *Use Option as Meta key*. In **iTerm2**, set Profiles > Keys > *Left
  Option key* to *Esc+*. Otherwise Option types special characters
  instead of Alt shortcuts.
- *Function keys:* hold **fn** with F1–F12, or turn on System Settings >
  Keyboard > *Use F1, F2, etc. keys as standard function keys*. macOS uses
  some function keys itself, so the menu bar (Alt+letter) or the Modern
  keyboard may be easier.
- *Colors:* Terminal.app has 256 colors instead of 24-bit. WordBench
  notices and picks the closest colors. iTerm2, kitty, WezTerm and Ghostty
  show exact colors.

**Linux.** Any modern terminal works. Konsole, GNOME Terminal, kitty,
WezTerm, Ghostty, foot and Alacritty all report keys fully. Some desktops
reserve keys such as Ctrl+Alt+F or F11 (full screen). Use the menu bar for
those commands, or change the desktop shortcut.

**tmux and screen.** WordBench works inside them, but pictures are shown
as placeholders. For all keys to get through tmux, add
`set -g extended-keys on` to `~/.tmux.conf`.

**If a key does nothing**, your terminal or desktop is probably keeping it.
Every command is also on the menu bar (Alt+letter) and in the command
palette (Alt+X), so you can always get to it another way.

---

## Troubleshooting

**"No spell checker found" / "No thesaurus found".** Install one as
described in [Spell check and thesaurus](#spell-check-and-thesaurus).

**Colors look wrong or washed out.** Your terminal may not support 24-bit
color. Try another theme, or `--plain` to use the terminal's own colors.
If your terminal does support 24-bit color but WordBench doesn't notice,
set `COLORTERM=truecolor`.

**Pictures show as `[Image: …]`.** The terminal can't draw images, or
WordBench didn't detect that it can. Try `WORDBENCH_GRAPHICS=kitty` or
`WORDBENCH_GRAPHICS=sixel`.

**Copy and paste with other programs doesn't work over SSH.** WordBench
then copies through the terminal (OSC 52), which some terminals turn off
by default. To paste into WordBench, use your terminal's own paste
(often Ctrl+Shift+V or Cmd+V). It arrives as plain text.

**Printing does nothing.** On Linux, check that CUPS is installed and a
default printer is set (`lpstat -d`). On Windows, check that a program is
set to open PDF files. You can always export a PDF and print it from
another program.

**Fonts I chose don't show on screen.** A terminal shows everything in one
font, so the font is kept for Word, LibreOffice and PDF. The status line
shows the font at the cursor.

**Reporting a problem.** Open an issue at <https://github.com/william-lampley/WordBench/issues>. Include your
system, terminal, `wordbench --version`, and, if you can, a file that
shows the problem.

---

*WordBench is an independent project and is not affiliated with, endorsed
by, or sponsored by Microsoft Corporation. Microsoft Word is a trademark
of the Microsoft group of companies, named here only to describe
compatibility.*
