# Atrium on Hyprland: window rules, app_id, and keybind capture

Atrium is plain GTK4 and runs on any Wayland desktop; under a tiling
compositor it needs nothing special. Three Hyprland-specific notes
are collected here because none of them fit the other reference
docs: a scratchpad rule for Quick Entry, the one app_id quirk to
know before writing class-based rules, and the keybind-driven
capture path that works today.

## Quick Entry as a scratchpad target

Quick Entry is deliberately built as a small, non-modal window with
a static title of `"Quick Entry"`: structurally exactly what a
float/scratchpad rule wants. To have it open as a floating capture
window pinned to every workspace, match on the title.

Hyprland's Lua config (current mainline syntax):

```lua
-- Match the TITLE, not the class: every Atrium window shares one
-- app_id (see the next section), so a class-based rule catches the
-- main window too. Match initial_title because static rules are
-- evaluated once, when the window opens.
hl.window_rule({
    match = { initial_title = "^Quick Entry$" },
    float = true,
    pin = true, -- capture from any workspace
})
```

Older ini-style configs spell the same rules with `windowrulev2`:

```ini
windowrulev2 = float, title:^(Quick Entry)$
windowrulev2 = pin, title:^(Quick Entry)$
```

Check `hyprctl clients` while Quick Entry is open to confirm what
your build reports for `initialTitle` before trusting either form.

## The app_id collision

All Atrium windows (the main window, Quick Entry, Memory Watch)
report the same Wayland `app_id`, `io.github.virinvictus.atrium`:
a GTK4 application has one application id, and GTK4 offers no
per-toplevel override. That is why the rules above match on title.

Class-based matching for the main window is still correct, and the
`.desktop` entry wires it up: `StartupWMClass` equals
`io.github.virinvictus.atrium`, so a default rule like

```
class:^(io.github.virinvictus.atrium)$
```

matches the main window (and any Atrium dialog). A rule keyed on
`initial_title` is what isolates Quick Entry specifically.
`StartupWMClass`, `Icon=`, and the application id are kept in
lockstep deliberately (Phase 21); if you ever rename one, rename all
three.

## Keybind-driven capture today (`atrium-cli add`)

`atrium-cli add` creates a task headlessly against Atrium's
database, with no GUI involvement. It is not the deferred zero-launch
capture daemon (`atriumd`, deferred post-1.0): the database must be
reachable from the calling session, and there is no inline-completion
feedback. But it works today as a keybind target, and the inline
vocabulary (`#tag`, `@date`, `!priority`) is parsed by the same
`atrium-inline` parser the GUI uses, so capture strings behave
identically in both.

```lua
-- SUPER+E: capture straight into the project, inline syntax included.
hl.dsp.exec_cmd("atrium-cli add 'Buy milk #errand @tomorrow' --project Errands")
```

Structured flags take over where inline syntax ends: `--tag`,
`--scheduled` (or `--when`), `--due` (or `--deadline`), `--project`,
`--parent`, and `--note`. Run `atrium-cli add --help` for the full
list and date formats.
