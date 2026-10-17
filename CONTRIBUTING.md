# Contributing to WordBench

Thanks for helping! Bug reports, ideas and pull requests are all welcome.

## Reporting a bug

Please include:

- your operating system and terminal (e.g. "Windows 11, Windows Terminal"
  or "macOS 15, iTerm2"),
- the output of `wordbench --version`,
- which keyboard you use (Classic or Modern),
- what you did, what you expected, and what happened,
- if you can, a small file that shows the problem. Remove anything
  private first.

## Building

You need Rust 1.88 or newer (<https://rustup.rs>).

```sh
cargo build            # debug build
cargo run -- FILE      # run it
cargo test             # all tests
```

No C libraries are needed. Spell check, the thesaurus, printing and the
font list use outside programs when they are installed (see the README).
The tests that need them skip themselves when the programs are missing.

## Before you send a pull request

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs these on Linux, macOS and Windows.

If you add or change a command or key, regenerate the key reference. A
test checks that it is current:

```sh
cargo run -- --keys > docs/KEYS.md
```

Please update [docs/USER_GUIDE.md](docs/USER_GUIDE.md) for anything a user
would notice.

## Code layout

| File | Purpose |
|---|---|
| `src/lib.rs` | Command line and terminal setup |
| `src/model.rs` | Document, paragraphs, tables, page setup |
| `src/layout.rs` | Wrapping, justification, lists, tables, pagination, footnotes |
| `src/editor.rs` | Cursor, selection, editing, undo, search |
| `src/app.rs` | Modes, dialogs, command dispatch, files |
| `src/ui.rs` | Drawing the screen and the color themes |
| `src/commands.rs` | Command list, menus and both keymaps |
| `src/reveal.rs` | Reveal Codes |
| `src/styles.rs`, `src/props.rs` | Styles, numbering, fonts and other properties |
| `src/review.rs` | Hyperlinks, comments, tracked changes, pictures in text |
| `src/pictures.rs`, `src/graphics.rs` | Picture decoding; kitty and sixel output |
| `src/print.rs`, `src/io/pdf.rs` | Page composition and PDF |
| `src/io/docx.rs` | Word files (lossless round trip) |
| `src/io/native.rs`, `src/io/html.rs` | `.wbd` files; HTML for the clipboard |
| `src/clip.rs` | System clipboard |
| `src/tools.rs` | Spell checker and thesaurus |
| `src/platform.rs` | Everything that differs between Linux, macOS and Windows |

Code that behaves differently per operating system belongs in
`src/platform.rs`, so the rest of the program stays the same everywhere.
Check that it still compiles for the other systems:

```sh
rustup target add x86_64-pc-windows-gnu aarch64-apple-darwin
cargo check --target x86_64-pc-windows-gnu
cargo check --target aarch64-apple-darwin
```

## License

By contributing, you agree that your contributions are licensed under
the same terms as WordBench: MIT or Apache-2.0, at the user's option.
