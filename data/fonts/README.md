# Bundled fonts

Atrium ships its own type system rather than depending on whatever the host has installed (per the project's "don't rely on system fonts" rule). Every file here is freely redistributable under SIL OFL 1.1.

| File | Family | Source | License |
|---|---|---|---|
| `InterVariable.ttf` | Inter Variable (UI sans) | [rsms/inter v4.1](https://github.com/rsms/inter/releases/tag/v4.1) | SIL OFL 1.1 — see `Inter-LICENSE.txt` |
| `InterVariable-Italic.ttf` | Inter Variable italic | [rsms/inter v4.1](https://github.com/rsms/inter/releases/tag/v4.1) | SIL OFL 1.1 |
| `SourceSerif4Variable-Roman.ttf` | Source Serif 4 Variable (note bodies) | [adobe-fonts/source-serif 4.005R](https://github.com/adobe-fonts/source-serif/releases/tag/4.005R) | SIL OFL 1.1 — see `SourceSerif4-LICENSE.md` |
| `SourceSerif4Variable-Italic.ttf` | Source Serif 4 Variable italic | [adobe-fonts/source-serif 4.005R](https://github.com/adobe-fonts/source-serif/releases/tag/4.005R) | SIL OFL 1.1 |
| `JetBrainsMono-Variable.ttf` | JetBrains Mono Variable (debug pane / monospace) | [JetBrains/JetBrainsMono v2.304](https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304) | SIL OFL 1.1 — see `JetBrainsMono-OFL.txt` |
| `JetBrainsMono-Variable-Italic.ttf` | JetBrains Mono Variable italic | [JetBrains/JetBrainsMono v2.304](https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304) | SIL OFL 1.1 |

## How they're loaded

`atrium/src/ui/typography.rs` registers each TTF at startup via
`pango::FontMap::add_font_file`, ensuring the fonts are available to
GTK regardless of what's installed system-wide.

If a file is missing at runtime, a warning is logged and Atrium falls
back to system fonts — useful during development if `data/fonts/`
hasn't been populated yet.

## Bumping a font

1. Download the new release from the upstream project.
2. Copy the variable TTF (and italic counterpart) into `data/fonts/`,
   keeping the same filename.
3. Refresh the LICENSE file alongside.
4. Update the table above with the new version + release tag link.
5. The change is patch-bump-worthy if the typography pass is otherwise
   unchanged; minor if a new family is added.
