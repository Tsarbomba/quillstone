# Quillstone

A lightweight, fast visual novel engine in Rust: one that
keeps what a visual novel needs and leaves out the bloat. Scripts are plain
markdown, and seamless video playback is a first-class feature.

**What it does:** characters (a name can come from a variable, such as one the player typed) and sprites (placed anywhere on screen, layered with `z`) with fades, layered sprites (swap an expression or outfit layer by layer), frame animations (blinking, talking) and looping moves, ambient snow, rain or falling petals, a colour wash over the stage (`::tint`, for night or a memory), music, sound effects,
voice (including auto-voice), video cutscenes, branching choices with
conditions, clickable hotspots (maps, collectibles, point-and-click scenes), variables with whole-number arithmetic, dice rolls and grouped names like `hero.hp`, list variables, text input (ask for a name), stories split over several files (`::include`), translations (players switch language in Settings; translators work in plain `lang/<code>.txt` files and never touch the script), items with an inventory screen and shops (both styled by your theme), `call`/`return`, bold/italic/colour/size text, full
rollback, history, skip and auto-forward, save/load slots, a gallery of pictures and looping clips, music
room and scene replays, gamepad support and hot reload. The `quillstone` tool checks a script,
plays every branch to find story bugs, and packages a game into a folder players
double-click. The full language is in [SCRIPT.md](SCRIPT.md).

## Set up Rust (one time)

**Windows**
1. Download and run `rustup-init.exe` from <https://rustup.rs>.
2. If it asks for the Visual Studio C++ Build Tools, accept. In the installer,
   tick **Desktop development with C++** (this is the linker Rust needs).
3. Open a *new* terminal and check: `cargo --version`

