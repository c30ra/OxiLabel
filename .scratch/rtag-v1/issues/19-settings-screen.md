# 19: Settings screen

**What to build:** A per-user settings screen with remappable keyboard shortcuts, pie menu sensitivity (flick distance), the label-switch style (number hotkeys plus label-ring key, or a label slice in the options ring), and light or dark theme. Settings are stored on the server and follow the user.

**Blocked by:** 07, 14

**Status:** done

- [x] I can rebind shortcuts, and conflicting bindings are flagged
- [x] Changing pie sensitivity changes how far I must flick, with immediate effect
- [x] The label-switch style setting changes how labels are switched in editing mode
- [x] Theme choice applies immediately
- [x] Settings persist across sessions and machines for the same account

**Decisions (2026-10-18):** every keyboard command is rebindable (key plus Ctrl/Alt/Shift), not only single keys; the number keys of labels stay as each label defines them. In both label-switch styles the number keys pick labels; the style decides the extra way: the label pie key (Shift+Q), or a "Label…" slice in the tool's options pie. Themes: dark, light, or as the system.

**Implementation note:** `rtag-core/src/keymap.rs` lists every command (`Action`, with id, group, name and default bindings) and matches key presses (Shift counts for letters and named keys, not for symbols such as `+`); the defaults have no clashes (Backspace now only removes the last point drawn; Delete and X delete). `rtag-core/src/settings.rs` holds `UserSettings` (own bindings by action id, flick distance clamped to 40–400 px, label style, theme), stored per user (`user_settings` table, `GET`/`PUT /api/me/settings`). The UI loads them with the session (`prefs.rs`) and every key handler (editor, viewer, filmstrip, grid, view filter) and the pies' key hints and tool rail read the keymap; the flick distance is how far a held pie key's pointer goes to pick without releasing. The theme sets `data-theme` on the page: the colours became tokens, with a light set (the image stage stays dark). The Settings screen is now for everyone: theme, flick distance, label style and a shortcuts table (Change captures the next key press, + adds one, Default and Reset all go back; keys used twice are flagged); the worker section stays admin-only.
