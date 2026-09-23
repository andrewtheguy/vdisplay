# vdisplay

A macOS menu-bar tool that keeps one virtual screen at a chosen size (in points)
and density (1x or 2x), using the private `CGVirtualDisplay` API. It is for
multi-display QA on a Mac that has no second monitor.

The display is on from launch and belongs to the process: turning it off,
**Quit**, or a crash takes it away. Its size and density are saved in the user
defaults (`defaults read com.andrewtheguy.vdisplay`: `width`, `height`, `hidpi`); the first
run uses 1440x900 at 2x.

## Build and run

Run it inside the Mac's desktop (GUI) session. On macvm, that is a window of
the `macsandbox` tmux session:

```sh
scripts/bundle.sh                  # builds dist/vdisplay.app and a .dmg (Apple Silicon)
open dist/vdisplay.app             # display at the saved size
open dist/vdisplay.app --args 1280x800@1   # this size for this run only
```

It has no Dock icon; it lives only in the menu bar.

The menu-bar icon (two screens) shows the display's size and density, and what
the WindowServer reports now. **Enabled** turns the display off and on.
**Settings…** opens a dialog for width, height and density (2x is HiDPI);
applying it saves the settings and, if the display is on, remakes it at the new
size.

## How it behaves

- The mode is listed in points. With 2x, macOS backs it with twice the pixels
  on each axis. `maxPixels` is set to exactly `points × scale`.
- The display's identity is fixed: name `vdisplay`, vendor `0x7664`, product
  `0x0001`, serial `0x0001`, whatever its size. macOS remembers a display's
  arrangement against that identity, so it always comes back as the same
  screen, in the position where it was left.
- The number macOS gives the display (its `CGDirectDisplayID`, e.g. 8) is not
  a screen count; it rises each time a virtual display is made in the login
  session. vdisplay logs it but does not show it in the menu.

## Releases

Push a `vX.Y.Z` tag that matches the version in `Cargo.toml`. The `release`
workflow builds `vdisplay.app` for Apple Silicon, packs it into a `.dmg`, and
attaches that to the GitHub release. Open the image and drag the app onto
Applications. The app is only ad-hoc signed, so clear the quarantine flag on
the copied app before opening it:
`xattr -dr com.apple.quarantine /Applications/vdisplay.app`.
