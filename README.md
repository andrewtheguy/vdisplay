# vdisplay

A macOS menu-bar tool that adds virtual screens at a chosen size (in points)
and density (1x or 2x), using the private `CGVirtualDisplay` API. It is for
multi-display QA on a Mac that has no second monitor.

Each display belongs to the process: **Remove**, **Quit**, or a crash takes it
away. Nothing is saved between runs.

## Build and run

Run it inside the Mac's desktop (GUI) session. On macvm, that is a window of
the `macsandbox` tmux session:

```sh
scripts/bundle.sh                  # builds dist/vdisplay.app (Apple Silicon)
open dist/vdisplay.app             # menu bar only
open dist/vdisplay.app --args 1920x1080@2 1280x800@1   # also create these at launch
```

It has no Dock icon; it lives only in the menu bar.

The menu-bar icon (two screens) lists the displays with their current size,
and has **Add Display…**: a dialog for width, height and density.

## How it behaves

- The mode is listed in points. With 2x, macOS backs it with twice the pixels
  on each axis. `maxPixels` is set to exactly `points × scale`.
- A display's identity is vendor `0x7664`, the product is the slot number,
  and the serial comes from the spec. macOS remembers a display's arrangement,
  including its density, against that identity. A new size or density therefore
  starts fresh, while a repeated spec returns to where it was left.

## Releases

Push a `vX.Y.Z` tag that matches the version in `Cargo.toml`. The `release`
workflow builds `vdisplay.app` for Apple Silicon, zipped, and attaches it to the
GitHub release. The app is only ad-hoc signed, so clear the quarantine flag on
a downloaded copy before opening it:
`xattr -dr com.apple.quarantine vdisplay.app`.
