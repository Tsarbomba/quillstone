<h1 align="center"><img src="qs_logo.jpg" alt="Quillstone" width="600"></h1>

A lightweight but powerful visual novel engine written in Rust, and an
alternative to the established VN engines. It is small, starts fast, and keeps
a game's story, look and assets organised in a few plain files.

**Status:** early and under active development. It is built and tested on Windows;
Linux and macOS are less tested. The script language may still change.

**What stands out**
- **Scripts are plain markdown.** A scene reads like a screenplay: headings are
  scenes, `**Name:** text` is a line, and a short `::command` stages it. No
  programming language to learn, and any text editor works.
- **A built-in story tester.** `quillstone test` plays every branch of a script
  and reports dead ends, unreachable scenes and choices that are never offered,
  before a player finds them. `quillstone graph` draws the story as a flowchart.
- **Structured projects.** Story, theme and translations are separate plain
  text files. Translators work in their own files and never touch the script.
- **Video as a first-class feature.** Cutscenes, video backgrounds and looping
  clips play smoothly alongside sprites and text.
- **Small, fast releases.** `quillstone package` ships only the files a game
  uses, in a single archive next to the player, with a `mods/` folder for
  patches and mods.

**What it covers:** characters and sprites with transitions and animations,
music, sound and voice, branching choices, variables, inventories, shops and quests,
clickable scenes, simple real-time mini-games, a theme for the menus and dialogue box,
rollback, history, skip and auto-forward, save slots, a gallery, music room and scene replays,
translations, gamepad support and hot reload. The full language is in
[SCRIPT.md](SCRIPT.md).

<p align="center"><a href="https://ko-fi.com/quillstone"><img src="https://img.shields.io/badge/Buy%20me%20a%20sausage-72a4f2?style=for-the-badge&logo=ko-fi&logoColor=white" alt="Buy me a sausage on Ko-fi"></a></p>

## Get started

You don't need to install or compile anything. Download the kit for Windows or Linux from the
[Releases page](https://github.com/Tsarbomba/quillstone/releases) and unpack it (Windows: right-click, Extract
All; Linux: `tar xzf quillstone-*-linux.tar.gz`). You get a folder `quillstone-<version>-<os>` that holds the `quillstone` tool, the player, FFmpeg's libraries and the examples.

Windows may show "Windows protected your PC" the first time, because the programs aren't signed:
click **More info**, then **Run anyway**. The Linux kit needs glibc 2.35 or newer (Ubuntu 22.04 or newer).

Play the demo from the kit's folder:

```
quillstone-player examples/demo.quill.md
```

