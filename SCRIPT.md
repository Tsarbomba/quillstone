# Quillstone script reference

A script is a markdown file (use the extension `.quill.md`). It is read top to
bottom; the player advances through it one *waiting* step at a time (a line, a
wait, a video, a choice). Everything else runs instantly in between.

`quillstone check script.quill.md` validates a script and that its files exist;
`quillstone test script.quill.md` plays every branch and reports story bugs (loops
with no way out, unreachable parts, options never offered, misspelt variables);
`quillstone run script.quill.md` plays it in the terminal; `quillstone dump` shows
the compiled form. Errors name the line.

## Layout of a file

````markdown
---
title: My Story
theme: theme.txt
vars: { trust: 0, met: false, name: "Alex" }
---

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

# rooftop

::bg{src=assets/rooftop.png fade=0.5}
::show{mika neutral at=right fade=0.4}

**Mika:** Hello, {name}.
She looks away.
````

- **Front matter** (optional, between `---` lines): `title:` (also shown on the
  main menu), `theme:` (a theme file, see [Themes](#themes)), `vars:`,
  `inventory:` (see [Inventory](#inventory)), and `persistent:`, `gallery:` and
  `music_room:` (see [Kept across playthroughs](#kept-across-playthroughs)).
- **Headings** (`#` to `######`) are *labels*: places to jump to. `# The Roof` is
  the label `the-roof` (lowercase, words joined by dashes). A heading like
  `# heal(amount, who)` is a label with parameters, see
  [Labels with parameters](#labels-with-parameters).
- **`**Name:** text`** is dialogue. Any other prose is narration. Consecutive
  lines join into one paragraph; a blank line, a new `**Name:**` or a command ends it.
- **`<!-- comments -->`** may span several lines and are ignored. Inside a
  `::choice` block they must fit on one line.
- **Commands** are one line: `::name{key=value key="text with spaces" flag}`.
  Times are in seconds. Paths are relative to the script file and must stay in its
  folder or below it (no `..`, no drive letters, no backslashes), with `/` between
  folders. The script is refused with a line-numbered error when a `src=`, `icon=`,
  `image=`, `sprites=`, inline image, `gallery:`, `music_room:` or `theme:` path breaks that
  (otherwise its saves could never be loaded again).

## Variables

Values are whole numbers, `true`/`false`, or `"text"`. Declare starting values in
`vars:`; any other variable springs into existence at `0` when first read.
Names are letters, digits and `_`, but not `true`, `false`, `and`, `or` or `not`.
Lists of items (an inventory) have their own [section](#list-variables).

- `::set{trust+=1, met=true, name="Alex"}` sets one or more (`=`, `+=`, `-=`;
  `+=` on text appends). The right-hand side may be any [expression](#expressions):
  `gold -= price`, `cost = base * 2 - discount`, `hp = min(hp + 4, max_hp)`.
- `{name}` inside a line or choice text is replaced by the variable's value.
  `\{` writes a literal brace; an unknown `{name}` is left as written. Text shows
  variables only, never a calculation: store the result first (`::set{left = max_hp - hp}`,
  then `{left}`).

### Dotted names

A name may have several parts joined by dots, to group related variables:
`hero.love`, `hero.hp`, `stats.hp.max`. Each part follows the rules above (letters,
digits and `_`, not starting with a digit) and none may be empty, so `hero.`, `.hero`
and `hero..love` are errors. The dot is only a way of writing the name: `hero.love` is
an ordinary variable everywhere, in `vars:` and `persistent:` (`vars: { hero.love: 0 }`),
`::set`, conditions, choice effects, `{hero.love}` in text, saves, and list variables
(`bag.items += "key"`, `bag.items has "key"`, `count(bag.items)`). Character ids,
label parameters and `::call` argument names stay plain, without dots. Numbers have no
decimals (`1.5` is an error: variables hold whole numbers).

## List variables

A list holds a set of items, such as an inventory. Declare it in `vars:` (or
`persistent:`) with `[]` or a starting list; items are text, in quotes if they
have spaces or commas:

````markdown
---
vars: { gold: 30, inventory: [], pack: [map, "magic key"] }
---

# shop
Your bag holds: {inventory}.
::choice
- [Buy the key (15 gold)](#shop){gold -= 15, inventory += "key", if gold >= 15 and not inventory has "key"}
- [Sell the map](#shop){gold += 5, inventory -= "map", if inventory has "map"}
- [Leave](#leave)
::
# leave
::if{inventory has "key"}
The key turns in the lock.
::endif
````

- `inventory += "key"` adds an item (nothing happens if it is already there);
  `inventory -= "key"` removes it (nothing happens if it isn't). The right side
  may be a text variable: `inventory += found`. A list has no duplicates and
  remembers the order items were added in.
- `inventory has "key"` is true if the list holds that item. The left side must
  be a list variable; the right side is a text, a variable, or a parenthesised
  expression. `has` binds tighter than `not`, `and` and `or`:
  `not inventory has "key" and gold >= 15`.
- `{inventory}` shows the items joined with `, ` (an empty list shows nothing).
  A bare `inventory` in a condition means "is not empty". `==` and `!=` between
  two lists compare their items, ignoring order.
- A bare list can't be used as a value: `copy = inventory`, `x += inventory` and
  `::call{to=heal who=inventory}` are errors (they would be silently ignored). Ask
  with `inventory has "key"` instead.
- Lists change only with `+=` and `-=` of text. `inventory = "x"`, adding a
  number, and `has` on a variable that isn't a declared list are errors that
  `quillstone check` reports with their line. (A computed value that isn't text
  is ignored at run time.) In `::jump{if=...}` write quotes inside as
  `if="inventory has 'key'"`.
- Rollback, saves and `persistent:` lists work like any variable.
  `quillstone test` warns about an item tested with `has` that nothing ever adds
  (`inventory has "kye"`). It plays every combination of items the script can
  add, so a shop with a dozen independent items makes the test very large; it
  stops at its situation limit and says so.

## Text tags

Inside lines and choice texts:

| Write | Shows |
| --- | --- |
| `**bold**`, `*italic*`, `***both***` | bold, italic, bold italic |
| `{color=#ff8080}words{/color}` | words in that colour (`#rrggbb` or `#rgb`; they nest) |
| `{size=+10}big{/size}` | text 10 pixels bigger than the text around it (`{size=-8}` smaller); they nest, so `{size=+10}a{size=+4}b{/size}{/size}` makes `b` 14 bigger |
| `{size=24}small{/size}` | text exactly 24 pixels tall (on the 1920x1080 screen; 8 to 200, as is the result of a relative size) |
| `First.{p}Second.` | a line break inside the line (a paragraph break); it doesn't wait. A `{p}` at the very start or end of a line is ignored. Not allowed in a choice text |
| `Well...{w=0.5} no.` | the typing pauses for half a second there (up to 10; a click skips it) |
| `Hold on.{nw}` | "no wait": once the line has finished typing the story goes on by itself, without a click or the auto-forward delay. It must be the very last thing in a dialogue line (not a choice text) |
| `\*`, `\{`, `\\` | a literal `*`, `{` or `\` (any other `\` is shown as written, so `C:\temp` is fine) |

A `{` with no `}` after it is just a brace, and an unknown `{name}` is left as
written. `{p}` and `{nw}` are tags, so a variable with one of those names can't be
shown with `{p}` / `{nw}`. Mistakes (an unclosed `**` or `{size=..}`, a size outside 8
to 200, `{nw}` in the middle of a line, `{p}` in a choice, an unknown tag like
`{huge=40}`) are errors with the line number. A variable's value always shows as it is, even if it
contains `*` or braces. The history and save previews show the words without
styling or tags (the history keeps a `{p}` break; a save preview shows it as a space). A line with `{size=..}` is as tall as its biggest text, so a bigger
word makes its line taller.

`{nw}` waits for the typing and for any `{w=..}` pause, including one at the very
end (`Well...{w=3}{nw}` stays three seconds, even with instant text); a click
still shows everything and cancels the pause. It does not wait for a voice clip:
the next line's batch stops the voice (a deliberate choice; put a `{w=..}` at the
end if the clip must play out).

With `{nw}` the line is still added to the history, and a `::choice` after it
appears once the line is typed (the line stays above the choices). It waits while
a menu is open, skip mode runs through it like any line, and going back never
stops on it (it would only move on again): rollback goes to the line before.
Use it for a line that leads straight into a choice or a scene change. For bold and italic in a theme font, give the theme its bold and
italic files too (`font_bold`, `font_italic`, see [Themes](#themes)).

## Conditions

Used by `::if`, `::jump{if=...}` and choice options `{if ...}`:
`==  !=  <  <=  >  >=` (a lone `=` also means equals), `and` / `or` / `not`
(also `&&` `||` `!`), parentheses, and a bare name means "is not 0/false/empty".
`trust > 1 and not met`, `name == "Mika"`, `(a or b) and c`. Either side of a
comparison may be a calculation: `hp * 2 < max_hp`, `count(inventory) >= 2`
(see [Expressions](#expressions)).

`seen(label)` is how many times the story has passed that label (0 if never), so
it works both as a yes/no and as a count: `::if{seen(cafe)}`, `seen(cafe) >= 3`.
The label is written like a link target (`seen(the-roof)`, or `seen(#The Roof)`).
Rollback and saves keep the counts; they belong to one playthrough.

## Expressions

Expressions appear wherever the language takes a value or a condition: `::if` and
`::elif`, `::jump{if=...}`, the `if` of a choice option, hotspot or shop line, the right
side of `::set` and of choice effects, and `::call` arguments. They are made of
numbers, `true`/`false`, `"text"`, variables, operators and the functions below.

| Operator | Meaning | Binds |
| --- | --- | --- |
| `( )` | grouping | tightest |
| `-x` | negative | |
| `*  /  %` | multiply, divide, remainder | |
| `+  -` | add, subtract | |
| `==  !=  <  <=  >  >=`, `list has item` | compare | |
| `not` (`!`) | the opposite | |
| `and` (`&&`) | both | |
| `or` (`\|\|`) | either | loosest |

Operators of one level work left to right: `10 - 4 - 3` is `3`. `hp + 4 > max_hp and not
dead` reads as `((hp + 4) > max_hp) and (not dead)`. A comparison does not chain
(`a < b < c` is an error); write `a < b and b < c`.

**Numbers** are whole numbers (from about -9.2 × 10^18 to 9.2 × 10^18). Arithmetic never
fails: results stop at the ends of that range instead of wrapping around.

- `/` divides and drops the fraction toward zero: `7 / 2` is `3` and `-7 / 2` is `-3`.
- `%` is the remainder and takes the sign of the left side: `7 % 2` is `1`, `-7 % 2` is `-1`.
- Dividing by zero, or taking a remainder by zero, gives `0` instead of stopping the
  game. Writing a literal `0` as the divisor (`hp / 0`) is an error when the script is
  checked.
- `true` counts as `1` and `false` as `0`. A text, a list or an unset variable counts as
  `0` in arithmetic. Where the declarations show that an operand is text or a list
  (`name + 1` with `name: "Alex"`, `1 + "a"`), the script is refused with the line instead.
  Text is joined with `+=` (`name += "!"`), and a number added with `+=` to a text variable
  is appended as text.
- `x += expression` and `x -= expression` take any expression on the right.

**Functions.** This is the whole list; there are no user-defined functions.

| Function | Result |
| --- | --- |
| `min(a, b, ...)` | the smallest value (at least two) |
| `max(a, b, ...)` | the largest value (at least two) |
| `abs(x)` | the value without its sign |
| `count(list)` | how many items the list variable holds (`count(inventory)`) |
| `random(lo, hi)` | a whole number from `lo` to `hi`, both included (see below) |

`count` needs a list variable declared in `vars:` or `persistent:`. An unknown function
or a wrong number of arguments is an error that names the line and lists these
functions. A variable may share a function's name (`count += 1` still works).

````markdown
::set{hero.hp = min(hero.hp + 4, hero.max_hp)}      heal, but not above the maximum
::set{hero.hp = max(hero.hp - dmg, 0)}              hurt, but not below zero
::set{cost = base * 2 - discount}
::if{count(inventory) >= 2 and abs(a - b) < 3}
````

### Random numbers

`random(lo, hi)` picks a whole number from `lo` to `hi`, both included. It can only be
the right side of an assignment, alone or inside a calculation:

````markdown
::set{roll = random(1, 6)}
::set{dmg = random(2, 4) + bonus}
- [Swing the sword](#fight){dmg = random(2, 5)}
````

Using it in a condition (`::if{random(1, 6) > 3}`, an option's `if`, a `::call` argument)
is an error that says to assign it to a variable first: write `::set{roll = random(1, 6)}`
and then test `roll`. Screens are rebuilt from their conditions after a rollback or a
load, and a roll inside one would come out differently each time. With literal bounds,
`lo` must not be larger than `hi`; with variables as bounds, a swapped pair is put in
order when it runs.

Rolls are repeatable *within a game*. The game keeps a random seed as part of the story
state, and every roll moves it on. Going back with rollback restores the state, seed
included, so the same choice made again gives **the same roll**, and rolling forward
replays the rolls too: players cannot re-roll by going back. Saves keep the seed, so a
loaded game continues with the rolls it would have had. A new game starts from a fresh
seed chosen by the player program, so each playthrough rolls differently. (`quillstone
run` and `quillstone test` use one fixed seed.)

`quillstone test` treats a roll as a branch: a range of up to 12 values is tried value by
value, and a wider one at its lowest, middle and highest value, with a warning that the
rest were not tried.

## Scene

| Command | What it does |
| --- | --- |
| `::bg{src=... fade=1}` | Background; crossfades over `fade`. May be a **video** file (plays looped). |
| `::bg{src=... fade=1 through=#000000}` | Change the background **through a colour**: the stage fades to the colour over half of `fade`, the picture is swapped (no crossfade) and the colour fades away over the other half. It waits for the first half like `::fade{out}` does. `through` on its own is black; `through` needs a `fade` above 0 and can't be combined with `wipe`. Sprites are not touched: use the explicit `::fade` pair below when sprites change too. Use it on an uncovered screen: it ends uncovered. |
| `::bg{src=... fade=1 wipe=right}` | Wipe instead of crossfading: the new picture slides in from one side over `fade` seconds. `wipe`: `left`, `right`, `up`, `down` (the direction the edge travels). |
| `::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}` | Declare a character: name-plate text and colour, and a sprite path where `{pose}` is filled in. Declare before use. `**Mika:**` lines pick up the name and colour automatically. |
| `::show{mika happy at=left fade=0.4 scale=1}` | Show a sprite (or change its pose; the new pose crossfades and keeps its position and size). `at`: `far-left` `left` `center` `right` `far-right`, a number 0 to 1, or `30%`. Without `at` it keeps its position, or centres if new; without `scale` (0.05 to 10) it keeps its size, or is 1 if new. |
| `::show{mika happy y=0.9 z=2}` | `y`: where the sprite's **bottom edge** is, as a fraction of the screen height (a number or `N%`, from -0.5 to 1.5; default 1, the bottom of the screen). Less than 1 lifts the sprite, more than 1 sinks it below the screen edge; the screen cuts off what hangs outside. `z`: a whole number from -100 to 100 (default 0); sprites with a higher `z` are drawn in front, and equal ones in the order they were shown. `z` only orders sprites among themselves: the background stays behind them and the textbox and menus stay on top. Showing a sprite again keeps its `y` and `z` unless you give them. |
| `::show{mika happy flip xscale=1.5 yscale=0.8}` | `flip` mirrors the sprite left to right (`flip=false` turns it back). `xscale` and `yscale` (each 0.05 to 10, default 1) stretch it along one axis, on top of `scale`; it keeps standing on its `y` line and stays centred on `at`. Showing a sprite again keeps its flip, `xscale` and `yscale` unless you give them. |
| `::show{src=assets/ghost.png id=ghost at=center}` | A sprite that isn't a character. |
| `::hide{mika fade=0.5}` / `::hide{all}` | Remove one sprite or all. |
| `::move{mika at=right time=0.8}` | Slide a sprite. |
| `::move{mika scale=1.2 time=0.5 ease=out}` | Grow or shrink it (can be combined with `at` and `y`). `ease`: `inout` (default: slow at both ends), `in`, `out`, `linear`. |
| `::move{mika y=0.9 time=0.3 ease=out}` | Slide it up or down the same way (`y` as for `::show`). `z` can't be animated; change it with `::show`. |
| `::move{mika xscale=1.4 yscale=0.7 time=0.5}` | Stretch it along one axis (as for `::show`); combines with `at`, `y` and `scale`. `flip` (or `flip=false`) can be added to a move too, but a mirror can't be animated: it switches at the **start** of the move. |
| `::camera{zoom=1.3 x=0.3 y=0.4 time=4}` | Pan and zoom the stage (background and sprites; the textbox and menus stay put). `zoom` 1 to 4; `x`, `y` (0 to 1, or `30%`) is the point to centre on, kept so the view never leaves the stage. Anything left out stays as it is; `ease=` as for `::move`. Doesn't wait. |
| `::camera{reset time=1}` | Back to the whole stage. |
| `::shake{time=0.4 strength=20}` | Shake the stage, dying away over `time` seconds (`strength` in pixels at 1920x1080, up to 200). Doesn't wait. While it lasts the stage is zoomed in slightly about the centre (by just enough to hide the edges), easing in and out with the shake. |
| `::shake{axis=x}` / `::shake{axis=y}` | Shake along one axis only: `x` sideways, `y` up and down, `both` (the default) both ways. A one-axis shake moves the stage by up to `strength` pixels along its axis and not at all along the other, and zooms in only as far as that axis needs. |
| `::flash{time=0.4 color=#ffffff}` | A flash over the whole screen: the colour jumps to full strength and fades out over `time` seconds (more than 0, up to 10; default 0.4). `color` is `#rrggbb` or `#rgb` (default white). Doesn't wait. It is drawn over the stage and inline images but under the textbox and menus. |
| `::fade{out time=0.5 color=#000000}` | **Dip to a colour**: the whole stage (background, sprites, inline images, video and camera) fades to the colour over `time` seconds (more than 0, up to 10; default 0.5; `color` as for `::flash`, default black) and **stays covered**. It waits until the fade is done, like `::wait`; a click, or skip mode, ends it at once and the story goes on. The textbox, choices and menus are not covered, so a line of narration works over a black screen. |
| `::fade{in time=0.5}` | Fade the cover away over `time` seconds. Doesn't wait. With nothing covering the screen it does nothing. |
| `::textbox{hide}` / `::textbox{show fade=0.5}` | Hide the whole dialogue box (textbox, name plate, line and quick menu) for a clean picture, or show it again. `fade` is a number of seconds above 0, up to 10 (default: at once). Doesn't wait. A hidden box comes back by itself with the next line or choice, so the story never moves on unseen; clicks still advance while it is hidden. On a `::hotspots` screen it stays hidden, and the line that is pending isn't drawn either, which gives a clean map: show it again with `::textbox{show}` or just say a line. Rollback, saves and hot reload keep it. |
| `![alt](assets/x.png)` | An inline image. It appears **with the next line that waits** and disappears when the player advances, so put it *before* the line it illustrates. (`![](x.webm)` plays a video.) |

Sprites stand on the bottom edge of the screen unless `y` says otherwise, about
940 px tall at `scale=1` (on a 1920x1080 screen), and the text box draws over their
lower part. To put one sprite behind another, give it a lower `z`. The camera, sprite sizes,
heights, stretch, mirroring, `z` and the colour cover are part of the scene, so rollback
and saves restore them (a save made before `::fade` existed loads uncovered, one made
before `flip` and the scales existed loads with unflipped, unstretched sprites); a shake or a flash is a moment and
isn't replayed. A flash stands still while a menu is open and ends at once when the
player skips.

The cover is drawn over the stage, inline images, video and hotspot pictures, under a
`::flash` and under the textbox and everything else on the interface. Rollback goes
past the wait of a `::fade{out}` (like a `::wait`), so going back never stops halfway
through a fade, and roll forward does the same. The last line stays on screen while the
screen fades out; hide the box first (`::textbox{hide}`) for a clean fade.

## Sound and video

| Command | What it does |
| --- | --- |
| `::music{src=... fade=1 volume=0.8 loop=false}` | Background music; loops unless `loop=false`. `volume` 0 to 1. A different track crossfades over `fade` seconds. |
| `::music{src=loop.ogg intro=intro.ogg}` | A track with an **intro**: `intro` plays once, and `src` then loops, without a gap. `intro` needs `src` (the loop) and a looping track (so not `loop=false`). `fade` fades the intro in. Both files are checked and packaged like any sound. |
| `::music{stop fade=2}` | Stop it. |
| `::sfx{src=door.ogg volume=0.5}` | One-shot sound effect. |
| `::sfx{src=rain.ogg loop volume=0.6 fade=1}` | A **looping** sound effect, fading in over `fade` seconds. It belongs to the scene, like music. |
| `::sfx{stop=rain.ogg fade=1}` | Stop that loop (and a one-shot of that file that is still playing), fading out over `fade`. The file must be one some `::sfx` plays. |
| `::sfx{stop}` | Stop everything on the sound-effect channel: all loops and all one-shots playing. |
| `::voice{src=line1.ogg}` | Voice clip for the **next** line; stops when the player advances. |
| `::video{src=clip.webm fit=cover loop skippable}` | Fullscreen clip; waits until it ends or the player skips it. `fit`: `contain` (default), `cover`, `stretch`. A `loop` video is always `skippable` (it never ends by itself). |
| `::wait{time=1.5}` | Pause; a click skips it. |

### Auto-voice

For a voiced game, put `voice: voice` in the front matter and drop recordings
into that folder named after their lines; each line with a recording plays it,
no `::voice` needed (an explicit `::voice` still wins). The names look like
`rooftop-1a2b3c4d.ogg`: the line's label and a code made from its speaker and
text, so a recording stays with its line when other lines change, and a line
that is reworded needs recording again (it gets a new name).
`quillstone voices script.quill.md > lines.tsv` lists every line's file name,
speaker and text, ready for a spreadsheet and the voice actors, with a
"recorded" column once the folder exists. `quillstone check` says how many lines
are voiced and warns about recordings that match no line (their line was
reworded or removed); `package` copies only the recordings in use.

Audio formats: Ogg Vorbis (`.ogg`, recommended for music), Opus (`.opus`,
smallest for voice lines), WAV, MP3 and FLAC. Opus is decoded with FFmpeg, the
others built in. `quillstone check` refuses anything else (AAC/`.m4a` is
patent-licensed, so it isn't supported).

How sound behaves:

- `::music` with the track that is **already playing** doesn't restart it; only
  the volume changes (fading over `fade`). So it's safe to repeat a scene's
  `::music` line. A `loop=false` track that has finished does start again.
- A track with an `intro` is the pair of files. The same `src` and `intro` already
  playing never restarts, so repeating the line is safe, and rollback or loading a
  save into a scene where that pair is playing leaves it alone. The playing position
  is not saved: whenever the pair is started afresh (a save loaded from the main
  menu, another track playing before it) it starts from the intro. `src` with a
  different `intro`, or none, is another piece and crossfades. In the music room an
  entry is unlocked by its `src` file, and playing it there plays that file alone,
  without the intro.
- Rollback keeps the music going if the earlier moment had the same track, and
  switches or stops it otherwise. A voiced line's clip plays again when you roll
  back to it. One-shot sound effects are never replayed by rollback.
- A looping `::sfx` is part of the scene, like the music: starting a loop that is
  **already playing** doesn't restart it (only its `volume` changes), so repeating a
  scene's `::sfx{... loop}` line is safe. Rollback, loading a save and reloading the
  script keep the loops the earlier moment had playing and stop the others. Loops
  follow the sound volume slider; skipping mutes one-shots but not loops. A loop
  keeps playing until a `::sfx{stop}`, and the main menu ends it. `loop` needs a
  file that exists and plays, like music, and `quillstone package` includes it.
  Loops are streamed from disk, like music, so a long ambience costs no memory or
  start-up time. If a loop can't start (a missing or damaged file) it is tried again
  at the next `::sfx{loop}` of it, or when the scene is redrawn (rollback, load).
  There are no other channels: one-shots, loops and `stop` all use the sound channel.

Image formats: **PNG**, **JPEG** or **WebP**, at most 8192 pixels on a side;
`quillstone check` refuses anything else.

Video formats: **WebM** (`.webm`) with VP9 (recommended) or VP8 video and Opus
or Vorbis sound. These are royalty-free and they are what a shipped game can
play, so `quillstone check` refuses other videos (MP4, AV1, H.264, Ogg Theora...)
and shows how to convert them, e.g.
`ffmpeg -i in.mp4 -c:v libvpx-vp9 -crf 32 -b:v 0 -c:a libopus out.webm`.
Video sound isn't played yet. An unskippable video only blocks clicks while it
is really playing, so a file that can't be decoded never traps the player.

## Flow

````markdown
# somewhere
::jump{to=ending}                       jump to a label
::jump{to=ending if="trust > 1"}        only if true
::call{to=flashback}                    run a label like a subroutine...
::call{to=heal amount=5 who="Mika"}     ...which may take parameters (see below)
::return                                ...and come back after the ::call
::end                                   finish the story

::if{trust > 1}
**Mika:** Great.
::elif{trust > 0}
**Mika:** Fine.
::else
**Mika:** Hm.
::endif                                  blocks may nest

::choice
- [Tell the truth](#truth){trust+=1}
- [Lie](#lie){trust-=1, seen=true}
- [Confess everything](#everything){if met and trust > 0}
::
````

- A choice option is `- [text](#label){effects, if condition}`. The braces are
  optional; an option with an `if` is only offered while it is true. If no option
  is on offer, the story carries on past the block. Many options shrink to fit
  above the textbox; keys 1-9 pick the first nine.
- `once` makes an option go away after it has been picked: `- [Ask about the
  scarf](#scarf){once}`. A menu that jumps back to itself then offers fewer and
  fewer questions, and when all of them are `once` and used up, the story carries
  on past it. Options with the same text and label count as the same option.
- Falling off the end of a label continues into the next one, so end each scene
  with `::jump`, `::return` or `::end` when that isn't what you want.
- `::call` can nest up to 256 deep. `::return` with nothing to return to ends the story.

### Labels with parameters

A label can take values from the `::call` that runs it:

````markdown
::call{to=heal amount=5 who="Mika"}
::call{to=heal amount=gold who=name}
::end

# heal(amount, who)
{who} recovers {amount} points.
::set{hp += amount}
::return
````

- Write the parameter names after the label in parentheses, separated by commas:
  letters, digits and `_`, with no defaults and no types. The label is still `heal`
  (the heading text before the parenthesis), so `seen(heal)` works as usual.
- **The rule for parentheses in a heading.** The `(` must touch the name *and* the
  heading must end with `)` for it to be a parameter list. `# Round 1(a)` and
  `# Café(night)` are therefore labels with parameters (`round-1` taking `a`,
  `café` taking `night`). With a space before the `(`, like `# Chapter 1 (the start)`
  or `# heal (amount)`, the whole heading is just a title, and its label is the slug of
  all of it: `chapter-1-the-start`, `heal-amount` (the parentheses and spaces turn into
  dashes). If you meant a plain title and wrote `# Round 1(a)`, the errors that follow
  ("can run into", "can only be reached by `::call`") say so: put a space before the `(`.
- A `::call` passes every parameter as `name=value`, with exactly the names the
  heading lists. The value is an ordinary expression, as on the right of a `::set`
  (except that `random` is not allowed, assign it first): a number, `"text"` in quotes,
  `true`/`false`, a variable, a calculation in parentheses (`n=(gold - 5)`), or a parenthesised
  condition. Spaces are fine inside quotes or parentheses (`who="Mika Tanaka"`,
  `ok=(gold >= 5 and met)`); elsewhere a space starts the next argument.
- **Parameters are ordinary variables.** At the call, every value is worked out
  first (so `a=b b=a` swaps them) and then stored into the variables of those names
  before the label runs. Nothing is restored at `::return`: they keep their values
  afterwards, rollback and saves treat them like any variable, and `{amount}` in a
  line shows them. Pick parameter names that don't clash with variables the story
  uses for something else. A list variable can't be a parameter.
- A label with parameters can only be reached by `::call`. `quillstone check`
  reports, with the line: a call that leaves out a parameter, passes one the
  label doesn't have (a misspelling shows up as both), or passes arguments to a label
  without parameters; a `::jump`, choice link or hotspot link to it; and a label
  above it that runs on into it. End the label above with `::jump`, `::return` or
  `::end` (put subroutines after the story, as in `examples/shop.quill.md`).
  `quillstone test` plays them like any other label.

## Hotspots

Clickable regions over the current background: a map to pick a place from, an
object to find, a point-and-click scene.

````markdown
::bg{src=assets/map.png}
::hotspots
- [Library](#library){rect=0.1,0.2,0.2,0.3}
- [Old key](#found){rect=60%,70%,5%,8% image=assets/key.png, once, found=true, if not found}
::
````

- An entry is `- [label](#target){rect=x,y,w,h ...}`. `rect` is required: the
  region's left, top, width and height on the screen, each a number from 0 to 1 or a
  percentage (no spaces inside it), and it must lie inside the screen. The label
  shows as a small tooltip next to the mouse while the region is hovered.
- `image=assets/key.png` (PNG, JPEG or WebP) is drawn stretched into the region.
  Without one the region is invisible until the mouse or keyboard reaches it,
  when it is outlined.
- `hidden` makes a region secret: no outline, tint or tooltip when the mouse is
  over it, so a collectible can't be found by sweeping the mouse (an `image=` is
  still drawn). Keyboard and gamepad focus skip hidden regions, so a block needs
  at least one visible region (such as an exit) for those players.
  Example: `- [Old key](#found){rect=60%,70%,5%,8%, hidden, once, found=true}`.
  It is an error in a `::choice`.
- Effects, `if condition` and `once` work as in a `::choice`, and so does the
  `#label` it jumps to. Put the other settings after `rect` and `image`, separated
  by commas. A region that is `once` or whose `if` is false disappears; with none
  left the block is skipped. Where regions overlap, the one listed last is on top.
- The block normally follows a `::bg` with no line, so the regions are the only
  way on. A line shown just before it stays on screen. Jump back to a label that
  sets the background and the hotspots again to make a hub (see
  `examples/hotspots.quill.md`).
- Mouse: click a region. Keyboard and gamepad: the arrow keys (or D-pad) move
  between the regions and Enter or Space takes the focused one. The number keys
  don't pick hotspots. Skip and auto-forward stop at the block, it is an auto save
  point like a choice, and the pick isn't added to the history. Going back
  returns to the screen before the click.
- `quillstone check` checks the images and warns about a region that can never
  be clicked because a later one covers it entirely, or a block whose regions are all
  `hidden`; `quillstone test` follows
  every region like a choice option.

## Items

Items give the things in a list variable a name, a picture and a description for
the [inventory](#inventory) and [shop](#shop) screens. Declare them before they are
used, like characters:

````markdown
::item{id=key name="Brass key" icon=assets/gui/key.png price=15 text="Opens the clock tower."}
::item{id=map}
````

- `id` is the text stored in list variables (`inventory += "key"`): letters, digits,
  `_` and `-`. Declaring an id twice is an error.
- `name` is what the screens show (the id if left out). `text` is the description
  shown when the item is hovered or selected.
- `icon` is a picture (PNG, JPEG or WebP) drawn above the name. `price` is a whole
  number, 0 or more: what a shop asks unless its line says otherwise.
- A list may hold an id that was never declared: the screens show it under its id
  with no picture. `quillstone check` warns about a `+= "x"` or `has "x"` on the
  inventory list that names no item (usually a typo) and checks the icons.
  It also warns about an icon wider or taller than 512 px (icons are drawn at about
  100 px, so a smaller file loads faster and saves memory) and about a description
  too long for the description box.

## Inventory

````markdown
---
inventory: pack
vars: { pack: [] }
---
::set{pack += "key"}
You pick up a brass key.
::inventory
````

- `inventory: pack` in the front matter names the list variable the inventory shows;
  it must be declared as a list in `vars:` (or `persistent:`). The game then has an
  **Items** button in the quick menu, and the **I** key opens the screen (a game
  without `inventory:` has neither).
- The screen shows the items in the list, in the order they were added, as a grid of
  cells with the item's picture and name. Pointing at an item (or moving to it with
  the arrow keys) shows its description, and its price if it has one, in a box under
  the grid. With more items than fit, the mouse wheel, Page Up/Down and the arrow keys turn
  pages. It is view-only; to let an item do something, offer a choice with
  `if pack has "key"`.
- Opened by the player, it sits over the story, which waits: a `::wait` doesn't run
  out and nothing is skipped. Esc, a right click, the I key or the Close button closes it.
- `::inventory` opens the same screen from the script. The story waits until the player
  presses its Close button (or I, or Backspace) and then carries on; Esc or a right click opens the
  settings instead, which return to the inventory, and S, L, H, F5 and F9 work there too. Rollback, saves and hot reload work with it like with
  any other waiting step, and `quillstone test` treats it as one click.

## Shop

````markdown
---
inventory: pack
vars: { gold: 40, pack: [], polite: false }
---
::item{id=key name="Brass key" price=15}
::item{id=charm name="Lucky charm" price=20}

::shop{pay=gold into=pack}
- key
- charm{price=18, if polite}
::
````

- `pay` is the whole-number variable that pays; `into` is the list that receives what
  is bought. Both are required, `pay` must not be declared as anything but a number and
  `into` must be a list declared in `vars:`.
- Each line is `- id`, naming a declared item, with optional settings in braces:
  `price=18` replaces the item's price and `if condition` offers the item only while
  the condition is true. A line needs a price from one place or the other. An item may
  appear once per shop.
- The story waits at the shop, which shows every offered item with its picture, name
  and price, and how much the player has. The Leave button (or Backspace, the gamepad's X) ends the
  block and the story continues after it. Esc or a right click opens the settings
  instead, as everywhere else, and closing them returns to the shop (page, selection
  and purchases kept); S, L, H, F5 and F9 work there too (save, load, history, quick
  save and load). If no item is on offer the block
  is skipped.
- An item the player can buy is bought by clicking it, or by moving to it with the
  arrow keys and pressing Enter or Space: the price is taken from `pay`, the item is
  added to `into`, a notice says "Bought Brass key", and the shop stays open with the
  new money. An item already in the list is marked **Owned** and can't be bought again;
  an item that costs more than the player has is greyed out and a click only says
  "Not enough gold". Money can never go below zero.
- `pay` and `into` must both be `persistent:` or both ordinary variables
  (`quillstone check` reports the line). A persistent list with ordinary money, or the
  reverse, would give the item twice or lose both the money and the item when a purchase
  is rolled back, because rollback hands persistent values back but not ordinary ones.
- Purchases are ordinary variable changes, so rollback, saves, hot reload and
  `persistent:` variables work with them. A shop is an auto save point like a choice
  and stops skip and auto-forward; what the player buys isn't added to the history.
- `quillstone test` follows every purchase the player can afford, and leaving, as
  branches. Money only goes down and each item is bought once, so a shop you can come
  back to is still finite. It warns about an item whose `if` is never true and a `pay`
  variable nothing sets.
- The look of the inventory and the shop comes from the theme file (see below). The
  demo `examples/market.quill.md` with `examples/market_theme.txt` uses all of it.

## Text input

````markdown
---
vars: { hero.name: "Alex" }
---
::input{var=hero.name prompt="What is your name?" default="Alex" max=16}

**Mika:** Nice to meet you, {hero.name}.
````

Asks the player to type a text and stores it in a variable (a name, mostly).

- `var` (required) is a text variable declared in `vars:` or `persistent:` with a text
  default (dotted names work). `prompt` (required) is the question; it takes `{name}`
  values, `**bold**`, `*italic*`, colour and size tags like a line does (not `{nw}` or
  `{p}`). `default` (optional) is the answer used when the player confirms an empty
  field; the field then shows it dimmed as a hint. `max` (optional) is how many
  characters the answer may have, from 1 to 64 (24 when left out); it counts characters,
  not bytes, and `default` may not be longer.
- The story waits at a centred panel with the prompt, a text field with a blinking
  caret and an OK button. A long text scrolls so its last characters stay in view.
  Typing adds characters at the end and Backspace removes the last one (there is no
  moving the caret inside the text, and no paste). Enter or OK confirms. The panel
  counts what is typed against the limit.
- What is stored: control characters (newlines, tabs) are dropped, spaces at both ends are trimmed, and
  the text is cut to `max`. An empty answer stores the `default`; with no default OK is
  dimmed and Enter does nothing until something is typed. The value is always text, even
  when it looks like a number (`n == "42"`, not `n == 42`).
- Letters and digits go to the field while it is up, so shortcuts like A, H, S, L, I, U and Tab
  do nothing there; Backspace deletes and never goes back. Esc and a right click open the
  settings (closing them returns to the field with the text kept); F5 and F9 quick save and load.
  A gamepad's A confirms (the default when the field is empty), X deletes and B or Start
  opens the settings; there is no on-screen keyboard.
- Whatever the player types shows exactly as typed: `{`, `}`, `*` and `\` in a name can't
  become text tags when `{hero.name}` appears in a line or a prompt.
- The input is an auto save point and stops skip and auto-forward. Neither the question nor
  the answer is added to the history. Rolling back from the line after it lands on the
  prompt with an empty field; the variable is back to its earlier value until the new
  answer. Saves hold the stored value (the text typed but not yet confirmed is not
  saved), and a `persistent:` text keeps the last name for the next playthrough.
- `quillstone test` answers with the `default` (or `Player` when there is none) and goes on,
  so a line that only another answer reaches, like `::if{hero.name == "Zed"}`, is reported as
  never shown. `quillstone run` in the terminal asks on the command line (an empty line
  takes the default; `!back` and `!quit` are the commands there).
- The demo `examples/greeting.quill.md` asks for a name and uses it in a few lines.

## Kept across playthroughs

````markdown
---
persistent: { saw_good_end: false }
gallery: [assets/cg_rain.png, assets/cg_kiss.png]
music_room: [assets/calm.ogg, assets/theme.ogg]
---
````

- **`persistent:`** declares variables that outlive a playthrough (endings
  seen, routes unlocked). Use them like any variable; whatever the story sets
  them to is still there in every new game and every loaded save, and rolling
  back doesn't undo it. Replaying a line that *adds* to one adds again, so
  yes/no flags are the safe kind.
- **`gallery:`** lists pictures for the main menu's **Extras** screen; each one
  unlocks the first time the story shows it (`::bg` or an inline image).
  **`music_room:`** does the same for `::music` tracks (named after their file:
  `rainy_day.ogg` shows as "Rainy day"). The keys may repeat to make long lists.
  `quillstone check` complains about entries the story never shows or plays,
  since they could never unlock.
- `quillstone test` plays the story from a first playthrough and also from
  later ones (each value the script gives a persistent variable).

## What rollback restores

Going back one step restores variables, the random seed (so a roll repeats), the call stack, background, sprites and
their positions, heights, depths, sizes, stretch and mirroring, the camera, the colour cover of `::fade{out}`, music (an intro is not resumed part-way), looping sound effects, the inline images and voice of the line, and the line
itself. One-shot sound effects, shakes and flashes are not replayed.

Rollback (and roll forward, the redo after a rollback) keeps the last 500 steps;
going back further simply stops there, and the backlog lists what that history holds.
Roll forward is dropped when the state rolled back to moves on by itself (a
`::wait`, the wait of a `::fade{out}`, a video or a line ending in `{nw}`): rollback goes past those, so there is
nothing to come back to. Persistent variables are never rolled back: a rollback or a
loaded save shows the screen (choices, hotspots, shop stock, a line's `{variables}`)
rebuilt with their current values.

## Saves and edits

Saves remember positions as *label + how far past it*, so editing a script
doesn't break existing saves: anything added or removed above a label leaves
saves after that label where they were. If a saved spot no longer matches the
script (say, a line was inserted just before it), the game picks up from that
spot instead. A save whose label was renamed or removed can't be loaded. Skip
mode remembers read lines by their label and text, so rewording a line makes it
"unread" again. The same rules apply when the player reloads a script you just
saved (hot reload): it carries on where you are. A save that was waiting at a
choice, hotspot block or shop where nothing is on offer any more (an edited
condition) carries on past it. Choice texts, hotspots, shop stock and input prompts always come from
the script, never from the save.

Saves are shared between players, so loading is strict: a file path outside the game
folder, a list of more than 10,000 items, more than 1,000 stored states or more than 256
return addresses makes the save unloadable; a variable of the wrong kind (a list where
the script declares a number, or the reverse) is ignored; numbers like positions,
sizes, volumes and waits are clamped to what the language allows.

## Themes

The look of the game lives in a small text file named by `theme:` in the front
matter. Every line is optional; anything left out keeps the built-in look.
Paths are relative to the script. `quillstone check` validates the file and
checks that everything it names exists, is inside the game's folder, and is the right
kind of file (fonts are `.ttf` or `.otf`; pictures are PNG, JPEG or WebP).

```text
font = assets/fonts/MyFont.ttf      # used for all text; else the system font
font_bold = assets/fonts/MyFont-Bold.ttf          # for **bold** (same family)
font_italic = assets/fonts/MyFont-Italic.ttf      # for *italic*
font_bold_italic = assets/fonts/MyFont-BoldItalic.ttf
text_size = 38                      # dialogue and buttons (8 to 200)
name_size = 36                      # the speaker's name plate (48 px high: bigger names shrink to fit, down to 60%, so keep it under about 60)
text_color = #ffffff
name_color = #ffd98c                # name plates of characters without color=
accent_color = #ffd98c              # highlights, the title on the main menu
dim_color = #ffffff8c               # hints and secondary text
panel_color = #000000b8             # the textbox, if there is no textbox_image
button_color = #000000b3
button_hover_color = #40336be0
overlay_color = #050510e0           # behind settings, history and save/load
textbox_image = assets/gui/textbox.png   # stretched over the textbox (1600x260 unless textbox_rect says otherwise)
quick_menu = visible                # visible (default), hover or hidden (see below)
menu_background = assets/gui/menu.png
menu_music = assets/audio/menu.ogg
```

Colours are `#rrggbb` or `#rrggbbaa` (the last pair is opacity).

### The dialogue box

The textbox, the name plate and the quick menu under them can be moved and styled.
Positions and sizes are in the 1920x1080 virtual screen; without these keys the box looks as
it always has (a 1600x260 box at 160, 780).

```text
textbox_rect = 360, 700, 1200, 340      # x, y, width, height; inside the screen, at least 400x120
textbox_padding = 40, 36, 40, 20        # text inset: left, top, right, bottom (or one number for all four)
name_plate_image = assets/gui/plate.png # stretched over the name plate, under the speaker's name
name_plate_rect = 380, 640, 400, 64     # x, y, width, height of the name plate
text_align = center                     # the dialogue text: left (default) or center
name_align = center                     # the speaker's name: left (default) or center
```

- The padding must leave at least 200x60 px for the text inside the textbox, otherwise it
  is reported and the default is used. The top inset is for a line without a speaker; with
  one, the text starts 10 px below the name plate when the plate is inside the box.
- Without `name_plate_rect` the plate is a 48 px strip at the top of the textbox, 22 px down and
  inset by the padding's left and right. The rect is absolute, so it can sit above the box.
- Choice buttons stack centred above the top edge of the textbox (and shrink if there is little
  room, so leave some above it), and inline pictures are drawn above it too.
- `quillstone check` warns about lines too long for the real textbox and padding, and checks
  `name_plate_image` and `textbox_image` as pictures; `package` includes them.
- The history screen is not affected.

**The quick menu** (Back, History, Skip, Auto, Save, Load, Q.Save, Q.Load, Settings, and Items
in a game with an inventory) sits in a row directly below the textbox, aligned with its right
edge. When there is no room below (the textbox ends less than 36 px above the bottom of the screen)
the row is drawn just inside the bottom edge of the textbox instead; a textbox narrower than the
row moves the row right so that every button stays on the screen. `quick_menu` says when it shows:

| `quick_menu` | |
| --- | --- |
| `visible` | always (default) |
| `hover` | only while the cursor is over the textbox or the row, fading in and out in about 0.15 s; buttons that aren't shown can't be clicked |
| `hidden` | never drawn or clickable |

The keyboard and gamepad shortcuts work in every mode. Players can override the mode in
Settings (Quick menu: Game default, Always, On hover, Hidden).

### The inventory and shop screens

One grid layout serves both screens, so they share the `inventory_` keys. Without
any of them the screens look like the other built-in ones. Positions and sizes are in
the 1920x1080 virtual screen.

```text
inventory_background = assets/gui/bag.png   # full-screen picture (else the screen is dimmed)
shop_background = assets/gui/stall.png      # the shop's own background, if different
inventory_panel = assets/gui/panel.png      # stretched over the panel, under the grid
inventory_panel_rect = 160, 70, 1600, 940   # x, y, width, height of the panel (default 210, 90, 1500, 900)
inventory_columns = 4                       # cells per row (1 to 12; fewer if they don't fit)
inventory_cell_size = 270, 210              # width, height (or one number for a square); width 48 to 600, height 120 to 600
inventory_cell_gap = 24
inventory_icon_size = 104                   # the icon is drawn in a square this big
inventory_cell = #4a3020                    # a cell: a colour #rrggbb[aa] ...
inventory_cell_hover = assets/gui/hover.png # ... or a picture, stretched over the cell
inventory_cell_owned = #2f4a2f              # an owned item in a shop
inventory_cell_disabled = #2a221cb0         # an item the player can't afford
inventory_title = Your pack                 # default "Items"
shop_title = Night market                   # default "Shop"
shop_currency = Gold                        # what the shop calls its money (default: the pay variable's name)
inventory_description_rect = 300, 760, 1300, 170   # where the hovered item is described
inventory_name_color = #f6e7cf              # item names (default text_color)
inventory_price_color = #f2b35c             # prices and the money line (default accent_color)
inventory_text_color = #e6d3b3              # descriptions (default text_color)
```

The panel rect holds the title, the grid, the description box and the buttons, laid
out from its top left, so a bigger panel or cells that don't fit it give fewer rows
and more pages rather than overflowing. `quillstone check` validates the pictures
and the colours; a line that can't be read is reported with its line number and
ignored.

## Not (yet) in the language

Pasting into a text input, multiple script files / `::include`,
character voices per speaker, and transitions other than fades and wipes.
The runtime is built so these slot in without changing what is written above.
