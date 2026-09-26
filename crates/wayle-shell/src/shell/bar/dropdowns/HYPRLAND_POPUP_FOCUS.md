# Hyprland: bar button ignores clicks after its dropdown closes

## Symptom

1. Click a bar button: the dropdown opens.
2. Click the same button again: the dropdown closes.
3. Click it a third time **without moving the mouse**: nothing happens.

Moving the pointer even slightly makes the next click work. The button's
`:hover` style is also lost after step 2 until the pointer moves.

Only happens with `bar.dropdown-autohide = true` (the default). With autohide
off, every click works.

Observed on Hyprland 0.56.2, GTK 4.22.5, gtk4-layer-shell 1.3.0.

## Cause

This is a compositor bug. Wayle and GTK behave correctly.

An autohide `gtk::Popover` is an `xdg_popup` with an explicit grab. While the
grab is active, Hyprland moves pointer focus onto the popup surface, even though
the cursor is still over the bar. When the popup is dismissed (`popup_done` →
`destroy`), Hyprland does **not** move pointer focus back to the bar surface.
Focus only returns on the next pointer motion.

In the meantime, button events still reach the client, but Hyprland delivers
them to the dead popup surface, so GTK drops them.

Protocol trace (`WAYLAND_DEBUG=1`, bar = `wl_surface#43`, popup = `#52`):

```
wl_pointer.button(..., 272, 1)                 # click 1 on bar
-> xdg_popup#60.grab(wl_seat#3, ...)           # autohide popup opens
wl_pointer.leave(wl_surface#43)
wl_pointer.enter(wl_surface#52, -0.0078, -0.0078)   # focus moved to popup
wl_pointer.button(..., 272, 1)                 # click 2
xdg_popup#60.popup_done()
-> xdg_popup#60.destroy()                      # popup gone; no leave/enter follows
wl_pointer.button(..., 272, 1)                 # click 3: sent to the dead popup surface
wl_pointer.button(..., 272, 0)
...
wl_pointer.leave(wl_surface#52)                # only after the user moves the mouse
wl_pointer.enter(wl_surface#43, ...)
```

## Workaround

`refocus_pointer_on_close` in `registry.rs` hooks every dropdown's `closed`
signal. When Hyprland is running, it:

1. waits 50 ms so Hyprland has processed the popup destroy (otherwise the
   refocus lands on the popup again),
2. reads the cursor position (`cursorpos`),
3. warps the cursor onto that same position.

The warp makes Hyprland re-evaluate pointer focus. It sends `leave(popup)` and
`enter(bar)` immediately, and the cursor doesn't visibly move.

Hyprland 0.56 moved dispatchers to Lua, so the warp is sent as
`hl.dsp.cursor.move({ x = X, y = Y })`. The legacy `movecursor X Y` is only
used as a fallback if the Lua form isn't answered with `ok`. On 0.56 the legacy
form fails with `')' expected near 'X'`, and `dispatch` returns that text as
`Ok`, not as an error, so check the reply rather than the `Result`.

Verified: after the warp the trace shows `leave(#52)` → `enter(#43)`, and the
next click reaches `toggle_for` and maps the popover.

## Rejected alternatives

- **Turn autohide off and close on keyboard-focus loss.** Clicks work without the
  grab, but with Hyprland's focus-follows-mouse, keyboard focus leaves the popup
  as soon as the pointer moves over another window. Dropdowns would close
  without a click.
- **Re-apply `:hover` with a CSS class.** Fixes only the look (the
  `hover-sync` class in `registry.rs` and the `bar_button` SCSS). Clicks are
  still lost. It's redundant once pointer focus is restored.

## Removing the workaround

Delete `refocus_pointer_on_close` and its call in `DropdownRegistry::get_or_create`
once Hyprland restores pointer focus to the parent surface when an `xdg_popup`
grab ends. To check a new Hyprland version, remove the workaround, then run the
three-click sequence above. It should work, and a `WAYLAND_DEBUG=1` trace should
show `enter(bar surface)` right after `popup_done`.

## Reproducing / debugging

```bash
WAYLAND_DEBUG=1 RUST_LOG=warn,wayle_shell=debug wayle shell > /tmp/wl.log 2>&1
grep -nE 'wl_pointer#[0-9]+\.(enter|leave|button)|xdg_popup#[0-9]+\.(grab|popup_done|destroy)|toggle_for|popover (mapped|closed)' /tmp/wl.log
```
