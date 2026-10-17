# WordBench

**A terminal word processor in the spirit of the classic DOS word processors, for Linux,
macOS and Windows.**

WordBench brings back the blue screen, function-key commands and Reveal
Codes, with what a modern word processor needs: real Word (`.docx`) files
that round-trip without losing anything, tracked changes and comments,
tables, footnotes, captions, headers and footers, spell check, PDF export, and
copy and paste with your other programs.

```
────────────────────────────────────────────────────────────────────────────────
                               Quarterly Report

       Sales grew in every region this quarter. North led with a twenty
       percent rise, and the new office opened on time.

       Next steps:

░░░░░░░[────▲────▲────▲────▲────▲────▲────▲────▲────▲────▲────▲────▲────]░░░░░░░
 Sales grew in every region this quarter. North led with a [UND]twenty [SRt]
 percent[und] rise, and the new office opened on time.[HRt]
 [HRt]
 Next steps:[HRt]

(untitled)   Courier New 12pt                           Doc 1 Pg 1 Ln 2" Pos 1"
```

*The Classic keyboard with Reveal Codes open (Alt+F3) under the text.*

## Highlights

- **Two keyboards, switchable at any time (Alt+K).** *Modern* keys
  (Ctrl+S, Ctrl+B, Ctrl+Z…) with the menu bar always on screen, which is
  the default, or *Classic* function keys (F10 save, F7 exit, F6 bold,
  Alt+F3 Reveal Codes). Everything is also on a menu bar and in a
  searchable command palette (Alt+X), so the whole program works from the
  keyboard. The mouse works too.
- **Reveal Codes.** A split view of the formatting codes (`[Bold]`,
  `[HRt]`, `[Just:Center]`, `[Tbl Def]`, `[Footnote]`…) under your text.
- **Word files without loss.** `.docx` files open with their styles, lists,
  fonts, colors, tables, pictures, footnotes, comments and tracked changes.
  Anything WordBench doesn't edit (charts, equations, text boxes, macros) is
  kept exactly as it was and saved back unchanged.
- **Pages as they print.** Paper size, margins, headers and footers with
  several lines, page numbers, footnotes and endnotes. A faint paper outline
  shows the sheet, and pages appear side by side when the window is wide.
  The screen, print preview and PDF all use the same layout.
- **Writing tools.** Spell check, thesaurus, word count, find and replace,
  captions for tables and figures, hyperlinks, date stamp.
- **Your clipboard.** Copy into a browser, mail or LibreOffice and keep bold,
  headings, lists, tables and links. Paste from a web page and the
  formatting comes with it.
- **Eight color themes.** Classic blue, Dark, Light (paper), Solarized
  dark, Amber screen, Green screen, High contrast, and your terminal's own
  colors. The theme picker previews each one live.

## Install

### Download a release

