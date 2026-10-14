# Translations

The app's UI is written in English and translated at runtime. Bahasa
Indonesia is the first translation; pick the language in **Settings →
Language**. Without a saved choice PakPos follows the system locale
(`LC_ALL`, `LC_MESSAGES`, `LANG` or `LANGUAGE` starting with `id`).

![PakPos in Bahasa Indonesia](https://raw.githubusercontent.com/sansob/PakPos/main/docs/screenshots/bahasa.png)

## How it works

- UI text goes through `crate::i18n::tr("English text")`, or
  `crate::i18n::trf("Saved {path}", &[("path", &p)])` when it has values in it.
- `crates/pakpos-app/src/i18n_id.rs` maps each English text to its Indonesian
  translation. A text without an entry is shown in English, so a missing entry
  never breaks the UI.
- The test `every_ui_text_is_translated` scans the app's source and fails when
  a text passed to `tr` / `trf` has no translation (texts that stay the same,
  like `Body` or `gRPC`, are listed in `SAME`). Another test checks that every
  `{placeholder}` survives translation.

What is not translated: request and collection content (names, test
descriptions), messages from servers and the core library's error messages,
the CLI and the MCP server.

## Adding UI text

Wrap it in `tr(...)` and add the entry to `i18n_id.rs`; `cargo test -p pakpos`
tells you what is missing.

## Style for Bahasa Indonesia

Casual but clear, the way Indonesian developers talk: "kamu", not "Anda".
Keep the technical terms developers use as they are (request, response,
header, body, token, environment, commit, push, stream); translate the
everyday words around them ("Simpan", "Kirim", "Hapus", "Pengaturan").

## Adding a language

Add a variant to `Lang` in `i18n.rs` (code and its own name), a table module
like `i18n_id.rs`, and a branch in `tr`. Run the tests against the new table
too.