**Linux / macOS**
```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
(Linux also needs a C compiler: `sudo apt install build-essential`. macOS: `xcode-select --install`.)

## Try it

```
cargo test --workspace                          # run all tests
cargo run -- run   examples/demo.quill.md       # play the demo in the terminal
cargo run -- dump  examples/demo.quill.md       # show the compiled ops
cargo run -- check examples/demo.quill.md       # validate script + assets
cargo run -- test  examples/demo.quill.md       # play every branch, report story bugs
```

`test` plays every choice branch and reports, with the choices that lead there:
endless loops that never show anything, places the story can never end from,
labels and lines no branch reaches, choice options whose condition is never
true, and variables that are read but never set (usually typos). It exits with
an error only for the first two; the rest are warnings.

Exit codes: `0` when it finished and found no error, `1` for a real problem, and
`2` when it stopped at the `--limit` (variables that keep growing can make the
story endless), because then only part of the story was played. Raise `--limit`,
or pass `--allow-partial` if a partial run is fine for you (the exit code is then `0`).

## Set up FFmpeg for video (one time)

The player plays video with FFmpeg. Building it needs an FFmpeg 8.x *shared*
build (use an **LGPL** one so games can ship it) and libclang.

**Windows**
1. Download `ffmpeg-n8.1-latest-win64-lgpl-shared-8.1.zip` from
   <https://github.com/BtbN/FFmpeg-Builds/releases> and unzip it to `C:\ffmpeg`
   (so `C:\ffmpeg\include` and `C:\ffmpeg\lib` exist).
2. Install LLVM: `winget install LLVM.LLVM`.
3. Set two variables, then open a *new* terminal:
   `setx FFMPEG_DIR C:\ffmpeg` and `setx LIBCLANG_PATH "C:\Program Files\LLVM\bin"`.
4. At run time the FFmpeg DLLs must be found: add `C:\ffmpeg\bin` to `PATH`, or
   copy its `.dll` files next to `quillstone-player.exe` (that is also how a game ships).

**Linux / WSL**: the same, with `ffmpeg-n8.1-latest-linux64-lgpl-shared-8.1.tar.xz`;
install libclang (`sudo apt install libclang-dev`), `export FFMPEG_DIR=...`, and
`export LD_LIBRARY_PATH=$FFMPEG_DIR/lib` to run.

**macOS**: `brew install ffmpeg pkg-config llvm`, then
`export LIBCLANG_PATH=$(brew --prefix llvm)/lib` (FFmpeg is found through
pkg-config, no `FFMPEG_DIR` needed). Homebrew's FFmpeg is a GPL build: fine for
developing, but ship an LGPL build with a game.

No FFmpeg? Build without video: `cargo run --release --no-default-features --features text,audio -- ...`
(videos then show a placeholder).

## Run the player

The player is its own small project so the engine crates stay dependency-free:

```
cd player
cargo run --release -- ../examples/demo.quill.md
```

The first build downloads and compiles wgpu, winit, cosmic-text and kira (a few minutes).

**Hot reload:** leave the game running while you write. Saving the script (or
a file it includes, or its theme) updates the game within half a second, at the same spot, the way a
save would load after an edit; the music keeps playing. If the edited script has
errors, a notice shows the first one (all of them go to the console) and the old
version keeps running. New pictures are picked up too; a new theme font needs a
restart.

Run without a script argument, the player plays `game.qsa` if it sits next to its
executable, else `main.quill.md` (or the only `.quill.md` file) from a `game`
folder next to it, or else from the executable's own folder. That is how a shipped game starts: players just
double-click the exe. If it can't start, the reason is also written to
`error.txt` next to the exe.

| Control | What it does |
| --- | --- |
| click / Space / Enter / Right | advance (the first press finishes typing the line) |
| 1-9 or click | pick a choice (a timed choice picks its default option when the bar runs out) |
| Backspace / Left / wheel up | go back (rollback); wheel down rolls forward again |
| H / Page Up | history (backlog); wheel or Page Up/Down to scroll, Esc to close |
| A | auto-forward on/off (waits for the voice clip; stops at choices) |
| Tab / hold Ctrl | skip lines already read (stops at unread lines, choices and the end; "Skip unread text" in settings skips everything up to a choice) |
| S / L | save / load screens (9 pages of 6 slots; the load screen also has the Auto and Quick pages) |
| F5 / F9 | quick save / quick load |
| U | hide the textbox and quick menu to see the picture; the next key press or click brings them back (and does nothing else) |
| Esc / right-click | settings: text speed, auto-forward delay, skip unread, music/sound/voice volume, fullscreen, quick menu (game default, always, on hover, hidden), Main menu, Quit |
| I | the inventory, in games that have one (over the story Esc, right-click or I closes it). In a shop and in an inventory the script opened, Esc and right-click open the settings (closing them returns), S / L / H / F5 / F9 work as in the story, and only the Leave / Close button (or Backspace, the gamepad's X; or I in the inventory) leaves |
| typing | at a text input the keys type into the field (Enter or OK confirms, Backspace deletes, Esc opens the settings) and shortcut letters are off there |
| F11 | fullscreen |
| Up / Down, Enter | pick out a choice, a menu entry or a save slot, and take it (on a hotspot screen the arrow keys cycle the regions) |

**Gamepads** (Xbox, PlayStation, Switch Pro, Steam Deck) work too: A advances or
confirms, B or Start goes back (settings during the story), X rolls back, Y
opens the history, LB toggles auto-forward, RB toggles skip, holding RT skips,
LT and Back open the load and save screens, and the D-pad or left stick moves
around. Building on Linux needs libudev's development files for this
(`sudo apt install libudev-dev`), or build with `--no-default-features --features text,audio,video`.

The game opens at a main menu (Start, Continue, Load, Extras when the game has a
gallery, music room or scene replays, Settings, Quit) and
returns to it at the end of the story. The quick menu under the textbox (Back, History, Skip, Auto, Save, Load,
Q.Save, Q.Load, Settings, and Items in a game with an inventory) does the same with the mouse; a game may show it
only while the mouse is over the textbox, or not at all, and Settings lets you choose; a theme can draw the buttons as pictures at places of its own. The game saves automatically
whenever a choice comes up and when you quit. Settings and saves are kept per
user (Windows: `%APPDATA%\Quillstone\<game>\`; macOS: `~/Library/Application Support/Quillstone/<game>/`;
Linux: `~/.local/share/quillstone/<game>/`). `<game>` is the script's `id:` front matter key; without one it is
the title (so renaming the title moves the saves: set `id:` before you ship), or the script's file name.
The first time, the window opens at 1920x1080 (the game's own size), or at the largest 16:9 size that fits
the monitor with some margin, centred; after that the player remembers the window's size, position and
full-screen state (in `settings.txt`) and restores them, kept on screen if the monitors changed.
`quillstone-player <game> --unlock-all` shows every gallery, music room and scene entry as unlocked for that run
(to test the Extras screen); nothing is written to the saved unlocks.
Minimising the window pauses everything (music, sound, voice, videos, waits, typing, animations and timed
choices) and restoring it carries on exactly where it stopped. Videos play with FFmpeg (see above). `QUILLSTONE_TEST_VIDEO=1` plays a test
pattern in place of every video, to check the pipeline without FFmpeg.

**Troubleshooting**
- The player has optional features (`text`, `audio`, `video`, `gamepad`; all on
  by default), so you can narrow a problem down. For example,
  `cargo run --release --no-default-features --features text -- ../examples/demo.quill.md`
  runs without sound or video, and `--no-default-features` alone shows just the
  window and images.
- No sound device? The player prints a warning and runs silent.
- Build errors about FFmpeg: see [Set up FFmpeg](#set-up-ffmpeg-for-video-one-time).

## Ship a game

Build the player once without a console window, then package the script:

```
cd player
cargo build --release --features no-console
cd ..
cargo run --release -p quillstone -- package examples/demo.quill.md --player player/target/release/quillstone-player.exe --libs C:\ffmpeg\bin --out MyGame
```

`MyGame/` then holds `Quillstone Demo.exe` (named after the title; `--name` picks
another), FFmpeg's five DLLs (avcodec, avformat, avutil, swscale, swresample;
never avfilter or avdevice), `licenses/FFmpeg.txt` (the LGPL notice FFmpeg
requires) and `game.qsa`: the script and only the files it and its theme use, packed
into one file (stored as they are, not compressed or scrambled, so it is only a
tidy way to ship them). Zip the folder; players unzip it and double-click the exe.
The script is checked first, assets outside the script's folder are refused, and
an existing non-empty `--out` folder is never overwritten. Without `--libs`, the
DLLs are taken from beside the player.

Add `--folder` to get a `game/` folder of loose files instead of `game.qsa`.
`quillstone unpack MyGame/game.qsa --out look-inside` writes the files of an
archive into a new folder, to see what is in it.

**Patches and mods.** Put a file at the same path in a `game/` folder beside the
exe and it replaces the shipped one: a new `assets/room.png`, a fixed
`chapters/ch2.quill.md`, a `lang/fr.txt` translation. A file with a path the game
doesn't have yet is simply added (a voice recording in the voice folder, say).
`quillstone-player path/to/game.qsa` does the same with the `game/` folder beside
that archive.

**Slim FFmpeg for shipping.** The full FFmpeg download carries every codec FFmpeg
has (~130 MB of DLLs). `tools/build-ffmpeg.sh` builds one with only what Quillstone
plays (VP8 and VP9 video in WebM, Vorbis and Opus sound, and `.opus` sound files) and
only the five libraries the player loads. On Windows, run it in the MSYS2 **UCRT64**
shell (the packages to install are listed at the top of the script):

```
cd /c/Users/you/path/to/quillstone
tools/build-ffmpeg.sh ffmpeg-slim
```

Then package with `--libs ffmpeg-slim\bin` (Linux: `ffmpeg-slim/lib`). The player
doesn't need rebuilding: the slim DLLs replace the full ones as long as the FFmpeg
version matches (8.1). Keep developing against the full download.

A `no-console` game shows no messages; if it can't start or crashes, the reason
is written to `error.txt` next to the exe. On Linux the player loads FFmpeg's
`.so` files from its own folder, so the same layout works there.

## Script format

````markdown
::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

# rooftop
::bg{src=assets/rooftop.png fade=0.5}
::show{mika neutral at=right fade=0.4}

**Mika:** You came after all, {name}.

::choice
- [Tell the truth](#truth){trust+=1}
- [Say nothing](#silence){if met}
::
````

See [SCRIPT.md](SCRIPT.md) for every command, conditions, variables and flow control,
and `examples/demo.quill.md` for a script that uses them all (`examples/hotspots.quill.md`
shows hotspots: a map, a collectible and an exit; `examples/shop.quill.md` shows a list
variable as an inventory in a shop; `examples/market.quill.md` and `examples/market_theme.txt`
show items, the built-in inventory and shop screens, and a theme that restyles them;
`examples/iconmenu.quill.md` and `examples/icons_theme.txt` replace the quick menu's text buttons with picture buttons at positions the theme picks;
`examples/staging.quill.md` shows sprite height and layering, text sizes, `{nw}` and a `::tint` colour wash;
`examples/stats.quill.md` shows grouped stats, arithmetic, `min`/`max`/`abs`/`count` and dice rolls;
`examples/anim.quill.md` shows frame animations (a talking sprite, a one-shot, an animated background) and looping moves; `examples/places.quill.md` picks the background and a pose from variables; `examples/layers.quill.md` builds a sprite from body, eyes and mouth layers, with a blinking eyes layer; `examples/weather.quill.md` shows ambient snow, rain and falling petals; `examples/chapters.quill.md` and `examples/chapters/` show a story split over several files with `::include`; `examples/lang.quill.md` and `examples/lang/sv.txt` show a game in English and Swedish).

## Layout

| Crate | Job |
| --- | --- |
| `quillstone-script` | parser/compiler (values, expressions, ops), no dependencies |
| `quillstone-runtime` | pure state machine; scene, call stack and variables in `State`; rollback (`Session`) |
| `quillstone-video` | `Decoder` trait, decoder threads + frame queue (`Clip`), which clips play (`Videos`), a test-pattern decoder |
| `quillstone-render` | virtual 1920x1080 screen, letterboxing, `View` (sprites, fades, typewriter) + `Draw` list, `build_frame` (quads + text jobs), image prep (mipmaps, alpha bleeding) |
| `quillstone-audio` | `Mixer`: runtime commands -> audio actions (music crossfades, voice per line, rollback-safe) on music/sfx/voice channels |
| `quillstone` | the CLI (`check`, `test`, `voices`, `lang export`, `dump`, `package`, text-mode `run`) |
| `quillstone-ui` | `Game`: input routing per screen, settings (+ file), backlog, quick menu, auto-forward, skip mode, save/load slots, confirm dialogs |
| `player/` | the window: winit + wgpu draws the frame, cosmic-text rasterizes text, kira plays sound, FFmpeg decodes video |

The renderer is split on purpose: `View` turns runtime commands into a plain
list of `Draw` items (images, rectangles, text), and a backend only has to
execute that list. A video frame is one more textured quad.