(In PowerShell, the usual Windows terminal, put `.\` in front of the program: `.\quillstone-player examples/demo.quill.md`,
`.\quillstone check ...`.)

Then make your first game. Make a folder `mygame/` and put this in `mygame/mygame.quill.md`:

````markdown
---
title: My First Game
---
::character{id=mika name=Mika color=#ffd98c}

# start
**Mika:** Hello! This is my first game.

::choice
- [Say hello](#hello)
- [Say nothing](#quiet)
::

# hello
**Mika:** Nice to meet you.
::end

# quiet
**Mika:** Shy, are you? That's fine.
````

Run it with `quillstone-player mygame/mygame.quill.md`. Leave it running, change a line, and save:
the game updates within half a second, at the same spot (hot reload).

Every command, choice, variable and effect is in [SCRIPT.md](SCRIPT.md). When something is wrong,
`quillstone check mygame/mygame.quill.md` says what, with the line.

## Playing

```
quillstone-player examples/demo.quill.md
```

Run without a script argument, the player looks for a game by itself, in this order:

1. `game.qsa` next to its executable;
2. `main.quill.md` (or the only `.quill.md` file) in a `game` folder next to it;
3. the same in the executable's own folder.

That is how a shipped game starts: players just double-click the exe. If it can't start, the
reason is also written to `error.txt` next to the exe.

### Hot reload

Leave the game running while you write. Saving the script, a file it includes or its theme
updates the game within half a second, at the same spot, the way a save would load after an
edit. The music keeps playing.

- If the edited script has errors, a notice shows the first one (all of them go to the console)
  and the old version keeps running.
- New pictures are picked up too. A new theme font needs a restart.
- It works when the player runs a script, not a `game.qsa`.

### Controls

In the story:

| Control | What it does |
| --- | --- |
| click / Space / Enter / Right | advance (the first press finishes typing the line) |
| 1-9 or click | pick a choice |
| Up / Down, Enter | pick out a choice, a menu entry, a save slot or (at a normal line) a clickable HUD button, and take it |
| Backspace / Left / wheel up | go back (rollback); wheel down rolls forward again |
| A | auto-forward on/off (waits for the voice clip; stops at choices) |
| Tab / hold Ctrl | skip lines already read |
| U | hide the textbox and quick menu to see the picture |
| F5 / F9 | quick save / quick load |
| F11 | fullscreen |

Screens:

| Control | What it opens |
| --- | --- |
| Esc / right-click | the save screen, which is also the game menu (see below) |
| S / L | the save / load screens |
| H / Page Up | the history (backlog) |
| I | the inventory, in games that have one |
| J | the journal, in games that have one (the inventory's screen, listing quests) |
| F12 | the developer console, only when the player was started with `--dev` (see "Developer tools" in SCRIPT.md) |

The details:

- **Choices.** A timed choice picks its default option when the bar runs out. On a hotspot
  screen the arrow keys cycle the regions.
- **Skip** stops at unread lines, choices and the end. With "Skip unread text" on in Settings it
  skips everything up to a choice.
- **U** hides the textbox until the next key press or click, which brings it back and does
  nothing else.
- **The game menu.** The save screen has a row of tabs along the top: Save, Load, Settings,
  History, Main menu, Quit. Click one, or press Tab to move on to the next screen. Esc or
  right-click on any of those screens closes it again. From the main menu the tabs are Load,
  Settings and Quit.
- **Settings** holds text speed, text size (100% to 150%), auto-forward delay, skip unread, music / sound / voice volume,
  fullscreen, the quick menu, and the language in a game that has several. The quick menu's
  Settings button opens the same screen.
- **Save and load.** There are 9 pages of 6 slots; the load screen also has the Auto and Quick
  pages. Left/Right turn the page.
- **History.** Scroll with the wheel, Up/Down, Page Up/Down or Home/End. H, Esc, right-click or
  a click closes it.
- **Inventory and journal.** Over the story, Esc, right-click or the same key (I or J) closes
  them.
- **A shop, or an inventory the script opened.** Esc and right-click open the save screen
  (closing it returns), and S / L / H / F5 / F9 work as in the story. Only the Leave / Close
  button leaves (or Backspace; or I in the inventory).
- **Typing.** At a text input the keys type into the field: Enter or OK confirms, Backspace
  deletes, Ctrl+V pastes, Esc opens the save screen. Shortcut letters are off there.
- **Extras** (from the main menu). Click or Enter opens an entry, Tab switches between Gallery,
  Music room and Scenes, and Left/Right turn the page. In a picture or clip, Left/Right move
  on, and any click, Esc, Enter, Space or Backspace closes it. Esc, Backspace or right-click
  goes back.
- **A question** (Quit, Overwrite, ...). Enter or Y says yes; Esc, N or right-click says no.

**Gamepads** (Xbox, PlayStation, Switch Pro, Steam Deck) work too:

| Button | What it does |
| --- | --- |
| A | advance or confirm |
| B / Start | Esc: in the story it opens the save screen, elsewhere it closes the screen |
| X | roll back (and leave a shop) |
| Y | history |
| LB / RB | auto-forward on/off / skip on/off |
| hold RT | skip |
| LT / Back | the load / save screens |
| D-pad / left stick | move around |

### Menus, saves and the window

**Menus.** The game opens at a main menu and returns to it at the end of the story: Start,
Continue, Load, Settings, Quit, and Extras when the game has a gallery, music room or scene
replays.

The quick menu under the textbox does with the mouse what the keys do: Back, History, Skip,
Auto, Save, Load, Q.Save, Q.Load, Settings, and Items and Journal in a game with an inventory or
a journal. A game may show it only while the mouse is over the textbox, or not at all; Settings
lets you choose. A theme can draw the buttons as pictures at places of its own.

**Saves.** The game saves automatically whenever a choice comes up and when you quit. Settings
and saves are kept per user:

| System | Folder |
| --- | --- |
| Windows | `%APPDATA%\Quillstone\<game>\` |
| macOS | `~/Library/Application Support/Quillstone/<game>/` |
| Linux | `~/.local/share/quillstone/<game>/` |

`<game>` is the script's `id:` front matter key. Without one it is the title, or else the
script's file name. Renaming the title then moves the saves, so set `id:` before you ship.

**The window.** The first time, it opens at 1920x1080 (the game's own size), or at the largest
16:9 size that fits the monitor with some margin, centred. After that the player remembers the
window's size, position and full-screen or maximised state (in `settings.txt`) and restores
them, kept on screen if the monitors changed.

Minimising the window pauses everything (music, sound, voice, videos, waits, typing, animations
and timed choices). Restoring it carries on exactly where it stopped.

### Options for testing

- `quillstone-player <game> --unlock-all` shows every gallery, music room and scene entry as
  unlocked for that run, to test the Extras screen. Nothing is written to the saved unlocks.
- `quillstone-player <game> --dev` adds a developer console on F12 (inspect and set variables,
  jump to a label). It also allows `--start <label>` and `--set var=value` (repeatable) to begin
  a new game there; see "Developer tools" in SCRIPT.md. Only the flag turns it on, so a shipped
  game has none of it.
- Videos play with FFmpeg, which the kit includes. `QUILLSTONE_TEST_VIDEO=1` plays a test
  pattern in place of every video, to check the pipeline without FFmpeg.

**Troubleshooting**
- No sound device? The player prints a warning and runs silent.
- The player can also run with parts turned off, to narrow a problem down: see [BUILDING.md](BUILDING.md).

## Check and test a story

```
quillstone check examples/demo.quill.md                 # validate script + assets
quillstone test  examples/demo.quill.md                 # play every branch, report story bugs
quillstone run   examples/demo.quill.md                 # play in the terminal
quillstone dump  examples/demo.quill.md                 # show the compiled ops
```

All commands (`<script>` is a `.quill.md` file; the script's folder is the game folder). Run them from the kit's folder, or put it on your `PATH`:

| Command | What it does |
| --- | --- |
| `quillstone check <script>` | validate the script, its theme, translations and that every file exists; warns about unused files |
| `quillstone test <script> [--limit <n>] [--allow-partial]` | play every branch and report story bugs (below) |
| `quillstone graph <script> [--format mermaid\|dot] [-o <file>]` | draw the story as a flowchart (Mermaid by default, to the screen unless `-o` names a file) |
| `quillstone run <script> [--start <label>] [--set var=value]...` | play in the terminal; `--start` begins at a label and `--set` (repeatable) gives variables a starting value |
| `quillstone dump <script>` | print the compiled ops and labels |
| `quillstone voices <script>` | list every line's voice file name, speaker and text (tab-separated, for the actors) |
| `quillstone lang export <script> <code>` | create or update `lang/<code>.txt` for a language the script lists |
| `quillstone package <script> --out <folder> [--player <exe>] [--name <name>] [--libs <folder>] [--folder] [--zip]` | build a game folder to ship (see "Ship a game"); `--player` defaults to the `quillstone-player` beside `quillstone` |
| `quillstone unpack <game.qsa> --out <folder>` | write the files of an archive into a new folder |

`test` plays every choice branch and reports what it finds, with the choices that lead there:

- endless loops that never show anything (an error);
- places the story can never end from (an error);
- labels and lines no branch reaches;
- choice options whose condition is never true;
- variables that are read but never set (usually typos).

Only the first two are errors; the rest are warnings.

| Exit code | Meaning |
| --- | --- |
| `0` | it finished and found no error |
| `1` | a real problem |
| `2` | it stopped at the `--limit`, so only part of the story was played |

`--limit` is how many situations it explores, 100000 by default (variables that keep growing can
make a story endless). Raise it, or pass `--allow-partial` if a partial run is fine for you: the
exit code is then `0`.

## Ship a game

```
quillstone package mygame.quill.md --out MyGame --zip
```

`MyGame/` then holds:

| File | What it is |
| --- | --- |
| `<Title>.exe` | the player, named after the game's title (`--name` picks another name) |
| `game.qsa` | the script and only the files it and its theme use, in one file |
| five FFmpeg libraries | avcodec, avformat, avutil, swscale and swresample (DLLs on Windows; never avfilter or avdevice) |
| `licenses/FFmpeg.txt` | the LGPL notice FFmpeg requires |

With `--zip` you also get `MyGame.zip` beside the folder (it unzips to `MyGame/`). Players unzip
it and double-click the exe.

`game.qsa` stores the files as they are, not compressed or scrambled: it is only a tidy way to
ship them.

What `package` does for you:

- It checks the script first, and warns about asset files nothing uses (it leaves those out).
- It refuses assets outside the script's folder.
- It never overwrites an existing `--out` folder that isn't empty.
- Without `--libs`, it takes the FFmpeg libraries from beside the player.
- Without `--player`, it uses the `quillstone-player` beside the `quillstone` program. The kit
  therefore needs neither option.

**Game icon.** A game gets the Quillstone icon. To use your own, set `icon: assets/icon.png` in
the front matter: a square PNG, JPEG or WebP, 256x256 or larger, with a transparent background.
`check` warns when it isn't square or is smaller.

The window shows the icon. When the player is a Windows `.exe`, `package` also writes it into
the renamed exe, so Explorer, the desktop and the taskbar show it. It goes in at 16, 32, 48 and
256 pixels (the small sizes are sharpened automatically), and this works from any system. The
player you built is never changed, only the copy in `--out`.

**Loose files.** Add `--folder` to get a `game/` folder of loose files instead of `game.qsa`.
To see what is in an archive, `quillstone unpack MyGame/game.qsa --out look-inside` writes its
files into a new folder.

**Patches and mods.** Put a file at the same path in a `mods/` folder beside the exe and it
replaces the shipped one: a new `assets/room.png`, a fixed `chapters/ch2.quill.md`, a
`lang/fr.txt` translation. A file with a path the game doesn't have yet is simply added (a voice
recording in the voice folder, say). `quillstone-player path/to/game.qsa` does the same with the
`mods/` folder beside that archive.

The folder is `mods`, not `game`, on purpose. A `game/` folder is what `--folder` releases ship,
and one left over from an older release must not win over a newer archive.

**Smaller games.** The kit's FFmpeg libraries are already a slim build with only what Quillstone
plays: VP8/VP9 video in WebM, Vorbis and Opus sound. A full FFmpeg (~130 MB of DLLs) works too:
package with `--libs <its bin or lib folder>`. To build the slim one yourself, see
`tools/build-ffmpeg.sh` and [BUILDING.md](BUILDING.md).

**When a shipped game fails.** The kit's player has no console window, so a shipped game shows
no messages. If it can't start or crashes, the reason is written to `error.txt` next to the exe.

**Linux.** The player loads FFmpeg's `.so` files from its own folder, so the same layout works
there.

## Script format

````markdown
::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

# rooftop
::bg{src=assets/rooftop.png fade=0.5}
::show{mika neutral x=80% fade=0.4}

**Mika:** You came after all, {name}.

::choice
- [Tell the truth](#truth){trust+=1}
- [Say nothing](#silence){if met}
::
````

See [SCRIPT.md](SCRIPT.md) for every command, conditions, variables and flow control.
The `examples/` folder has a script for each area:

| Example | Shows |
| --- | --- |
| `demo.quill.md` | a bit of everything: the place to start |
| `hotspots.quill.md` | clickable regions: a map, a collectible, an exit, hover pictures, an imagemap and a region that returns |
| `minigame.quill.md` | a timed screen with a moving target, a timing bar and key prompts |
| `shop.quill.md` | a list variable used as an inventory in a shop |
| `market.quill.md` + `market_theme.txt` | items, the inventory and shop screens, a quest journal, a HUD button, and a theme that restyles them |
| `hud.quill.md` | on-screen text, bars and pictures |
| `stats.quill.md` | grouped stats, arithmetic, `min`/`max`/`abs`/`count` and dice rolls |
| `greeting.quill.md` | a typed-in name |
| `staging.quill.md` | sprite height and layering, text sizes, `{nw}` and a colour wash |
| `anim.quill.md` | frame animations (talking, a one-shot, an animated background) and looping moves |
| `layers.quill.md` | a sprite built from body, eyes and mouth layers, with blinking eyes |
| `places.quill.md` | the background and a pose picked from variables |
| `weather.quill.md` | snow, rain and falling petals |
| `gallery.quill.md`, `replay.quill.md` | the Extras screen: a gallery with pictures and clips, and scene replays |
| `iconmenu.quill.md` + `icons_theme.txt` | picture buttons in the quick menu, placed by the theme |
| `chapters.quill.md` + `chapters/` | a story split over several files with `::include` |
| `lang.quill.md` + `lang/sv.txt` | a game in English and Swedish |

## Contributing

Bug reports and feature ideas are welcome as GitHub issues. Pull requests aren't
being taken yet. To build Quillstone from source, see [BUILDING.md](BUILDING.md).

## License

The engine is licensed under either of [MIT](LICENSE-MIT) or
[Apache License 2.0](LICENSE-APACHE), at your option. Unless you say otherwise, any
contribution you submit is licensed the same way.

The example scripts and their art, sounds and video in `examples/` are released into the
public domain under [CC0 1.0](examples/LICENSE-CC0): use them for anything.

The Quillstone logo (`qs_logo.jpg`) is not covered by these licenses; all rights reserved.

Packaged games include FFmpeg, which is licensed under the LGPL; `quillstone package`
adds its license notice to every game.
