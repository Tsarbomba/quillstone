# Quillstone script reference

A script is a markdown file (use the extension `.quill.md`). It is read top to
bottom; the player advances through it one *waiting* step at a time (a line, a
wait, a video, a choice). Everything else runs instantly in between.

`quillstone check script.quill.md` validates a script and that its files exist;
`quillstone test script.quill.md` plays every branch and reports story bugs (loops
with no way out, unreachable parts, options never offered, misspelt variables);
`quillstone run script.quill.md` plays it in the terminal; `quillstone dump` shows
the compiled form. Errors name the file and line.
`quillstone graph script.quill.md > story.md` writes the story's structure as a flowchart:
a box per heading (as you wrote it, grouped per `::include`d file), an arrow for running on
into the next heading, `::jump` (thick), `::call` (dashed), each choice or hotspot (labelled
with its text) and the inventory's Use. A condition shows as `if ...`, an ending has rounded
ends and a heading nothing leads to is dashed red. The output is Mermaid, which GitHub and VS
Code draw as a picture; `--format dot` gives Graphviz and `-o file` writes a file. It reads
the script, it doesn't play it: for what can actually happen, use `test`.
`check` also warns (never an error) about pictures, audio, video and fonts in the game
folder that nothing uses, the files `package` would leave out: it lists the first 20.
It stays quiet when other scripts sit beside this one (one assets folder shared by several games).

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
::show{mika neutral x=80% fade=0.4}

**Mika:** Hello, {name}.
She looks away.
````

