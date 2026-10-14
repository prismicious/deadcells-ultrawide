# Dead Cells ultrawide fix

Makes Dead Cells fill 21:9, 32:9 and other wide screens. You see more of the
level (Hor+), the HUD sits at the real screen edges, and nothing is stretched.
It changes **no game files**: a small launcher adjusts one value in the
running game.

Tested on Steam v35 "The End Is Near" (2026-06-16, Steam build 23762174) at 5120x1440.

| Stock, 5120x1440 | With the fix |
|---|---|
| ![stock menu](screenshots/before-menu.png) | ![fixed menu](screenshots/after-menu.png) |
| ![stock gameplay](screenshots/before-gameplay.png) | ![fixed gameplay](screenshots/after-gameplay.png) |

## Why the game has black bars

Dead Cells runs on HashLink, and its compiled code is stored inside
`deadcells.exe`. On every resize, `Main.onResize` does this:

```
pixelScale   = max(1, 3 * min(W/1920, H/1080))      rounded down to 0.25
stage width  = min(pixelScale * Viewport.NATIVE_WIDTH,  W)
stage height = min(pixelScale * Viewport.NATIVE_HEIGHT, H)
```

Everything outside the stage is painted black. `NATIVE_WIDTH` is hardcoded to
**683** and `NATIVE_HEIGHT` to 400, which is about 16:9. At 5120x1440 the
pixelScale is 4, so the stage is 4 × 683 = 2732 px wide, and 1194 px on each
side stays black.

## What the fix does

It raises `NATIVE_WIDTH` so that the stage covers the whole monitor. At
5120x1440 the new value is 5120 / 4 = **1280**. The HUD, menus and parallax
backgrounds already lay themselves out from the stage size and
`NATIVE_WIDTH`, so they follow automatically. Vertical scale and pixel size
stay the same as stock, so sprites are never stretched.

The exe is protected by SteamStub DRM. Editing it on disk makes Steam refuse to
start it ("Application load error 3:0000065432"), so the launcher does not
touch it. Instead it:

1. starts the game through Steam;
2. waits for the game to set up its `Viewport` statics, which sit in memory as
   the int pair `683, 400`;
3. writes the new width into that single location, reads it back to confirm,
   and resizes the window by one pixel and back so the game re-lays-out at
   once.

If it does not find exactly one such pair, it refuses and changes nothing, so
an unsupported game version just runs stock.

## Use

Requires Python 3 on Windows and nothing else.

```
python deadcells_ultrawide.py                # launch via Steam, fit the primary monitor
python deadcells_ultrawide.py --width 1280   # pick the logical width yourself
python deadcells_ultrawide.py --attach       # fix a game that is already running
```

When started with `pythonw` (no console window), messages go to
`deadcells_ultrawide.log` next to the script instead.

### Run it from Steam's Play button (optional)

In Steam, open Dead Cells → Properties → Launch Options and enter:

```
"C:\path\to\pythonw.exe" "C:\path\to\deadcells_ultrawide.py" %command%
```

Steam then starts the game through the launcher every time.

## Undo

Start the game normally from Steam, or clear the launch options. Nothing on
disk was changed, so there is nothing to restore.

## After a game update

The fix finds the value afresh on every launch, so it keeps working as long as
the game still sets `NATIVE_WIDTH = 683, NATIVE_HEIGHT = 400`. If an update
changes that, the launcher prints that it found no match, and the game runs
stock.

## Side effects

- Steam achievements, cloud saves and save files are not touched. The game
  has no anti-cheat.
- The title screen's backdrop is drawn to cover the wider stage, so it shows a
  closer crop of the artwork.
- Full-screen panels such as the pause/equipment screen span the whole
  width. They are laid out for the wider stage, and the art inside them is
  not stretched (see `screenshots/after-pause-menu.png`).
- Small rooms and some cinematics were laid out for a 16:9 view. On a very
  wide screen you can see past their intended edges in places.

## Credits

The bytecode was explored with [hlbc](https://github.com/Gui-Yom/hlbc).
Earlier community work: czarman's 3DMigoto shader fix on PCGamingWiki (21:9).
This repository contains no game files.

## License

MIT, see [LICENSE](LICENSE).