Download a release from the [Releases page](https://github.com/william-lampley/WordBench/releases/latest), unpack it, and put
`wordbench` (and its short name `wb`) somewhere on your `PATH`.

| System | File |
|---|---|
| Windows 10/11 (64-bit) | `wordbench-<version>-x86_64-pc-windows-msvc.zip` |
| macOS (Apple silicon) | `wordbench-<version>-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `wordbench-<version>-x86_64-apple-darwin.tar.gz` |
| Linux (x86-64) | `wordbench-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux (ARM64) | `wordbench-<version>-aarch64-unknown-linux-gnu.tar.gz` |

On macOS, the first time you run a downloaded binary you may need to allow
it: `xattr -d com.apple.quarantine wordbench wb`.

### Build from source

You need [Rust](https://rustup.rs) 1.88 or newer. Then:

```sh
cargo install --git https://github.com/william-lampley/WordBench
```

or, from a checkout of the repository, `cargo install --path .`.

This installs `wordbench` and `wb` into `~/.cargo/bin` (on Windows,
`%USERPROFILE%\.cargo\bin`). No C libraries are needed on any system.

### Optional helpers

WordBench uses these programs if they are installed:

| Feature | Linux | macOS | Windows |
|---|---|---|---|
| Spell check | `aspell-en` or `hunspell-en-us` | `brew install aspell` | Hunspell (`scoop install hunspell`) or a word list |
| Thesaurus | `mythes-en-us` or `wordnet` | `brew install wordnet` | MyThes `.dat`/`.idx` files |
| Printing | CUPS (`lp`) | built in (`lp`) | the program registered to print PDFs |
| Font list | fontconfig | built in | built in |

The [user guide](docs/USER_GUIDE.md#spell-check-and-thesaurus) explains
each one.

## Quick start

```sh
wordbench                     # a new document
wordbench report.docx         # open a Word file
wb notes.wbd                  # `wb` is the same program
wordbench --classic           # start with the classic function keys
wordbench --convert report.docx report.pdf   # convert without opening
```

Press **F3** (Classic keys) or **F1** (Modern keys) for help, **Alt+X**
for the command palette, and **F9** or **F10** for the menu bar.

| | Classic | Modern |
|---|---|---|
| Menu bar | F9 or Alt+= | F10 |
| Save / Exit | F10 / F7 | Ctrl+S / Ctrl+Q |
| Open / List files | Shift+F10 / F5 | Ctrl+O |
| Bold / Underline / Italic | F6 / F8 / Ctrl+I | Ctrl+B / Ctrl+U / Ctrl+I |
| Center / Flush right | Shift+F6 / Alt+F6 | Ctrl+E / Ctrl+R |
| Block (select) | Alt+F4 | Shift+arrows |
| Search / Replace | F2 / Alt+F2 | Ctrl+F / Ctrl+H |
| Reveal Codes | Alt+F3 | F11 |
| Format (line, page, document) | Shift+F8 | Ctrl+D |
| Font | Ctrl+F8 | Ctrl+Shift+F |
| Spell / Thesaurus | Ctrl+F2 / Alt+F1 | F7 / Shift+F7 |
| Print | Shift+F7 | Ctrl+P |

The full list is in [docs/KEYS.md](docs/KEYS.md), or run `wordbench --keys`.

## Documentation

- [User guide](docs/USER_GUIDE.md): everything from first steps to
  tracked changes, with notes for each operating system.
- [Key reference](docs/KEYS.md): every command and its keys in both
  keyboards.
- [Contributing](CONTRIBUTING.md)
- [Support WordBench](#support-wordbench)

## File formats

| Format | Open | Save | Notes |
|---|---|---|---|
| Word `.docx` | ✓ | ✓ | Lossless round trip |
| WordBench `.wbd` | ✓ | ✓ | Native format (JSON) |
| Plain text `.txt`, Markdown `.md` | ✓ | ✓ | Text only |
| PDF `.pdf` | | ✓ | Export and print |

## Terminals

WordBench runs in any terminal with at least 80×24 characters. It works
best in one that reports all keys and supports 24-bit color, such as
Windows Terminal, kitty, WezTerm, Ghostty, Konsole, GNOME Terminal, iTerm2
or foot. In terminals that can show pictures (kitty, Ghostty, WezTerm,
iTerm2, foot), pictures are drawn in the document. Otherwise a placeholder
such as `[Image: logo.png]` takes their place.

The user guide's [terminal notes](docs/USER_GUIDE.md#terminal-notes) cover
Alt and function keys on macOS and in older Windows consoles.

## Limitations

A terminal draws one fixed-width font. So WordBench lays pages out on a
typewriter grid (10 characters per inch, 6 lines per inch, like 12-point
Courier). Fonts, sizes and colors are kept and saved, but the screen and
PDF show them in that grid. Charts appear as placeholders, and multi-column
sections show as one column.

## Building and testing

```sh
cargo build --release      # target/release/wordbench and wb
cargo test                 # unit, layout, file-format and screen tests
cargo clippy --all-targets
```

`WORDBENCH_SHOW=1 cargo test -- --nocapture` prints the rendered test
screens. GitHub Actions runs the tests on Linux, macOS and Windows. Pushing
a `v*` tag builds the release downloads.

## Support WordBench

WordBench is free and made in my spare time. If it's useful to you, you can
[buy me a coffee](https://buymeacoffee.com/williamlampley) ☕. It helps keep
new features and fixes coming. Starring the repo and reporting bugs help
too.

[![Buy Me a Coffee](https://img.shields.io/badge/Buy%20me%20a%20coffee-support-FFDD00?logo=buymeacoffee&logoColor=black)](https://buymeacoffee.com/williamlampley)

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Trademarks

WordBench is an independent project. It is not affiliated with, endorsed
by, or sponsored by Microsoft Corporation. Microsoft Word is a trademark
of the Microsoft group of companies, named here only to describe the file
format WordBench reads and writes.