- **Front matter** (optional, between `---` lines): `title:` (also shown on the
  main menu), `id:` (the name of the folder the player keeps saves in, e.g. `id: my-story`: 1 to 64 lowercase
  letters, digits and `-`; set it once and the saves survive a new title; without it the folder is named after the
  title, or the script file), `theme:` (a theme file, see [Themes](#themes)), `vars:`,
  `inventory:` (see [Inventory](#inventory)), `journal:` and `journal_done:` (see [Journal](#journal)), `languages:` (see [Translating a game](#translating-a-game)), and `persistent:`, `gallery:`,
  `music_room:` and `replays:` (see [Kept across playthroughs](#kept-across-playthroughs)
  and [Scene replays](#scene-replays)). `sprite_size:` and `art_scale:` set how big sprites are drawn (see
  [Sprites at their picture's size](#sprites-at-their-pictures-size)).
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
  (otherwise its saves could never be loaded again). Braces can't be part of a file name
  (see [Picture names from variables](#picture-names-from-variables) for the one place a `{variable}` may stand in a name).

## Splitting a story over several files

````markdown
::include{chapters/ch1.quill.md}
::include{src=chapters/ch2.quill.md}
````

`::include` on a line of its own reads another script file at that spot, as if its lines
were written there: its labels, characters, items and animations are part of the one story,
and a `::jump` may go from any file to a label in any other. The inclusion happens when the
game is compiled, so saves, rollback and the tester see one story.

- The path is relative to the **main script's folder** (the game folder), like every other
  path (assets keep that same base inside an included file: `assets/x.png`, not a path
  relative to the included file), and must be inside it. The file name ends in `.quill.md`
  (or `.md`). It can't hold a `{variable}`.
- The **main script** keeps the front matter: `title`, `vars`, `theme`, `gallery`, `replays`,
  `persistent` and the rest. An included file has none, and starting with `---` is an error.
- Labels are global: the same label in two files is an error that names both places.
- Includes may nest up to 8 files deep. A file can be included once: including it a second
  time, or a cycle (`a` includes `b` includes `a`, or a file including the main script), is
  an error. A game may include up to 200 files and 8 MB of script in all.
- A `::choice`, `::hotspots` or `::shop` block, a comment and a front matter block each
  stay inside one file; an `::if` block may open in one file and close in another.
- Errors, warnings and `quillstone test` findings name the file and line, such as
  `chapters/ch1.quill.md:12`. In `quillstone dump` the line column shows the file as well.
- A script handed to the compiler as plain text, with no folder to read from, can't use it.
- Hot reload watches every included file: saving one updates the running game. `quillstone
  package` copies every included file into the game's `game` folder at the same path (put
  included scripts in a folder such as `chapters/`, or name the main script `main.quill.md`,
  so the player can tell which file starts the game).

See `examples/chapters.quill.md`.

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
| `::bg{color=#102030 fade=0.5}` | A **solid colour** instead of a picture, exactly one of `src=` and `color=` (`#rrggbb` or `#rgb`). `fade`, `wipe` and `through` work as for a picture, and a colour crossfades to or from a picture or another colour. It needs no file. Use it where you would otherwise show a plain black picture: `::bg{color=#000000}`. Rollback, saves and hot reload keep it. |
| `::bg{anim=rain fade=1}` | An **animation** (see "Frame animations" below) as the background, instead of `src=`/`color=`: exactly one of the three. `fade`, `wipe` and `through` work as for a picture. |
| `::bg{src=... fade=1 through=#000000}` | Change the background **through a colour**: the stage fades to the colour over half of `fade`, the picture is swapped (no crossfade) and the colour fades away over the other half. It waits for the first half like `::fade{out}` does. `through` on its own is black; `through` needs a `fade` above 0 and can't be combined with `wipe`. Sprites are not touched: use the explicit `::fade` pair below when sprites change too. Use it on an uncovered screen: it ends uncovered. |
| `::bg{src=... fade=1 wipe=right}` | Wipe instead of crossfading: the new picture slides in from one side over `fade` seconds. `wipe`: `left`, `right`, `up`, `down` (the direction the edge travels). |
| `::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}` | Declare a character: name-plate text and colour, and a sprite path where `{pose}` is filled in. Declare before use. `**Mika:**` lines pick up the name and colour automatically. |
| `::character{id=hero name="{hero.name}" color=#9cc8ff}` | The name may hold `{variable}`s of declared variables (an unknown one is an error). The name plate, History and the text-mode output show the variable's value at that line, so a character the player named (see `::input`) is shown by the typed name, and earlier lines keep the name they had after a rollback or a variable change. What the player typed never becomes formatting. Write `**hero:**` or the name text exactly as declared, `**{hero.name}:**`, before a line. In a translation file the `@ Speaker` context for such a character is the name as written in the script (`@ {hero.name}`), and the name's own entry is `> {hero.name}`. Auto-voice file names do not depend on the variable. |
| `::character{id=mika name=Mika layers=body,eyes,mouth layer_sprites=assets/mika/{layer}_{value}.png}` | A character drawn from **layers** (see "Layered sprites" below): `layers` lists the layer ids back to front (at most 8), `layer_sprites` is where each layer's pictures are (`{layer}` is the layer id, `{value}` what is shown on it). A character may have `sprites=` poses, layers, or both. |
| `::show{mika body=casual eyes=open mouth=smile x=20%}` | Show or change a **layered** sprite: each layer id is an attribute whose value says what that layer shows (`eyes=none` hides it, `eyes=anim:blink` uses an animation). See "Layered sprites" below. |
| `::show{mika happy x=20% fade=0.4 scale=1}` | Show a sprite (or change its pose; the new pose crossfades and keeps its position and size). `x`: `left` `center` `right`, a number or `30%` (see "Placing things: `x` and `y`" below). Without `x` it keeps its position, or is centred if new; without `scale (0.05 to 10) it keeps its size, or is 1 if new. |
| `::show{mika happy y=0.5 z=2}` | `y`: `top` `center` `bottom`, a number or `N%`, placed like `x` (a new sprite stands on the bottom, `y=bottom`). The screen cuts off what hangs outside. `z`: a whole number from -100 to 100 (default 0); sprites with a higher `z` are drawn in front, and equal ones in the order they were shown. `z` only orders sprites among themselves: the background stays behind them and the textbox and menus stay on top. Showing a sprite again keeps its `y` and `z` unless you give them. |
| `::show{mika happy flip xscale=1.5 yscale=0.8}` | `flip` mirrors the sprite left to right (`flip=false` turns it back). `xscale` and `yscale` (each 0.05 to 10, default 1) stretch it along one axis, on top of `scale`; it keeps its `x` and `y` placement, worked out again for its new size. Showing a sprite again keeps its flip, `xscale` and `yscale` unless you give them. |
| `::show{src=assets/ghost.png id=ghost x=center}` | A sprite that isn't a character. |
| `::show{mika anim=blink x=20%}` | A sprite whose picture is an **animation** (see "Frame animations" below). `mika` is the sprite's name (a character's name finds that character's sprite); the character's `sprites=` pattern and poses aren't used, so `anim=` can't be combined with a pose or with `src=`. Everything else (`x`, `scale`, `flip`, `y`, `z`, `fade`, `::move`, `::hide`) works as for any sprite. |
| `::hide{mika fade=0.5}` / `::hide{all}` | Remove one sprite or all. |
| `::move{mika x=80% time=0.8}` | Slide a sprite (it animates `x` and `y`). |
| `::move{mika scale=1.2 time=0.5 ease=out}` | Grow or shrink it (can be combined with `x` and `y`). `ease`: `inout` (default: slow at both ends), `in`, `out`, `linear`. |
| `::move{mika y=0.9 time=0.3 ease=out}` | Slide it up or down the same way (`y` as for `::show`). `z` can't be animated; change it with `::show`. |
| `::move{mika xscale=1.4 yscale=0.7 time=0.5}` | Stretch it along one axis (as for `::show`); combines with `x`, `y` and `scale`. `flip` (or `flip=false`) can be added to a move too, but a mirror can't be animated: it switches at the **start** of the move. |
| `::move{mika y=0.85 time=1.2 ease=inout loop}` | A **looping move**: the sprite goes to the target, back to where it started, and forth again, until something else moves it. See "Looping moves" below. |
| `::camera{zoom=1.3 x=0.3 y=0.4 time=4}` | Pan and zoom the stage (background and sprites; the textbox and menus stay put). `zoom` 1 to 4; `x`, `y` place the view over the stage the same way as a sprite (`left` `center` `right`, `top` `center` `bottom`, a number or `N%`, but only 0 to 100%: the view never leaves the stage). Anything left out stays as it is; `ease=` as for `::move`. Doesn't wait. |
| `::camera{reset time=1}` | Back to the whole stage. |
| `::shake{time=0.4 strength=20}` | Shake the stage, dying away over `time` seconds (`strength` in pixels at 1920x1080, up to 200). Doesn't wait. While it lasts the stage is zoomed in slightly about the centre (by just enough to hide the edges), easing in and out with the shake. |
| `::shake{axis=x}` / `::shake{axis=y}` | Shake along one axis only: `x` sideways, `y` up and down, `both` (the default) both ways. A one-axis shake moves the stage by up to `strength` pixels along its axis and not at all along the other, and zooms in only as far as that axis needs. |
| `::flash{time=0.4 color=#ffffff}` | A flash over the whole screen: the colour jumps to full strength and fades out over `time` seconds (more than 0, up to 10; default 0.4). `color` is `#rrggbb` or `#rgb` (default white). Doesn't wait. It is drawn over the stage and inline images but under the textbox and menus. |
| `::notify{text="Saved" time=3}` | A small message in the top right corner, on the theme's panel colour and text colour, for `time` seconds (0.5 to 30; default 3): it fades in, stays, and fades out. `text` can hold `{variable}` values and the same markup as a `::hud` text (`**bold**`, `{color=...}`) (filled in when the command runs). Doesn't wait and doesn't stop the story; a new `::notify` replaces one still showing. It is a passing message: not saved, not shown again by rollback, and it ends at once when the player skips. Its `text` is translated like a dialogue line (looked up before the `{variable}` values go in). |
| `::unlock{src=assets/cg1.png}` | Unlocks a `gallery:` or `music_room:` entry for the Extras screen without showing or playing it; see [Kept across playthroughs](#kept-across-playthroughs). Doesn't wait or show anything. |
| `::fade{out time=0.5 color=#000000}` | **Dip to a colour**: the whole stage (background, sprites, inline images, video and camera) fades to the colour over `time` seconds (more than 0, up to 10; default 0.5; `color` as for `::flash`, default black) and **stays covered**. It waits until the fade is done, like `::wait`; a click, or skip mode, ends it at once and the story goes on. The textbox, choices and menus are not covered, so a line of narration works over a black screen. |
| `::fade{in time=0.5}` | Fade the cover away over `time` seconds. Doesn't wait. With nothing covering the screen it does nothing. |
| `::particles{snow}` / `{rain}` / `{src=assets/petal.png}` | Ambient weather over the picture: snow, rain, or any small picture of yours falling and drifting. `{none}` (or `{off}`) stops it. Settings: `amount=` (how many are on screen, 1 to 200, default 60), `speed=` (0.2 to 5, default 1), `wind=` (-2 to 2, default 0; negative blows left) and `fade=` (seconds to ease in or out, 0 to 10, default 1), and `layer=back` to put it behind the sprites. Doesn't wait. See [Ambient particles](#ambient-particles). |
| `::tint{color=#203060 alpha=0.35 fade=1}` | A colour wash over the whole stage that **stays** until it is changed or removed: `::tint{none}` (or `{off}`) takes it away. `color` is `#rrggbb` or `#rgb` (required); `alpha` is how strong it is, 0 to 0.9 (default 0.4; it never goes higher so the story stays visible; `35%` also works); `fade` is the seconds to ease from the current tint (or from none), 0 to 10 (default 0: at once). `{none}` takes only `fade`. Doesn't wait. See "Colour wash". |
| `::textbox{hide}` / `::textbox{show fade=0.5}` | Hide the whole dialogue box (textbox, name plate, line and quick menu) for a clean picture, or show it again. `fade` is a number of seconds above 0, up to 10 (default: at once). Doesn't wait. A hidden box comes back by itself with the next line or choice, so the story never moves on unseen; clicks still advance while it is hidden. On a `::hotspots` screen it stays hidden, and the line that is pending isn't drawn either, which gives a clean map: show it again with `::textbox{show}` or just say a line. Rollback, saves and hot reload keep it. |
| `![alt](assets/x.png)` | An inline image. It appears **with the next line that waits** and disappears when the player advances, so put it *before* the line it illustrates. (`![](x.webm)` plays a video.) |

### Placing things: `x` and `y`

One model places sprites (`::show`, `::move`), HUD elements (`::hud`) and the camera (`::camera`). `x` and
`y` each take a **keyword**, a **number** or a **percentage**:

- `x`: `left` (0%), `center` (50%) or `right` (100%). `y`: `top` (0%), `center` (50%) or `bottom` (100%).
- A plain number is a fraction, not pixels: `0.25` is the same as `25%`, as everywhere else in the language.
- The point `N%` of the way **across the object** is put `N%` of the way across the **screen**. So `x=0` is flush
  with the left edge, `x=right` flush with the right edge, `x=25%` a quarter of the way along the free room.
  Between 0% and 100% the object stays wholly on screen whatever its size (the HUD and the camera are held to
  that range). Outside it, a sprite moves by whole screens: `x=-100%` puts its left edge one screen width left of the
  screen's, so a sprite no wider than the screen is wholly off to the left (and `x=200%` wholly off to the right),
  which is how you slide one in or out (sprites may go from -200% to 300%; `y` works the same way).
- A new sprite is placed `x=center y=bottom`. A sprite you show or move again keeps what you do not give.
- There is no anchor: this model cannot say "the centre of this sprite exactly at that spot", because where
  the centre ends up depends on the picture's size. A sprite as tall as the screen has no free room, so `y`
  does nothing to it until you give it a smaller `scale`.

Sprites are about 940 px tall at `scale=1` (on a 1920x1080 screen) and stand on the bottom edge unless `y` says otherwise; the text box draws over their
lower part (to size them by their pictures instead, see the next section). To put one sprite behind another, give it a lower `z`. The camera, sprite sizes,
heights, stretch, mirroring, `z` and the colour cover are part of the scene, so rollback
and saves restore them (a save made before `::fade` existed loads uncovered, one made
before `flip` and the scales existed loads with unflipped, unstretched sprites); a shake, a flash or a `::notify` is a moment and
isn't replayed. A flash stands still while a menu is open and ends at once when the
player skips.

The cover is drawn over the stage, inline images, video and hotspot pictures, under a
`::flash` and under the HUD elements, the textbox and everything else on the interface. Rollback goes
past the wait of a `::fade{out}` (like a `::wait`), so going back never stops halfway
through a fade, and roll forward does the same. The last line stays on screen while the
screen fades out; hide the box first (`::textbox{hide}`) for a clean fade.

### Sprites at their picture's size

By default every sprite is 940 px tall whatever its picture's size. For art drawn for one screen size, say so in
the front matter:

```
sprite_size: pixels
art_scale: 1.5
```

- `sprite_size:` is `fixed` (the default, the 940 px above) or `pixels`: a sprite is as big as its picture, so
  `scale`, `xscale` and `yscale` multiply the picture's own width and height.
- `art_scale:` (0.25 to 8, default 1) scales every picture first, for art drawn for a screen other than 1920x1080:
  a 1280x720 picture with `art_scale: 1.5` shown at the default `x=center y=bottom` exactly fills the screen.
  It needs `sprite_size: pixels` (alone it is an error), and it applies to layered sprites and `::anim` frames too.
  Backgrounds, inline images and the HUD are not affected.

## Ambient particles

`::particles` is cheap weather: no spawning, no physics, never more than 200 particles.

```
::particles{snow}
::particles{rain amount=120 wind=0.6}
::particles{src=assets/petal.png amount=24 speed=0.9 wind=0.3}
::particles{snow layer=back}
::particles{none fade=2}
```

- **Snow** is small soft white squares of a few pixels, with different sizes (the bigger ones are
  nearer: they fall faster and are brighter) that sway as they fall. **Rain** is thin, faint
  streaks that fall fast and lean with the wind. A **picture** (`src=`, a .png, .jpg or .webp inside
  the game folder; no `{variable}` in the name) falls and drifts like snow, at its own pixel size
  scaled a little for each particle and without turning. Keep it small: 32 px or so is plenty,
  and `quillstone check` warns about one over 256 px.
- Using `::particles` again changes the settings; going from one kind to another crossfades,
  and a change of `speed` or `wind` is smooth. `fade=` is how long it takes to ease in (or, with
  `none`, out); `fade=0` is at once.
- They are drawn over the background and the sprites and under the inline images, video, `::fade`
  cover, `::flash`, the HUD, the textbox and every menu. With `layer=back` they go between the
  background and the sprites instead.
- They belong to the screen, not the stage: the camera and `::shake` don't move them.
- They animate with the same clock as other animations: they stand still while a menu or
  Settings is open and don't run ahead in skip mode.
- Where each particle is isn't saved, only the settings, so rollback, saves and replays bring the
  weather back (starting from a fresh sky). A save from before `::particles` loads without any.

## Colour wash

`::tint` lays a translucent colour over the whole picture and keeps it there, for night, dusk, a
sick green or a sepia memory.

```
::tint{color=#203060 alpha=0.35 fade=1}
::tint{color=#8a6a3a alpha=0.5}
::tint{none fade=1}
```

- It is one flat colour at `alpha` opacity, so it is **not** a filter: a grey or sepia tint gives
  a flashback look, but the picture is not truly drained of colour, and bright parts stay bright.
  There are no blend modes.
- A new `::tint` eases from the one on screen (colour and strength), or from nothing, over
  `fade`. `::tint{none fade=1}` eases the wash away. Removing a tint that isn't there does nothing.
- It is drawn over the background, sprites, particles, inline images and video, and under the
  `::fade` cover, `::flash`, the HUD, the textbox, choices and every menu, so the text is never
  tinted. It belongs to the screen: the camera and `::shake` don't move it.
- The colour and strength are saved, so rollback, saves and replays bring the tint back at once
  (without its fade). A save from before `::tint` loads without one. `{variable}` isn't supported in it.
- `quillstone run` prints `[tint: #203060]` and `[tint off]`.

## Picture names from variables

A picture name may hold `{variable}` parts, so a variable can choose the picture: a
`place` variable picks the background, a `mood` variable picks a pose.

```
vars: { place: "rooftop", mood: "happy" }
...
::bg{src="assets/places/{place}.png"}
::show{mika {mood}}
![](assets/items/{item}.png)
```

**Where it works:** the pose of `::show` (`::show{mika {mood}}`, also joined to other
letters, like `a_{mood}`: it goes through the character's `sprites=` pattern exactly like
a written pose), `src=` of `::show` (which then needs a sprite name, `::show{mika
src="x/{mood}.png"}`), `src=` of `::bg`, inline images, and the value of a layer of a
layered sprite (`::show{mika eyes={mood}}`, see "Layered sprites"). Sounds, voices, videos, HUD
pictures, item icons, animation frames and the gallery take plain file names only (a brace
there is an error), and `anim=` and `color=` take no variables.

**The variable** is a text variable; dotted names work (`{hero.mood}`). Only a whole
`{name}` is replaced: no spaces inside the braces, no calculations. A variable that is
not declared in `vars:` is allowed (a `::set` may create it, and `quillstone test` warns
when nothing ever sets it), but a declared number, `true`/`false` or list is an error. The
name needs a written-out extension (`.png`, `.jpg`, `.jpeg` or `.webp`; nothing else), at most four variables
fit in one name, and a name with a `{variable}` can only be a picture, not a video.
Braces cannot be part of any file name: there is no way to write a literal one.

**What a value may be.** The text is put into a file name, and players can type text
(`::input`) or share saves, so only a safe fragment is used: 1 to 64 characters, made of
letters (any alphabet), digits, `_`, `-`, spaces and dots, where a dot or a space is never
the first or last character and `..` never appears. Anything else (a `/`, `\`, `:`, a control
character, an empty text, a longer one, or a name Windows keeps for devices such as `con`
or `nul`) does not make a path at all: the picture is treated as a file that is not there.
It draws nothing, and the game shows a short notice once saying which variable and picture name
were involved (it never prints the value). A variable that holds a number or is unset
counts the same way. Nothing outside the game folder can ever be reached this way.

**When the name is built.** At the moment the command runs, from the variables as they
are then. The game, its saves and rollback keep the finished file name, so a save
loads as before and going back shows the picture that was shown.

**`check`, `package` and loading.** A name with `{variable}` stands for every file it
could build, so it is read as a pattern where each `{variable}` stands for one safe
fragment inside its own folder (never across `/`). `quillstone check` reports an error
when a pattern matches no file (`assets/places/{place}.png matches no file`),
checks that every match is a picture the player can show, and warns when a pattern
matches more than 200 files. `quillstone package` ships **every** file a pattern matches,
so keep unrelated files out of the folder, or name them so the pattern cannot match them
(`places/rooftop.png` is matched by `assets/places/{place}.png`; `assets/places/old/x.png`
and `assets/places/notes.txt` are not). The game cannot know the value ahead of time, so
it loads these pictures when they are first shown instead of in advance; a short pause the
first time a picture appears is possible with very large files.

## Frame animations

A frame animation is a list of pictures with the time each one stays up, like an eye
that is open for three seconds and closed for a moment. Declare it once, before it is
used, then use it wherever a picture goes:

```
::anim{id=blink frames="eyes_open.png 3, eyes_closed.png 0.12" loop}
::anim{id=rain frames="rain1.png 0.1, rain2.png 0.1, rain3.png 0.1" loop}
::anim{id=arrive frames="door_shut.png 0.6, door_open.png 0.1"}

::bg{anim=rain fade=1}
::show{mika anim=blink x=20%}
```

- `id` is letters, digits and `_`; declaring an id twice is an error. Animations are
  global, like characters, and can be shown more than once at the same time.
- `frames` is a comma-separated list of `picture seconds` pairs, in quotes because of
  the spaces. At least one frame; each picture is a `.png`, `.jpg`, `.jpeg` or `.webp`
  inside the game folder (so file names have no spaces), and each time is from 0.02 to
  60 seconds. A frame is shown from its start up to, but not including, the next frame's start.
- `loop` (on its own) starts over from the first frame after the last one. Without it
  the animation plays once and **stays on its last frame**.
- A one-frame animation is allowed: it is just a picture.
- `anim=` goes on `::show` (a sprite) and `::bg`. Anywhere else it is an error, and so
  is an animation that is not declared above the line that uses it.

An animation sprite is an ordinary sprite: it can be moved, scaled, flipped, faded, put
behind another, given a `y`, hidden, rolled back and saved. Showing another picture on
the same sprite (`::show{mika happy}`) replaces the animation with a crossfade, and
the old animation keeps playing while it fades out. Frames may differ in size; each
is drawn as a sprite (or background) picture of its own size.

**Timing.** The animation starts at its first frame when it starts being shown. Time
counts in seconds of the story's own clock, the same as fades and waits: it stands still
under Settings, History, the save screens and dialogs, and skip mode does not speed it
up (it runs at normal speed while lines go by). Where an animation is in its run is not
part of a save or of rollback: after loading a save, rolling back or reloading the
script it starts again from its first frame. Saves remember *which* animation each
sprite or background shows (a save that names an animation the script does not declare
is refused, like one naming a file outside the game folder; a hot reload instead drops
the picture of an animation you removed: a background goes, a sprite leaves the stage).

**Cost.** Every frame is loaded as an ordinary picture, all of them together, a few
lines before the animation can appear (and as long as it is on screen), so a short
loop of well-sized pictures is best. `quillstone check` reports a frame that is missing
or is not a picture as an error, and warns about an animation of more than 64 frames or a
frame bigger than 2048 pixels on a side. `quillstone package` includes every frame.
Frames of an animation are not crossfaded into each other.

## Layered sprites

A character can be drawn from several pictures stacked on top of each other, such as a body,
a pair of eyes and a mouth, so that an expression or an outfit changes without a picture for
every combination. Declare the layers on the character, back to front:

```
::anim{id=blink frames="assets/mika/eyes_open.png 3, assets/mika/eyes_closed.png 0.12" loop}
::character{id=mika name=Mika layers=body,eyes,mouth layer_sprites=assets/mika/{layer}_{value}.png}

::show{mika body=casual eyes=open mouth=smile x=20% fade=0.4}
::show{mika mouth=open}
::show{mika eyes=anim:blink body=formal}
::show{mika eyes=none}
```

- `layers` is a comma-separated list of layer ids (letters, digits and `_`, at most 8, no
  repeats; not `none`, and not the name of a `::show` attribute such as `x` or `scale`).
  `layer_sprites` must contain both `{layer}` and `{value}` and end in a picture type
  (`.png`, `.jpg`, `.jpeg` or `.webp`). With `layer_sprites=assets/mika/{layer}_{value}.png`,
  `eyes=open` shows `assets/mika/eyes_open.png`.
- A `::show` names the layers it changes. **A layer you leave out keeps what it showed**, and so
  do the sprite's position, size and everything else. The first `::show` of a character with
  layers must give a value for every layer (the script is refused otherwise, and the error lists
  the missing ones; use `none` for a layer that starts empty). A bare `::show{mika x=80%}` on a
  character that has only layers just moves the sprite.
- `eyes=none` hides that layer. `eyes=anim:blink` shows the frame animation `blink` (declared
  above, see "Frame animations") on that layer, so blinking is just an animated eyes layer; its
  clock runs on while other layers change.
- A value may hold `{variable}` parts like a pose does (`mouth={feeling}`, `body=outfit_{day}`);
  the same rules apply as in "Picture names from variables": the variable must be a text variable, and
  a value that is not a safe file-name fragment shows nothing and gives a notice.
- All the layers of a character should be the **same size** (the same canvas): they are drawn
  stacked, each at the same place, size and mirroring, so one layer is not placed on its own.
  `quillstone check` warns about a layer picture whose size differs from the first.
- Everything that works on a sprite works on the whole stack: `x`, `y`, `z`, `scale`, `flip`,
  `xscale`, `yscale`, `fade`, `::move`, `::move{... loop}`, `::hide`, the camera and shake.
- **Changing a layer crossfades that layer only**, over the `fade` of the `::show` (none by
  default); the other layers do not flicker. A sprite's own `fade` fades the whole stack in or out.
  The layers are fading as separate pictures, so a sprite that is halfway through a fade is a little
  more see-through where layers overlap than a single picture would be.
- **Poses and layers.** A character with only layers can't be shown with a pose
  (`::show{mika happy}` is an error). A character with both `sprites=` and layers can be shown either
  way: a pose replaces the whole sprite (and drops its layers); layers on a sprite that was shown
  as a pose replace it, so all of them need a value. When a sprite is hidden and shown again, the
  layers start over: one with no value stays hidden and the game shows a notice
  (`quillstone test` reports it).
- Layers are part of the scene: rollback, saves and hot reload restore them. A save names each
  layer's value, and is checked like a sprite's picture (a path outside the game folder, an
  unknown animation or a layer the character does not declare is refused; after a script
  edit that removed a layer or an animation, the load drops it). Saves from before layers existed
  load as they were.

`quillstone check` looks at every layer picture the script can show, including every file a
`{variable}` value can build, and `quillstone package` ships them all. A demo is in
`examples/layers.quill.md`.

## Looping moves

Add `loop` to a `::move` and the sprite keeps going back and forth:

```
::move{mika y=0.85 time=1.2 ease=inout loop}
::move{mika scale=1.04 time=2 loop=pingpong}
```

- The sprite glides to the target as for any move, then back to where the move started,
  then to the target again, for ever. `time` is the time of **one leg** (it must be above
  0), and `ease` applies to each leg.
- `loop` alone and `loop=pingpong` mean the same; there is no other kind of loop.
- It animates any mix of `x`, `y`, `scale`, `xscale` and `yscale`; a move that changes
  none of them (only `flip`, say) can't loop. `flip` may be added: it switches at the start.
- "Where it started" is the sprite's resting values: what it had when the move was given.
  The loop doesn't change them, so a pose change (`::show{mika happy}`) keeps the loop going.
- The loop ends when the sprite gets another `::move`, a `::show` that **sets** `x`, `y`,
  `scale`, `xscale` or `yscale` (whether or not the value differs), or `::hide`. A plain
  `::move` (without `loop`) glides from wherever the sprite is at that moment to its new
  values; anything it doesn't name goes back to the resting value. A second `::move{... loop}`
  replaces the loop.
- It never holds up the story: the next line shows at once, rollback goes back one line
  as always, and the last line of the story ends with the sprite still bobbing.

Like animations, where a loop is in its cycle is not part of the state: a save, rollback or
reload restarts the sprite at its resting values and heading for the target. A loop stands still while a menu is open,
and skip mode does not speed it up.

## HUD: text, bars and pictures over the story

`::hud{...}` puts an always-on element over the story: a counter, a clock, a health bar, a
title card. An element has an `id` (letters, digits and underscores); using the same `id` again
**replaces** the element where it stands. Which kind it is depends on which attribute is there,
and exactly one of these is required:

| Attribute | Kind |
| --- | --- |
| `text="Day {day}, {hero.gold} gold"` | **Text**. The same tags and `{variable}` values as dialogue (`**bold**`, `*italic*`, `{color=..}`, `{size=..}`), but not `{nw}` or `{p}`. The values are read **every time the screen is drawn**, so the text follows the variables by itself. |
| `bar=hero.hp max=hero.max_hp size=320,26` | **Bar**. `bar` and `max` are whole-number expressions (a variable, a number, or arithmetic like `hero.max_hp + 2`). It is filled `bar / max` of the way from the left, kept between empty and full; with `max` 0 or less it is empty. `size` is width,height in pixels of the 1920x1080 screen. |
| `image=assets/heart.png scale=0.5` | **Picture** (`.png`, `.jpg`, `.jpeg` or `.webp` inside the game folder, like a hotspot image), drawn at its own pixel size times `scale` (0.05 to 10, default 1). `quillstone check` tests it as a picture and `package` includes it. |

Attributes for every kind:

| Attribute | |
| --- | --- |
| `x=right` `y=4%` | Where the element is, placed as a sprite is (see "Placing things: `x` and `y`"): `x` is `left` `center` `right` or a number or percentage from 0 to 100%, `y` is `top` `center` `bottom` or the same; the point that far across the element is that far across the screen, so the element is always wholly on screen. Default `left` and `top`. A text element's width is estimated, so in between the three keywords it can be a few pixels off. |
| `if="hero.hp <= 3"` | Draw the element only while the condition is true; it is checked every time the screen is drawn. `random` isn't allowed, as in any condition. |
| `fade=0.3` | Fade in over that many seconds (more than 0, up to 10) when the element is created. |
| `call=` `open=` `hover=` | Make a text or picture element a button: see "Buttons" below. |

Attributes of a **text** element: `size` (8 to 300; default the theme's text size), `color`
(default the theme's text colour), `back` (a panel behind the text, e.g. `back=#00000099`; colours
here may have an alpha part, `#rrggbbaa` or `#rgba`) and `pad` (the padding around the text in pixels, 0
to 200; default 0). Without `back` the text is aligned to the side its `x` leans to (left, centre or right) inside that estimated box, so at `left`, `center` and `right` it is placed exactly. With `back` the panel is as
wide as the text is *expected* to be, a little generously, so it may be a few pixels wider than the text.

Attributes of a **bar**: `color` (the fill; default the theme's accent colour), `back` (the empty
part; default `#333333`), `border` (an outline colour; drawn outside the bar's size) and `border_size`
(1 to 3 pixels, default 2).

```
::hud{id=hp bar=hero.hp max=hero.max_hp x=0.02 y=0.04 size=320,26 color=#cc3333 back=#333333 border=#ffffff}
::hud{id=clock text="Day {day}, {hero.gold} gold" x=0.98 y=0.04 size=32 back=#00000099 pad=12 fade=0.3}
::hud{id=low text="LOW HP" x=0.02 y=0.11 color=#ff5555 if="hero.hp <= 3"}
::hud{id=heart image=assets/heart.png x=0.02 y=0.9 scale=0.5}
```

**Buttons.** A text or picture element (not a bar) becomes a button with one of these:

| Attribute | |
| --- | --- |
| `call=#phone` | A press **calls** that label and, when it ends with `::return`, shows the same line again (like an item's Use). The label must exist and take no parameters; `check` says which line is wrong. |
| `open=journal` | A press opens a built-in screen instead, as the quick menu and keys do: `journal` (J), `inventory` (I), `history` (H), `save` (S), `load` (L) or `settings` (the quick menu's Settings button). `journal` and `inventory` need a game that has one. |
| `hover=assets/phone_hover.png` | For a picture button: shown instead of the picture while the mouse is over it. It needs `call=` or `open=` and is checked and packaged like the picture. A text button simply lights up under the mouse. |

`call=` and `open=` can't be combined. Buttons work **only while the story waits at a normal line**
(the same rule as an item's Use). Over a choice, a timed choice, `::input`, `::wait`, a shop, a `{nw}`
line, a replay or while skipping they are drawn dimmed and a press only says "Can't use that now". A
click on a button never also moves the story on; a choice or hotspot under the same spot wins over the
button, and so does the quick menu. Buttons are **mouse only**: the keyboard and a gamepad already have
their own keys for the screens, and a `call=` button gets no focus of its own. The click area is the
rectangle the element is drawn at (for text its estimated box), so keep a button clear of the textbox.
You show, hide and change a button with `::hud` like any element, and it follows the scene through
rollback, saves and hot reload; a save can only name a label some `::hud` of the script offers.
`quillstone test` presses each `call=` button once, where it can be pressed, so its label is not
reported as unreached, and `quillstone graph` draws a dotted edge from a "HUD" node to each such label.

```
::hud{id=phone image=assets/phone.png hover=assets/phone_hover.png call=#phone x=right y=0.12}
::hud{id=log text="Journal" open=journal x=0.02 y=0.12 back=#00000099 pad=10}

# phone
::set{calls += 1}
Mika: *rrring* Hello? ... Oh, it's you.
::return
```

`::hud{hide=clock fade=0.3}` removes one element (with an optional fade out); hiding an id that
isn't there does nothing. `::hud{clear}` removes all of them (`fade=` works too). Neither waits.

**Where they are drawn.** Elements are drawn in the order they were created (an element that is
replaced keeps its place), above the background, sprites, inline images, video, hotspot pictures and
the colour cover and flash, and below the textbox, quick menu and every other piece of interface. That
is why a title card can sit on a black screen:

```
::bg{color=#000000}
::hud{id=card text="{size=96}Chapter 1{/size}" x=center y=0.4 fade=1}
::wait{time=3}
::hud{hide=card fade=1}
```

They are part of the story's interface: they are not drawn on the main menu, nor under the full-screen
screens (Settings, History, Save, Load, Extras, the shop, the inventory and text input). They **are**
drawn (dimmed) under a confirmation question over the story. The `U` key hides them together with the
textbox; `::textbox{hide}` does not. Text in a `{color=..}` tag doesn't fade with the element.

Elements, like the background and sprites, are part of the scene: rollback, saves and hot reload
restore them (a save from before `::hud` existed shows none; fades are not saved). `quillstone test`
ignores `::hud` itself but counts the variables its text, `bar`, `max` and `if` read, so a typo is
reported as a variable that is never set. Putting an element in a text-heavy corner is up to you: the
textbox and the quick menu are drawn over it.

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
- Voice ducking: with `voice_duck` in the theme (for example `0.5`), music and looping
  sounds sink to that fraction of their volume (0.3 s) while a voice clip plays and come
  back (0.6 s) when it ends. One-shot sounds and the voice itself are never ducked; a line
  whose voice follows another stays ducked; skipping, which plays no voices, never ducks.
  The volume sliders and each `::music`/`::sfx` `volume` still apply on top. Without the
  key (or `1`) nothing changes.

Image formats: **PNG**, **JPEG** or **WebP**, at most 8192 pixels on a side;
`quillstone check` refuses anything else.

Video formats: **WebM** (`.webm`) with VP9 (recommended) or VP8 video and Opus
or Vorbis sound. These are royalty-free and they are what a shipped game can
play, so `quillstone check` refuses other videos (MP4, AV1, H.264, Ogg Theora...)
and shows how to convert them, e.g.
`ffmpeg -i in.mp4 -c:v libvpx-vp9 -crf 32 -b:v 0 -c:a libopus out.webm`.
A video's own sound plays with its picture: a `::video` cutscene and a clip opened in the Extras
gallery have sound; a looping `::bg` video and a video menu background are always muted. The sound
uses the **music** volume setting. Voice lines don't turn it down, and music already playing keeps playing
under it (stop the music before a cutscene if you want the clip alone). It starts with the first picture,
and stops when the clip is skipped, rolled back past, replaced or closed; a menu over the story or a
minimised window holds it with the picture. A clip without a sound track is simply silent. An unskippable video only blocks clicks while it
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
::end_replay                            end a scene replay (see below); otherwise does nothing

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
- **Locked options.** An option with an `if` is hidden while the condition is false. Add `show`
  to draw it greyed instead (it can't be picked, but it tells the player there is more to find),
  and `show="reason"` to also print a reason in small dim text under it:

  ````markdown
  ::choice
  - [Open the door](#door){if has_key, show}
  - [Lift the gate](#gate){if str >= 5, show="Needs STR 5"}
  - [Read the runes](#runes){if lore >= 3, show="???"}
  ::
  ````

  A `show` needs an `if` (what unlocks the option), and the reason goes in double quotes. The
  reason is optional on purpose: if you don't want to give the game away, use plain `show` (just
  the greyed text) or write `show="???"` yourself. The reason can use `{variables}` and markup
  like the option text, and is translated like it. A greyed option can't be clicked, chosen with
  a number key or picked when a timed choice runs out, and the arrow keys and a gamepad skip it:
  if the `default` option is locked then, the first pickable one is picked. A choice where every
  option is locked counts as one with nothing on offer, so the story carries on past it. A
  `{once}` option that was already picked stays hidden, not greyed. `show` isn't for
  `::hotspots`. `quillstone test` never picks a locked option.
- A **timed choice** runs out: `::choice{time=8}` (seconds, 0.5 to 600, a plain
  number) and exactly one option marked `default` in its braces. When the time is up
  that option is picked as if it had been clicked, so a silent answer is an answer too:

  ````markdown
  ::choice{time=8}
  - [Tell the truth](#truth)
  - [Say nothing](#silent){default}
  ::
  ````

  `default` combines with the other settings (`{default, once, trust-=1, if met}`). If
  the default option's `if` is false when the choice shows, the first visible option is
  picked instead. A countdown bar over the options shows the time left; it starts when
  the options appear (not while the line above is still being typed), and starts full
  again after a rollback to the choice or a load. It waits while Settings, the history or
  a question is open and while the window is not the active one. Picking by click or key
  works as usual, hovering changes nothing, auto-forward never picks for the player, and
  skipping stops at the choice. A timed choice is an error without `default` (or with
  two), `default` is an error without `time`, and `::shop` can't be timed (a
  `::hotspots` block can: see Hotspots). `quillstone check` warns when the time is under 2 seconds, too quick to read;
  `quillstone test` plays the default like any other option. The bar's look is a theme
  setting (see Themes).
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

- An entry is `- [label](#target){rect=x,y,w,h ...}`. `rect` is required (unless the region
  follows a sprite, see below): the
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
- **Timed screens.** `::hotspots{time=3 default=#too-slow}` gives the player 3 seconds
  (0.5 to 600, a plain number) and jumps to the label when time runs out; both are
  needed together. It counts down like a timed `::choice` (see Choices): the same bar
  (along the top of the screen, since there are no buttons), starting when the screen
  appears and again after a rollback or a load, waiting while Settings, the history or
  a question is open and while the window is not the active one, never picked for the
  player by auto-forward, and skipping stops at the screen. The jump is a label, not
  one of the regions, because running out of time is the player clicking nothing;
  a region's own effects don't run. `quillstone test` plays the timeout as one more
  outcome and `quillstone graph` draws it as a "(time out)" edge.
- **Regions that follow a sprite.** `sprite=firefly` instead of `rect=` binds the region to
  a sprite: it is wherever that sprite is drawn right now, through `::move` (also a
  looping one), `::anim` frames, `scale`, `xscale`, `flip`, `sprite_size: pixels` and the
  camera. The name is the one a `::show` gave the sprite (`::show{id=firefly src=...}` or a
  character's name); `quillstone check` reports a name no `::show` uses. With a
  `rect=` too, the rect is a fraction (0 to 1, or a percentage) of the sprite's own
  rectangle, say the lower half `rect=0,50%,1,50%`; without one it is the whole sprite.
  While the sprite is not on stage (not yet shown, hidden, fading out) the region can't be
  clicked or reached with the arrow keys; `quillstone test` counts it as offered
  while the sprite is shown, since it can't tell where it is. Highlight, tooltip and an
  `image=` follow the sprite too. Example, a lamp that bobs across the screen
  (`examples/minigame.quill.md`):

  ````markdown
  ::show{id=lamp src=assets/lamp.png x=10% y=40%}
  ::move{lamp x=90% time=2 ease=inout loop}
  ::hotspots{time=5 default=#too-slow}
  - [The lamp](#caught){sprite=lamp}
  ::
  ````
- `quillstone check` checks the images and warns about a region that can never
  be clicked because a later one covers it entirely, or a block whose regions are all
  `hidden`; `quillstone test` follows
  every region like a choice option.

## Timing bar

A bar with a marker that sweeps back and forth; the player stops it, and where it stops
decides the result: inside the *perfect* zone, inside the *good* zone, or a miss. It is one
step of the story, like a choice: the result is stored in a variable and the story goes on
with the next line, where `::if` can branch on it. Lockpicking, a golf swing, a fishing
strike (`examples/minigame.quill.md` has a lock with three pins):

````markdown
---
vars: { pins: 0, hit: "" }
---
::hud{id=pintext text="Pins set: {pins} / 3" x=0.5 y=0.12 size=32 color=#ffd98c}
::timing{result=hit speed=1.2 good=35%..65% perfect=47%..53% time=6 prompt="Pin {pins}: stop the needle!"}
::if{hit == "perfect"}
::set{pins += 1}
The pin clicks home, clean.
::elif{hit == "good"}
::set{pins += 1}
The pin catches, just.
::else
The pick slips.
::endif
````

- `result` (required) is a text variable declared in `vars:` (a text default, like `hit: ""`).
  It is set to `"perfect"`, `"good"` or `"miss"`; without a `perfect` zone it is never
  `"perfect"`.
- `good` (required) is the zone to stop in, written `from..to` as percentages of the bar's
  length (`40%..60%`) or fractions (`0.4..0.6`); `from` must be below `to`. The ends count as
  inside. `perfect` (optional) is a smaller zone that must lie inside `good`.
- `speed` (optional, default 1) is how many times the marker crosses the bar in a second,
  from 0.2 to 5. It starts at the left end, runs to the right end and back again, and so on
  for ever, so `speed=1` is a back-and-forth every two seconds.
- `time` (optional, seconds, 0.5 to 600, a plain number) is the limit: when it runs out
  without a press the result is `"miss"`. Without it the bar waits for ever.
- `prompt` (optional) is shown above the bar. Like an `::input` prompt it takes `{name}`
  values, `**bold**`, `*italic*`, colour and size tags (not `{nw}` or `{p}`), and it is
  translated through `lang export` like one.
- The player stops the marker with a click, Space, Enter or Right, or the gamepad's confirm
  button: the same presses that advance a line. The marker starts moving once the line before
  it is fully typed. It stands still while the window is not in focus or minimised and
  while Settings, History or a confirmation is open, like the countdown of a timed choice.
  Skip mode stops at the bar and auto-forward never presses it.
- Rollback goes back to before the bar, which starts afresh (marker at the left end, full
  time limit); roll forward uses the result that was recorded. A save made at the bar loads
  at the bar, started afresh. Nothing about the marker is saved.
- `quillstone test` plays each result as a branch (miss and good, plus perfect when there is
  a perfect zone). `quillstone graph` draws no extra arrows: the story simply goes on, and
  your `::if` jumps show as usual. `quillstone run` asks you to type `p`, `g` or `m` for
  perfect, good or miss.
- The look comes from the theme (see Themes): by default a track in `panel_color`, the good zone
  in `accent_color` (see-through), the perfect zone in `name_color` and a thin marker in
  `text_color`, in the middle of the screen.

````
timing_bar_image = assets/gui/lockbar.png      # stretched over the track instead of panel_color
timing_marker_image = assets/gui/needle.png    # the marker, drawn at its height with its own proportions
timing_good_color = #ffd98c80                  # the good zone (default accent_color, made see-through)
timing_perfect_color = #ff5555                 # the perfect zone (default name_color)
timing_marker_color = #ffffff                  # the plain marker (default text_color)
````

  The zones are always drawn over the track, so a picture should leave them visible. The
  pictures are checked by `quillstone check` and packed by `quillstone package` like other theme
  pictures.

## Key prompts

Ask for a key within a time: a quick-time event ("Duck!") or, with `count`, button mashing
("Force the door!"). Like the timing bar it is one step of the story: the result is stored in
a variable and the story goes on with the next line, where `::if` can branch on it
(`examples/minigame.quill.md` has a dodge and a door at the end):

````markdown
---
vars: { ok: "" }
---
::press{key=space time=1.2 result=ok prompt="Duck! Press **Space**!"}
::if{ok == "hit"}
The dog sails over you.
::endif
::press{key=enter time=4 count=12 result=ok prompt="Force the door!"}
````

- `key` (required) is one of `space`, `enter`, `left`, `right`, `up`, `down`, `click` (a mouse
  click) or a letter `a` to `z`. Anything else is an error that lists these. The prompt draws
  a key cap with the key's name. `space` and `enter` are the same key here (either one
  counts), which is what lets the gamepad's confirm button press them.
- `time` (required, seconds, 0.5 to 600, a plain number) is the limit.
- `count` (optional, 1 to 200, default 1) is how many presses it takes: a bar under the time
  bar fills with each one. All of them within `time` is a hit.
- `result` (required) is a text variable declared in `vars:`. It is set to `"hit"` or
  `"miss"` (time ran out first).
- `prompt` (optional) is shown above the key cap, with `{name}` values and styling like the
  timing bar's, and translated through `lang export` too.
- A wrong key does nothing (it is not a miss, so a slip of the finger is forgiving). While
  the prompt is up, Space, Enter, the arrows, Backspace, PageUp and the letters do not
  advance, go back or open the history, auto-forward and so on, whatever the key asked for;
  Esc still opens the Save screen, and the quick menu buttons still work. A key that was already
  held down when the prompt appeared does not count, and holding a key does not repeat it:
  every press is a new one. The same goes for the timing bar.
- The clock stands still while the window is not in focus or minimised and while Settings,
  History or a confirmation is open, and starts once the line before it is fully typed. Skip
  mode stops at the prompt and auto-forward never presses it.
- Rollback goes back to before the prompt, which starts afresh (full time, no presses); roll
  forward uses the recorded result. A save made at the prompt loads at the prompt, started
  afresh.
- `quillstone test` plays both results as branches; `quillstone graph` draws no extra arrows.
  `quillstone run` asks you to type `h` (hit) or `m` (miss).
- Looks: a key cap in `panel_color` (or the picture `press_key_image`), the time bar in
  `choice_timer_color` (default `accent_color`) and the presses bar in `timing_perfect_color`
  (default `name_color`), in the middle of the screen.

````
press_key_image = assets/gui/keycap.png   # the key cap, 160x120 on the 1920x1080 screen (checked and packed like other theme pictures)
````

## Items

Items give the things in a list variable a name, a picture and a description for
the [inventory](#inventory) and [shop](#shop) screens. Declare them before they are
used, like characters:

````markdown
::item{id=key name="Brass key" icon=assets/gui/key.png price=15 text="Opens the clock tower."}
::item{id=map}
::item{id=potion name="Potion" count=potions use=#drink-potion}
````

- `id` is the text stored in list variables (`inventory += "key"`): letters, digits,
  `_` and `-`. Declaring an id twice is an error.
- `name` is what the screens show (the id if left out). `text` is the description
  shown when the item is hovered or selected.
- `icon` is a picture (PNG, JPEG or WebP) drawn above the name. `price` is a whole
  number, 0 or more: what a shop asks unless its line says otherwise.
- `count=potions` makes a stack: it names a whole-number variable declared in `vars:`
  (or `persistent:`), and the item is shown with "x3" under its name while that number
  is above 0, and not at all at 0 or less. For an item with `count=` the list is not
  looked at (set the number, not `pack += "potion"`); items without it work as before.
  Counted items are listed after the others, in the order they are declared.
- `use=#drink-potion` gives the item a **Use** button on the [inventory](#inventory)
  screen: it calls that label (a label without parameters; `quillstone check` reports
  an unknown one). The item's count is not changed for you, the label does it:

  ````markdown
  # drink-potion

  ::set{potions -= 1}
  ::set{hp += 5}
  You drink the potion. ({potions} left)
  ::return
  ````

  A `use=` item with no `count=` is used the same way and stays in its list unless the
  label does `::set{pack -= "scroll"}`.
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
  pages. Selecting an item (a click, or the arrow keys) only shows it; an item with
  [`use=`](#items) also shows a **Use** button next to Close, and Enter or Space on
  the selected item presses it. To let another item do something, offer a choice with
  `if pack has "key"`.
- Use works only when the player opened the inventory over the story (the Items
  button or I) and the story is waiting for them to advance a line. It closes the
  inventory and **calls** the label as a normal step, so when the label ends with
  `::return` the same line is shown again; rollback goes back to before the use and a
  save made inside the label loads inside it. Over a choice, `::wait`, `::input`, a shop,
  a line ending in `{nw}`, a scene replay, or in an inventory opened by `::inventory`,
  the button is shown greyed and a press only says "Can't use that now".
  `quillstone test` uses each such item once, so its label counts as reached.
- Opened by the player, it sits over the story, which waits: a `::wait` doesn't run
  out and nothing is skipped. Esc, a right click, the I key or the Close button closes it.
- `::inventory` opens the same screen from the script. The story waits until the player
  presses its Close button (or I, or Backspace) and then carries on; Esc or a right click opens the
  Save screen instead, which returns to the inventory, and S, L, H, F5 and F9 work there too. Rollback, saves and hot reload work with it like with
  any other waiting step, and `quillstone test` treats it as one click.

## Journal

The journal is the inventory screen fed by a second list: a quest log with no new
machinery. A quest is an [`::item`](#items) (its `name`, `text` and `icon` are the title,
description and picture), and the quest's state is just which list holds its id.

````markdown
---
journal: quests
journal_done: quests_done
vars: { quests: [], quests_done: [] }
---
::item{id=find-cat name="Find the cat" text="She was last seen on the roof." icon=assets/gui/cat.png}

::set{quests += "find-cat"}
The baker asks you to find her cat.
::journal

::set{quests_done += "find-cat"}
You carry the cat home.
````

- `journal: quests` names the list variable the journal shows; it must be declared as a
  list in `vars:` (or `persistent:`), or the error names the front matter line. The game then
  has a **Journal** button in the quick menu, the **J** key opens it, and `::journal` opens it
  from the script (an error in a game without `journal:`). Theme picture buttons use the id `journal`
  (`quick_journal_icon`, `_hover`, `_pos`, `_size`), and `quick_buttons = listed` handles it like the others.
- Entries are declared like items, in the same `::item` lines (a quest id and an item id share one
  namespace). Only `name`, `text` and `icon` matter here: the journal has no Use button, prices or counts,
  even if the item has them. An id with no declaration is shown under its id; `quillstone check`
  warns about a literal `+= "x"` or `has "x"` on the journal lists that names nothing declared.
- `journal_done: quests_done` (optional, needs `journal:`) names a second list of finished quests.
  They are shown after the open ones, greyed, with "Done" under the name, so marking one done is
  one line, `::set{quests_done += "find-cat"}`; you may also take it out of the first list with
  `::set{quests -= "find-cat"}`, which changes nothing on screen. Without `journal_done:` a quest is
  finished by removing it from the list. Nothing else is tracked: there is no state machine, only plain
  list variables, so saves, rollback and `quillstone test` treat them like any other.
- The screen is otherwise the inventory's (the same grid, descriptions, pages, theme keys and close keys;
  only J, not I, closes the journal, and `journal_title` sets its title, default "Journal").
  The look of a done quest is `inventory_cell_disabled`. Opened by the player it sits over the story;
  `::journal` leaves the story waiting until it is closed, and `quillstone test` treats it as one click.
- The title and the "Done" label are not translated; the quest names and descriptions are, like any
  item's (see [Translating a game](#translating-a-game)).

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
  block and the story continues after it. Esc or a right click opens the Save screen
  instead, as everywhere else, and closing it returns to the shop (page, selection
  and purchases kept); S, L, H, F5 and F9 work there too (save, load, history, quick
  save and load). If no item is on offer the block
  is skipped.
- An item the player can buy is bought by clicking it, or by moving to it with the
  arrow keys and pressing Enter or Space: the price is taken from `pay`, the item is
  added to `into`, a notice says "Bought Brass key", and the shop stays open with the
  new money. An item already in the list is marked **Owned** and can't be bought again;
  an item that costs more than the player has is greyed out and a click only says
  "Not enough gold". Money can never go below zero.
- An item with [`count=`](#items) is never "owned": buying it adds 1 to its count
  variable (and nothing to `into`), as often as the player can pay. Its price must be
  above 0, and its count variable must be `persistent:` exactly when `pay` is.
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
  Save screen (closing it returns to the field with the text kept); F5 and F9 quick save and load.
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
- **`gallery:`** lists pictures and video clips for the main menu's **Extras** screen; each one
  unlocks the first time the story shows it (`::bg` or an inline image; a `::bg{anim=...}` unlocks
  every listed picture that is one of its frames). An entry that is a video (`.webm`, or any
  format `quillstone check` accepts for `::bg`) unlocks the first time the story plays it as a
  looping `::bg` background or as a `::video`; pictures and clips can be mixed freely
  (`gallery: [assets/cg1.png, assets/clip1.webm]`). An entry can name the picture its tile
  shows after a `|` (spaces around it are optional): `gallery: [assets/cg1.png, assets/clip1.webm | assets/thumbs/clip1.png]`.
  A thumbnail is a picture (PNG, JPEG or WebP) in the game folder; `quillstone check` reports a
  missing or unreadable one and `package` ships it. It is only the tile of an unlocked entry (a
  locked tile stays a `?`, and the entry itself unlocks as above, never the thumbnail). Without a
  thumbnail a picture's tile shows the picture and a clip's tile is a dark placeholder with a play
  symbol and the clip's name (no thumbnail is made from the video). Opening a clip
  plays it on repeat, full screen and fitted like a picture, with its sound; click, Esc or
  Enter closes it, Left/Right move to the next unlocked entry, picture or clip. The clip stops
  as soon as it is closed.
  **`music_room:`** does the same for `::music` tracks (named after their file:
  `rainy_day.ogg` shows as "Rainy day"). The keys may repeat to make long lists.
  `quillstone check` complains about entries the story never shows or plays,
  since they could never unlock.
  **`::unlock{src=assets/clip1.webm}`** unlocks a `gallery:` or `music_room:` entry by its file
  (the entry's own path, not its thumbnail) without showing or playing it: for a picture that is
  a composed scene, a clip or track unlocked by a story flag, or a bonus. `src` must be a listed
  entry (`quillstone check` and the parser report anything else, with the line), and `check`
  counts it as showing or playing the entry. Like any unlock it is kept across playthroughs,
  rollback and loading a save never take it away, and `quillstone test` simply goes past it.
- **`replays:`** lets players watch scenes again from Extras: see [Scene replays](#scene-replays).
- `quillstone test` plays the story from a first playthrough and also from
  later ones (each value the script gives a persistent variable).

## Scene replays

````markdown
---
replays: { the-rooftop: "The rooftop", night-walk: "A walk at night" }
---

# The rooftop
...
::end_replay
::jump{to=finale}
````

- **`replays:`** lists scenes the player can watch again. Each entry is a label (written the
  way a `::jump` writes it: `# The rooftop` is `the-rooftop`; spaces and underscores
  become dashes) and the title shown in the menu. The label must exist and
  take no parameters; titles must be different and not empty, at most 60 characters (write `\"` for a quote
  mark inside one); there can be up to 64.
  Entries show in the order written (several `replays:` lines add up).
- A scene **unlocks** the first time the story reaches its label, however it gets there
  (falling in from above, `::jump`, `::call`, a choice). Like gallery pictures, it stays
  unlocked for good: rolling back or loading an older save never takes it away.
- The main menu's **Extras** screen then has a third tab, **Scenes** (Extras appears when the
  game has a gallery, a music room or replays; Tab switches tabs). Unlocked scenes show their
  title, locked ones show `???`. Enter, a click or the gamepad's A starts a scene.
- A replay starts a **fresh game at that label**: the script's starting variables, the
  player's persistent variables as they are, an empty stage (nothing carries over from
  whatever the player did), and no menu music. Everything works as in the story: choices,
  waits, hotspots, shops, text input, videos, rollback (back as far as the start), history
  and settings.
- It ends back on the Scenes tab, with the menu music resumed, at **`::end_replay`**, or when
  the story ends (`::end`, `::return` with nothing to return to, the end of the script). In the
  normal game `::end_replay` does nothing, so you can put it where a scene ends and the story
  carries on after it. It takes no attributes.
- A replay **changes nothing that lasts**: no auto save, and Save, Load, Quick save and Quick
  load show "Not available during a replay"; variables the scene sets, persistent variables,
  gallery, music room and scene unlocks, and which lines were read are all thrown away when
  it ends. (Skip uses the player's real read record, so it stops at lines the player has
  not read yet.) Settings' "Main menu" asks "Leave this replay?" and goes back to Extras;
  Quit still quits the program.
- `quillstone test` warns about a replay whose label no branch can reach ("can never be
  unlocked"), and `quillstone check` warns when no `::end_replay` follows a scene, so it
  would run to the end of the game (a scan along every jump and choice, so approximate).
  The demo `examples/replay.quill.md` has two scenes.

## Translating a game

````markdown
---
title: My Story
languages: en=English, sv=Svenska
---
````

`languages:` lists the languages the game is offered in as `code=Name`. The **first** is the
one the script is written in; every other language gets a translation file. Codes are short
(letters, digits, `-` and `_`, at most 16), at most 16 languages. The name is what the player sees
in Settings, so write it in that language itself. Without `languages:` nothing changes anywhere.

With more than one language, Settings gets a **Language** row (click, Left/Right or the gamepad
cycle it). The choice applies at once to the line on screen, the choices, the history and every
other text, and is kept with the player's other settings. Saves and rollback never contain translated
text or the language, so they work the same whichever language is chosen.

### The translation file

Each language besides the first has `lang/<code>.txt` in the game folder (`lang/sv.txt`; the same
folder rules as every other game file apply: inside the script's folder, no `..`). Let the tool create it:

```
quillstone lang export my_story.quill.md sv
```

It writes every translatable text of the script, once each, in script order. Run it again whenever the
script changes: new texts are added, texts that are gone are marked `! obsolete` and kept (with their
translation, which comes back if the text returns), and nothing a translator wrote is removed.

A file is plain lines that start with a marker. Nothing needs escaping: texts are written exactly as in the script.

```
# Notes for the translator start with #; lines starting #: say where a text is used.
#: rooftop:12
> Hello, {name}! {w=0.5}You came after all.
= Hej, {name}! {w=0.5}Du kom ändå.

# Words that need a different translation for one speaker: add @ Speaker above the >.
@ Mika
> Yes.
= Jo, absolut.

> Yes.
= Ja.

> Open the door
=
```

- `>` is the script's text, `=` the translation. A text can span several lines (repeat the marker).
  An empty `=` means "not translated yet": the original shows.
- The `>` text is the **exact** script text before `{name}` values go in, tags included.
- `@ Name` (the speaker's display name) makes an entry apply only to lines said by that character; an entry
  without it applies everywhere, and is the fallback.
- `! obsolete` (above the `>`) marks a text the script no longer has.
- Blank lines separate entries and are ignored otherwise. A line starting with anything else is an error.

What can be translated: dialogue and narration, choice and hotspot texts (with the reason of a locked option), character names, item names
and descriptions, `::hud` texts, `::input` prompts, `::notify` texts, the title, `replays:` titles and the file names listed in `gallery:` and
`music_room:`. A text with no translation shows in the script's language, without any message. The
game's own buttons and screens ("Settings", "Save", "Back" ...) are not translated yet.

Keep every `{name}` and every text tag (`{nw}`, `{p}`, `{w=..}`, `{size=..}`, `{color=..}` and their
closing tags) in the translation, as often as in the original: `{name}` values are put in after the
translation is chosen, and `{nw}` must still end the line. A tag's own value (a colour, a pause) may differ.
The theme's `font` has to cover the characters the translations use.

`quillstone check` reads every translation file: syntax errors with `file:line`, a translation that
drops, adds or misspells a `{name}` or tag is an error, texts the script no longer has are warnings,
and a count of untranslated texts is printed for each language. `quillstone package` ships the
`lang/*.txt` files of the languages the script lists. Translation files are limited to 8 MB, 100,000 entries and 16 KB
per text.

## What rollback restores

Going back one step restores variables, the random seed (so a roll repeats), the call stack, background, sprites and
their positions, heights, depths, sizes, stretch, mirroring, layers, animations and looping moves (from the start of their cycle), the camera, the colour cover of `::fade{out}`, the ambient particles' settings, the colour wash of `::tint`, the HUD elements, music (an intro is not resumed part-way), looping sound effects, the inline images and voice of the line, and the line
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
spot instead. A save whose label was renamed or removed can't be loaded. Moving
part of a story into an `::include` (or back) changes nothing for saves, since they are
checked against the one compiled story. Skip
mode remembers read lines by their label and text, so rewording a line makes it
"unread" again. The same rules apply when the player reloads a script you just
saved (hot reload): it carries on where you are. A save that was waiting at a
choice, hotspot block or shop where nothing is on offer any more (an edited
condition) carries on past it. Choice texts, a timed choice's time limit and default, hotspots, shop stock and input prompts always come from
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
text_outline_size = 2               # a border round dialogue and name text, 0 to 8 px (default 0: none)
text_outline_color = #000000        # its colour (default black)
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
quick_buttons = all                 # all (default) or listed: only buttons with a quick_<id>_icon or _pos (see below)
quick_align = right                  # right (default), center or left: where the row sits along the textbox (see below)
menu_background = assets/gui/menu.png   # a picture, or a video file that loops silently on the main menu
menu_music = assets/audio/menu.ogg
voice_duck = 0.5                    # music and sound loops play at this fraction of their volume while a voice clip does, 0 to 1 (default 1: off)
choice_timer_color = #ffd98c        # the countdown bar of a timed ::choice (default accent_color)
choice_timer_height = 8             # its thickness, 2 to 40
timing_bar_image = assets/gui/lockbar.png   # the ::timing bar's track and marker pictures, and zone and marker colours:
timing_marker_image = assets/gui/needle.png # timing_good_color, timing_perfect_color, timing_marker_color (see "Timing bar")
press_key_image = assets/gui/keycap.png     # the key cap of a ::press prompt (see "Key prompts")
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
- `font` applies to the dialogue, the names and every other text of the game; `check` reports a
  file that is missing or is not a font. Only that one family is used for the whole game.
- `text_outline_size` draws a border of that many pixels (at 1920x1080, scaled with the window) in
  `text_outline_color` under the letters of the dialogue and of the speaker's name; other text
  (buttons, History, the screens) has none. It follows the textbox's fade. Thin values (1 to 3) look best.

### The text input screen

`::input` shows a centred 1200x460 panel. Two keys change it; without them it looks as it always has.

```text
input_image = assets/gui/input.png    # stretched over the panel instead of panel_color
input_rect = 360, 310, 1200, 460      # x, y, width, height; inside the screen, at least 600x460
```

The prompt, field and OK button keep their sizes and sit inside the panel (a taller panel centres
them; a wider one widens the prompt and the field). Clicking OK follows the panel. `check` reports a
missing or unreadable picture; `package` includes it.

### The settings screen

The Settings screen (not the main menu, Save, Load or History) can be moved, made smaller and
dressed with pictures. Without these keys it looks as it always has.

```text
options_rect = 80, 60, 960, 960       # x, y, width, height of the panel; at least 640x420 (default 400, 90, 1120, 910)
options_text_size = 30                # the screen's text size: rows, buttons and hint scale with it (8 to 200, default 36)
options_background = assets/gui/options.png   # full-screen picture instead of overlay_color
options_panel_image = assets/gui/frame.png    # drawn over the panel as a 9-slice
options_panel_border = 40             # the 9-slice corner size in the picture's pixels (0: stretch whole; default 0)
```

- The title, the rows, the buttons and the hint are laid out inside `options_rect`: the title at its top,
  the hint at its bottom, the slider tracks across the middle, the buttons centred. Mouse and keyboard
  follow the new layout. The panel must be tall enough for the rows: about 860 px at the default text
  size (less for a smaller `options_text_size`), and wide enough for the values (about 800 px at the
  default size).
- A 9-slice keeps the four corners (`options_panel_border` pixels square, shown at that size in the
  1920x1080 screen) and stretches the edges and the middle, so rounded corners come from the picture.
  The border is cut back if it is more than half of the picture or of the panel.
- `check` reports a missing or unreadable picture; `package` includes the pictures.

**The game menu.** Esc and a right click during the story (also over a shop, an inventory or a text
input) open the Save screen. A row of tabs runs along the top of the
Save, Load, Settings and History screens: Save, Load, Settings, History, Main menu and Quit (the
last two ask first, like the Settings buttons). A click on a tab, or the Tab key (next screen,
round again), moves between them; Esc or a right click closes back to where the player came from.
From the main menu the row shows Load, Settings and Quit only. The tabs use the theme's
`button_color`, `button_hover_color` (current tab and hover), `text_color` and `accent_color`
(the current tab's label). A Settings panel placed with `options_rect` above y = 40 is covered by them.
During a scene replay Esc opens Settings instead, where Main menu leaves the replay.

**The quick menu** (Back, History, Skip, Auto, Save, Load, Q.Save, Q.Load, Settings, and Items and Journal
in a game with an inventory or a journal) sits in a row directly below the textbox. `quick_align` says
where along the textbox: `right` (default) ends at its right edge, `left` starts at its left edge,
`center` is centred on it; the row is always moved as far as needed to stay on the screen. When there
is no room below (the textbox ends less than 36 px above the bottom of the screen) the row is drawn
just inside the bottom edge of the textbox instead. `quick_menu` says when it shows:

| `quick_menu` | |
| --- | --- |
| `visible` | always (default) |
| `hover` | only while the cursor is over the textbox or the row, fading in and out in about 0.15 s; buttons that aren't shown can't be clicked |
| `hidden` | never drawn or clickable |

The keyboard and gamepad shortcuts work in every mode. Players can override the mode in
Settings (Quick menu: Game default, Always, On hover, Hidden).

**Picture buttons.** A theme can replace the text labels with pictures and put buttons where it
likes. Every key is optional and names a button by its id: `back`, `history`, `skip`, `auto`, `save`,
`load`, `qsave`, `qload`, `settings`, `items` or `journal` (Q.Save and Q.Load are `qsave` and `qload`).

```text
quick_back_icon = assets/gui/back.png        # idle picture: drawn instead of the text label
quick_back_hover = assets/gui/back_hover.png # while the cursor is on it (needs _icon)
quick_skip_on = assets/gui/skip_on.png       # while Skip or Auto is switched on (needs _icon)
quick_back_pos = 160, 976                    # top left in the 1920x1080 screen
quick_back_size = 56, 56                     # width, height (16 to 512 each)
```

- `_icon`, `_hover` and `_on` are PNG, JPEG or WebP files; an id that isn't in the list, a bad
  number or a `_hover` or `_on` without its `_icon` is reported like any other theme mistake.
- Without `_size` a picture button is as big as its picture (kept between 16 and 512 px on a
  side); a text button is 150x32. A text label wider than its button is drawn smaller (down to
  10 px) so that it stays inside it; the button keeps the size the theme gave.
- A button with `_pos` leaves the row and sits there (pulled back in if it would leave the screen);
  the other buttons keep the row below the textbox, placed by `quick_align` (those to the left of the
  gap shift right to close it). A picture button without `_pos` stays in the row, as wide and tall
  as its picture. `_pos` and `_size` work on a text button too.
- Without `_hover` the picture is drawn a little dimmed (80%) until the cursor is on it; with one
  it is drawn whole and swapped for the hover picture. While Skip or Auto is on, the `_on` picture
  shows; without one the idle picture stays lit and gets a thin accent line along its bottom edge.
  A button that does nothing right now (Back on the first line, Q.Load before any quick save) is
  drawn at 40%.
- `quick_buttons = listed` shows only the buttons that have an `_icon` or a `_pos` of their own and
  hides the other built-in ones (their keyboard and gamepad shortcuts still work); the default
  `quick_buttons = all` keeps every button.
- Where placed buttons overlap, the one later in the list `back, history, skip, auto, save, load,
  qsave, qload, settings, items, journal` is on top and gets the click.
- Everything else is unchanged: `quick_menu` (`hover` mode fades pictures too, and the menu also
  shows while the cursor is on a placed button), the keyboard and gamepad shortcuts,
  `::textbox{hide}`, hiding the interface with U, and the screens that have no quick menu.
- `quillstone check` reports a missing or unsupported picture as an error, and warns when two
  buttons overlap or a placed button reaches past the screen; `package` includes the pictures.

`examples/iconmenu.quill.md` and `examples/icons_theme.txt` show it.

### The inventory and shop screens

One grid layout serves both screens (and the [journal](#journal)), so they share the `inventory_` keys. Without
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
journal_title = Quests                      # the journal's title (default "Journal")
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

## Developer tools

For trying a story, not for players: a game you ship has none of this unless it is started with `--dev`.

- `quillstone-player <game> --dev` turns on the console. Press F12 while the story is showing; a line opens at the
  top of the screen and the game stands still until Esc (or F12 again) closes it. Folder mode does not turn it on
  by itself, so you test the game the way players get it. Without `--dev` the key does nothing.
- Commands (one line, then Enter; errors show as a line and leave it open):
  - `get <var>`, or just the name: the value (`gold = 5`, text in quotes, lists in brackets).
  - `vars [prefix]`: the variables, sorted, at most 10 (the rest is counted; a prefix narrows it).
  - `set <var> = <expression>`: the same assignment `::set` takes (`=`, `+=`, `-=`, `random(1,6)` too). A variable the
    script declares keeps its kind: a number can't become text, and a list changes only with `+=` / `-=`.
    The line on screen is redrawn with the new value.
  - `jump <label>`: go there now, as `::jump` does: variables, call stack and the picture stay. It is an ordinary
    step, so rollback comes back to where you were.
  - `help`.
- What the console changes is real: saves made afterwards include it. Setting a variable is not a rollback step
  (rolling back to an earlier screen brings that screen's variables back). The console opens only over the story, not
  over a text input, shop, settings or a scene replay.
- `quillstone-player <game> --dev --start <label> --set gold=50 --set met=true` starts a new game at a label with those
  variables (`--set` repeats; the value is read by the variable's declared type, text needs no quotes; the variable
  must be declared in `vars:`). `--start` and `--set` without `--dev` are refused.
- `quillstone run <script> --start <label> --set var=value` does the same in the terminal.

## Not (yet) in the language

Pasting into a text input,
character voices per speaker, and transitions other than fades and wipes.
The runtime is built so these slot in without changing what is written above.
