# vdisplay

A macOS menu-bar tool that adds one virtual screen at a chosen size (in points)
and density (1x or 2x), using the private `CGVirtualDisplay` API. It is for
multi-display QA on a Mac that has no second monitor.

The display belongs to the process: **Remove**, **Quit**, or a crash takes it
away. Nothing is saved between runs.

## Build and run

Run it inside the Mac's desktop (GUI) session. On macvm, that is a window of
the `macsandbox` tmux session:

```sh
scripts/bundle.sh                  # builds dist/vdisplay.app (Apple Silicon)
open dist/vdisplay.app             # menu bar only
open dist/vdisplay.app --args 1440x900@2   # also create the display at launch
```

It has no Dock icon; it lives only in the menu bar.

The menu-bar icon (two screens) shows the display with its current size and
offers to remove it. With no display, it has **Add Display…**: a dialog for
width, height and density (1440x900 at 2x to start). Only one display exists at
a time; to change its size, remove it and add it again.

## How it behaves

- The mode is listed in points. With 2x, macOS backs it with twice the pixels
  on each axis. `maxPixels` is set to exactly `points × scale`.
- The display's identity is fixed: name `vdisplay`, vendor `0x7664`, product
  `0x0001`, serial `0x0001`, whatever its size. macOS remembers a display's
  arrangement against that identity, so it always comes back as the same
  screen, in the position where it was left.

## Releases

Push a `vX.Y.Z` tag that matches the version in `Cargo.toml`. The `release`
workflow builds `vdisplay.app` for Apple Silicon, zipped, and attaches it to the
GitHub release. The app is only ad-hoc signed, so clear the quarantine flag on
a downloaded copy before opening it:
`xattr -dr com.apple.quarantine vdisplay.app`.
