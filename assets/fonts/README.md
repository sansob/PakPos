# Bundled fonts

All fonts here are under the SIL Open Font License 1.1 (OSI-approved); each
licence text is next to them. They are compiled into the app
(`crates/pakpos-app/src/fonts.rs`) and the licences ship with every release.

| File | Font | Source |
|---|---|---|
| `Inter-Regular.otf` | Inter 4 (UI text) | https://github.com/rsms/inter |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono (code) | https://github.com/JetBrains/JetBrainsMono |
| `NotoEmoji-Regular.ttf` | Noto Emoji (monochrome icons) | https://github.com/google/fonts/tree/main/ofl/notoemoji |
| `NotoSansSymbols2-Regular.ttf` | Noto Sans Symbols 2 | https://github.com/google/fonts/tree/main/ofl/notosanssymbols2 |
| `NotoSansMath-Arrows.ttf` | Noto Sans Math, arrow blocks only | https://github.com/google/fonts/tree/main/ofl/notosansmath |

`NotoSansMath-Arrows.ttf` was made with
`pyftsubset NotoSansMath-Regular.ttf --unicodes="U+2190-21FF,U+27F0-27FF,U+2900-297F,U+2B00-2BFF"`.
