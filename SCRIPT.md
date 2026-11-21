# Quillstone script reference

A script is a markdown file (use the extension `.quill.md`). It is read top to
bottom; the player advances through it one *waiting* step at a time (a line, a
wait, a video, a choice). Everything else runs instantly in between.

The `quillstone` tool works on a script. Errors name the file and line.

| Command | What it does |
| --- | --- |
| `quillstone check script.quill.md` | validates a script and that its files exist |
| `quillstone test script.quill.md` | plays every branch and reports story bugs: loops with no way out, unreachable parts, options never offered, misspelt variables |
| `quillstone run script.quill.md` | plays it in the terminal (`--start <label>` and `--set var=value` begin elsewhere) |
| `quillstone graph script.quill.md > story.md` | writes the story's structure as a flowchart |
| `quillstone dump script.quill.md` | shows the compiled form |
| `quillstone package ...` | builds the folder you ship (see the README) |

**Unused files.** `check` also warns (never an error) about pictures, audio, video and fonts in
the game folder that nothing uses: the files `package` would leave out. It lists the first 20.
It stays quiet when other scripts sit beside this one (one assets folder shared by several
games).

**The flowchart.** `graph` draws:

- a box per heading, as you wrote it, grouped per `::include`d file;
- an arrow for running on into the next heading, for `::jump` (thick), and for `::call` and a
  hotspot's `call` (dashed);
- an arrow for each choice or hotspot, labelled with its text, and for the inventory's Use.

A condition shows as `if ...`, an ending has rounded ends, and a heading nothing leads to is
dashed red. The output is Mermaid, which GitHub and VS Code draw as a picture; `--format dot`
gives Graphviz and `-o file` writes a file. `graph` reads the script, it doesn't play it: for
what can actually happen, use `test`.

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

- **Front matter** (optional, between `---` lines) holds the game's settings. The keys are
  listed [below](#front-matter-keys).
- **Headings** (`#` to `######`) are *labels*: places to jump to. `# The Roof` is
  the label `the-roof` (lowercase, words joined by dashes). A heading like
  `# heal(amount, who)` is a label with parameters, see
  [Labels with parameters](#labels-with-parameters).
- **`**Name:** text`** is dialogue. Any other prose is narration. Consecutive
  lines join into one paragraph; a blank line, a new `**Name:**` or a command ends it.
- **`<!-- comments -->`** may span several lines and are ignored. Inside a
  `::choice` block they must fit on one line.
- **Commands** are one line: `::name{key=value key="text with spaces" flag}`.
  Times are in seconds.
- **Paths** are relative to the script file, with `/` between folders. They must stay in its
  folder or below it: no `..`, no drive letters, no backslashes. A `src=`, `icon=`, `image=`,
  `sprites=`, inline image, `gallery:`, `bonus:`, `music_room:` or `theme:` path that breaks this is a
  line-numbered error (otherwise the game's saves could never be loaded again). Braces can't be
  part of a file name; see [Picture names from variables](#picture-names-from-variables) for the
  one place a `{variable}` may stand in a name.

### Front matter keys

| Key | What it sets |
| --- | --- |
| `title:` | the game's name, also shown on the main menu |
| `id:` | the name of the folder the player keeps saves in (see below) |
| `icon:` | the game's icon (see below) |
| `theme:` | a theme file, see [Themes](#themes) |
| `vars:` | the variables and their starting values, see [Variables](#variables) |
| `voice:` | see [Auto-voice](#auto-voice) |
| `inventory:` | see [Inventory](#inventory) |
| `journal:`, `journal_done:` | see [Journal](#journal) |
| `languages:` | see [Translating a game](#translating-a-game) |
| `persistent:`, `gallery:`, `bonus:`, `music_room:` | see [Kept across playthroughs](#kept-across-playthroughs) |
| `replays:` | see [Scene replays](#scene-replays) |
| `chapters:` | a Chapters screen on the main menu: see [Chapter select](#chapter-select) |
| `sprite_size:`, `art_scale:` | how big sprites are drawn, see [Sprites at their picture's size](#sprites-at-their-pictures-size) |

**`id:`** is 1 to 64 lowercase letters, digits and `-`, like `id: my-story`. Set it once and the
saves survive a new title. Without it the folder is named after the title, or the script file.

**`icon:`** names a PNG, JPEG or WebP picture in the game folder, like `icon: assets/icon.png`.
Without it the game has the Quillstone icon.

- Use a square picture, 256x256 or larger, with a transparent background. `check` warns about
  one that isn't square or is smaller. Small sizes are sharpened automatically.
- It is the window's icon, and `quillstone package` also puts it on the Windows `.exe`.

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
  path, and must be inside it. Assets keep that same base inside an included file:
  `assets/x.png`, not a path relative to the included file.
- The file name ends in `.quill.md` (or `.md`). It can't hold a `{variable}`.
- The **main script** keeps the front matter: `title`, `vars`, `theme`, `gallery`, `bonus`, `replays`, `chapters`,
  `persistent` and the rest. An included file has none, and starting with `---` is an error.
- Labels are global: the same label in two files is an error that names both places.
- A file can be included once. Including it a second time, or a cycle (`a` includes `b`
  includes `a`, or a file including the main script), is an error.
- Limits: any number of files, nested up to 8 deep, each up to 8 MB, and up to 64 MB of script
  in all.
- A `::choice`, `::hotspots` or `::shop` block, a comment and a front matter block each
  stay inside one file; an `::if` block may open in one file and close in another.
- Errors, warnings and `quillstone test` findings name the file and line, such as
  `chapters/ch1.quill.md:12`. In `quillstone dump` the line column shows the file as well.
- A script handed to the compiler as plain text, with no folder to read from, can't use it.
- Hot reload watches every included file: saving one updates the running game.
- `quillstone package` packs every included file into the game archive at the same path. Put
  included scripts in a folder such as `chapters/`, or name the main script `main.quill.md`,
  so the player can tell which file starts the game.

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
`hero.love`, `hero.hp`, `stats.hp.max`.

- Each part follows the rules above (letters, digits and `_`, not starting with a digit) and
  none may be empty, so `hero.`, `.hero` and `hero..love` are errors.
- The dot is only a way of writing the name. `hero.love` is an ordinary variable everywhere:
  in `vars:` and `persistent:` (`vars: { hero.love: 0 }`), `::set`, conditions, choice effects,
  `{hero.love}` in text, saves, and list variables (`bag.items += "key"`,
  `bag.items has "key"`, `count(bag.items)`).
- Character ids, label parameters and `::call` argument names stay plain, without dots.
- Numbers have no decimals: `1.5` is an error, because variables hold whole numbers.

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
- A list changes with `+=` and `-=` of text, or is set whole with a list literal:
  `::set{inventory = []}` empties it, and `::set{inventory = ["key", "map"]}` replaces its
  items. The items are text, quoted or bare, with repeats dropped. It works in the same places
  as `+=`: a `::set`, a choice or hotspot effect.
- Only a list declared in `vars:` or `persistent:` can be set this way; `gold = []` is an
  error. So are `inventory = "x"`, adding a number, and `has` on a variable that isn't a
  declared list: `quillstone check` reports them with their line. (A computed value that isn't
  text is ignored at run time.)
- In `::jump{if=...}` write quotes inside as `if="inventory has 'key'"`.
- Rollback, saves and `persistent:` lists work like any variable.
- `quillstone test` warns about an item tested with `has` that nothing ever adds
  (`inventory has "kye"`). It plays every combination of items the script can add, so a shop
  with a dozen independent items makes the test very large; it stops at its situation limit
  and says so.

## Text tags

Inside lines and choice texts:

| Write | Shows |
| --- | --- |
| `**bold**`, `*italic*`, `***both***` | bold, italic, bold italic |
| `{color=#ff8080}words{/color}` | words in that colour (`#rrggbb` or `#rgb`; they nest). With a fourth part, `#rrggbbaa` or `#rgba`, the words are partly see-through: `{color=#ffffff80}` is white at half strength |
| `{size=+10}big{/size}` | text 10 pixels bigger than the text around it (`{size=-8}` smaller); they nest, so `{size=+10}a{size=+4}b{/size}{/size}` makes `b` 14 bigger |
| `{size=24}small{/size}` | text exactly 24 pixels tall (on the 1920x1080 screen; 8 to 200, as is the result of a relative size) |
| `First.{p}Second.` | a line break inside the line (a paragraph break); it doesn't wait. A `{p}` at the very start or end of a line is ignored. Not allowed in a choice text |
| `Well...{w=0.5} no.` | the typing pauses for half a second there (a click skips it; over 10 seconds `check` warns of a typo, and 3600 is the most) |
| `Hold on.{nw}` | "no wait": once the line has finished typing the story goes on by itself, without a click or the auto-forward delay. It must be the very last thing in a dialogue line (not a choice text) |
| `{u}underlined{/u}` | underlined text, in the text's colour (they nest, and mix with the other tags) |
| `{s}struck out{/s}` | struck-through text: a line through the middle of the words, in the text's colour (it nests and mixes like `{u}`) |
| `{speed=0.5}slowly{/speed}` | the typing speed for part of a line, as a factor of the player's text speed: `0.5` is half as fast, `3` three times. From 0.1 to 10, anything else is an error. They nest by multiplying (`{speed=2}` inside `{speed=2}` is four times, kept within 0.1 to 10). Skip and instant text ignore it |
| `{whisper}quietly{/whisper}` | a whisper: about 75% of the size of the text around it and 55% of its opacity. It nests with colour and size tags (a colour's alpha multiplies with the 55%) |
| `{shake}words{/shake}` | each letter trembles at random around its place, gently (scaled to the text size). `{shake=3}` is the strength factor, from 0.1 to 5 (`{shake}` is 1; 3 is a hard shake) |
| `{wave}words{/wave}` | the letters bob up and down in a wave that travels along the words. `{wave=2}` is the height factor, from 0.1 to 5 (`{wave}` is 1) |
| `{glitch}words{/glitch}` | letters now and then turn into a random symbol for a moment and jump a little sideways. `{glitch=0.5}` is the share of letters that are symbols at any moment, from 0 to 1 (`{glitch}` is 0.07, about one letter in fourteen; `{glitch=1}` turns every letter into a symbol all the time). A symbol is centred in the width of the letter it replaces |
| `{ruby=かんじ}漢字{/ruby}` | a small **reading** above the words (furigana, a gloss: see below) |
| `\*`, `\{`, `\\` | a literal `*`, `{` or `\` (any other `\` is shown as written, so `C:\temp` is fine) |

For bold and italic in a theme font, give the theme its bold and italic files too
(`font_bold`, `font_italic`, see [Themes](#themes)).

**Ruby text.** `{ruby=reading}words{/ruby}` draws `reading` at half the size, centred above
`words`:

```
**Mika:** Do you know the word {ruby=かんじ}漢字{/ruby}? It means {u}Chinese character{/u}.
```

- Only plain words go between the tags. Bold, colour, size and underline tags go **around**
  the whole `{ruby=..}...{/ruby}`, and ruby can't nest.
- The reading is plain text too: no `{variable}` in it.
- The reading types out with its first word.
- A line that has ruby is taller by the room it needs; the others keep their height.
- If the words wrap over two lines, the reading stays over the first piece. A reading wider
  than its words may overhang its neighbours; it never moves the text.
- The history shows the words alone (`漢字`). The text-mode `quillstone run` writes the reading
  in brackets after them: `漢字(かんじ)`.

**Animated text.** `{shake}`, `{wave}` and `{glitch}` each take one optional value (above); the speed of the movement and the length of the wave are fixed. Letters appear with
the typing as usual and start moving once they are shown:

```
**Mika:** The old radio says {glitch}signal lost{/glitch}, and I am {shake}shaking{/shake}.
```

- They work wherever text tags do (dialogue, choices, `::center`, HUD texts, messages), and a
  line only pays for the moving if it has one of them.
- The movement is only for the eyes: it uses no story randomness, so saves, rollback and
  `quillstone test` are not affected.
- An underline or strike-through on moving words stays still.
- Moving tags nest, the innermost one wins, and each must be closed by its own closing tag
  (`{shake}a{wave}b{/shake}c{/wave}` is an error). The closing tag has no value: `{/shake}`.
- Players can turn the moving off with **Animated text** in Settings (on by default; saved as
  `animated_text = false`). Then the words are drawn still: shake and wave as plain text, glitch
  with its real letters. It is for players who find moving text hard to read or uncomfortable.

**Braces and tag names.**

- A `{` with no `}` after it is just a brace, and an unknown `{name}` is left as written.
- `{p}`, `{nw}`, `{u}`, `{s}`, `{whisper}`, `{shake}`, `{wave}` and `{glitch}` are tags, so a variable
  with one of those names can't be shown with `{name}`.
- A variable's value always shows as it is, even if it contains `*` or braces.

**Mistakes** are errors with the line number: an unclosed `**`, `{size=..}`, `{u}`, `{s}`,
`{speed=..}`, `{whisper}`, `{shake}`, `{wave}`, `{glitch}` or `{ruby=..}`, an empty reading, a size
outside 8 to 200, a speed outside 0.1 to 10, a `{shake=..}`, `{wave=..}` or `{glitch=..}` value that is not a number or is outside its range, a moving tag closed by the wrong closing tag, `{nw}`
in the middle of a line, `{p}` in a choice, an unknown tag like `{huge=40}`.

**Where the styling shows.** The history and save previews show the words without styling or
tags. The history keeps a `{p}` break; a save preview shows it as a space. A line with
`{size=..}` is as tall as its biggest text, so a bigger word makes its line taller; a `{whisper}` is
smaller, and `check` judges whether a line fits with that size.

**More on `{nw}`.** Use it for a line that leads straight into a choice or a scene change.

- It waits for the typing and for any `{w=..}` pause, including one at the very end:
  `Well...{w=3}{nw}` stays three seconds, even with instant text. A click still shows
  everything and cancels the pause.
- It does not wait for a voice clip: the next line's batch stops the voice. That is a
  deliberate choice; put a `{w=..}` at the end if the clip must play out.
- The line is still added to the history.
- A `::choice` after it appears once the line is typed, and the line stays above the choices.
- It waits while a menu is open, and skip mode runs through it like any line.
- Going back never stops on it (it would only move on again): rollback goes to the line
  before.

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
| `pick(a, b, ...)` or `pick(list)` | one of the values, or one item of the list variable, chosen at random (see below) |

`count` needs a list variable declared in `vars:` or `persistent:`. An unknown function
or a wrong number of arguments is an error that names the line and lists these
functions. A variable may share a function's name (`count += 1` still works).

````markdown
<!-- heal, but not above the maximum -->
::set{hero.hp = min(hero.hp + 4, hero.max_hp)}
<!-- hurt, but not below zero -->
::set{hero.hp = max(hero.hp - dmg, 0)}
::set{cost = base * 2 - discount}
::if{count(inventory) >= 2 and abs(a - b) < 3}
...
::endif
````

A command is one whole line: text after its closing `}` is an error, so write remarks as `<!-- comments -->`.

### Looking up by a variable

A name may hold other variables in `{}` after a dot. `love.{who}` is the variable `love.mia` while `who` holds the text `"mia"`: that is how you keep one stat per character and pick the one in the scene. It works wherever a variable name is read or assigned, and in text:

````markdown
---
vars: { who: "mia", love.mia: 0, love.zoe: 0, slot.0: 5, slot.1: 9, i: 1 }
---
::set{love.{who} += 1}
::if{love.{who} > 3}
You have {love.{who}} points with {who}, and {slot.{i}} in slot {i}.
::endif
::choice
- [Hug](#hug){if love.{who} >= 2, love.{who} += 1}
::
````

**What goes in a `{}` part**

- The name of a **text or number variable**. A yes/no or list variable is an error. An
  undeclared one is allowed, like a `::call` parameter or an `::input` answer.
- A text spells its piece as it is. A whole number from 0 up spells its digits: `slot.{i}` is
  `slot.1`. So a list read by position is a set of variables named `slot.0`, `slot.1`...
  (parts after the first may start with a digit).

**Where a part may stand**

- After a dot, and before a dot or the end. A name may have up to 8: `grid.{x}.{y}`.
- Not at the start, not inside another `{}`, and not in `::input{var=...}`, `::shop` or other
  attributes that take a plain variable name.

**Values that name nothing**

- The value must be a safe piece: 1 to 64 letters, digits or `_`. Anything else names nothing:
  a dot, slash, space, an empty text, a negative number, a yes/no, an unset variable. This
  matters because players can type into variables.
- A name that names nothing, or a variable that isn't declared, reads as an unset variable
  (`0`; in text the `{...}` stays as written).
- An assignment to it does **nothing**, and a warning appears once per run (the developer
  console reports it).
- A typed value can therefore never make new variables: declare every `love.zoe` in `vars:`.

**Checking**

- `quillstone check` warns when the parts can only hold values the script gives them as
  literals (`who = "kim"`, the declared start, `::call` arguments) and one combination names an
  undeclared variable: "`love.{who}` can be `love.kim`, which isn't a declared variable".
  Nothing is said when a value is typed, computed or otherwise unknown.
- Values are ordinary variables, so saves, rollback and `quillstone test` need nothing
  special. `quillstone graph` prints the condition as written.

### Random numbers

`random(lo, hi)` picks a whole number from `lo` to `hi`, both included. It can only be
the right side of an assignment, alone or inside a calculation:

````markdown
::set{roll = random(1, 6)}
::set{dmg = random(2, 4) + bonus}
::choice
- [Swing the sword](#fight){dmg = random(2, 5)}
::
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

### Picking at random

`pick("a", "b", "c")` returns one of its values at random, and `pick(inventory)` one
item of a list variable (text items; numbers work too: `pick(1, 5, 10)`). It follows
the rules of `random`: only on the right of an assignment, never in a condition or
inside another `random` or `pick`, drawn from the same seed, so rollback, roll forward
and saves repeat the pick. An empty list picks `""`.

````markdown
::set{who = pick("Mika", "Ren", "Sora")}
::set{found = pick(loot)}
::if{found == ""}
Nothing left.
::endif
````

`quillstone test` tries each value of a `pick` as a branch, like a roll (up to 12
values; a longer list is tried at its first, middle and last item, with a warning).

## Scene

The commands at a glance. Each group is explained below the table.

| Command | What it does |
| --- | --- |
| `::bg{src=... fade=1}` | set the background: a picture, a video, a colour or an animation |
| `::character{id=mika name=Mika ...}` | declare a character |
| `::show{mika happy x=20%}` | show a sprite, or change one that is on stage |
| `::hide{mika fade=0.5}` / `::hide{all}` | remove one sprite, or all |
| `::move{mika x=80% time=0.8}` | animate a sprite |
| `::camera{zoom=1.3 x=0.3 y=0.4 time=4}` | pan and zoom the stage |
| `::shake{time=0.4 strength=20}` | shake the stage |
| `::flash{time=0.4 color=#ffffff}` | a flash over the whole screen |
| `::fade{out}` / `::fade{in}` | dip the stage to a colour, and bring it back |
| `::notify{text="Saved" time=3}` | a small passing message |
| `::notify{text="Trust" delta=2}` | the same, with a signed amount after it: green up, red down |
| `::unlock{src=assets/cg1.png}` | unlock an Extras entry |
| `::particles{snow}` | weather: see [Ambient particles](#ambient-particles) |
| `::tint{color=#203060 alpha=0.35}` | a colour wash: see [Colour wash](#colour-wash) |
| `::vignette{strength=0.6}` | dark edges: see [Vignette](#vignette) |
| `::filter{gray=1}` | a colour filter: see [Colour filter](#colour-filter) |
| `::textbox{hide}` / `::textbox{show}` | hide or show the dialogue box |
| `::rollback{block}` | the player can't go back to before this: see [What rollback restores](#what-rollback-restores) |
| `::center <text>` | one line shown alone in the middle of the screen, without the textbox: see [Centred lines](#centred-lines) |
| `::page` / `::page{off}` | lines stack up on a full-screen page, or back to the textbox: see [Full-screen pages](#full-screen-pages) |
| `![alt](assets/x.png)` | an inline image |

### Backgrounds

`::bg` takes exactly one of `src=` (a picture or a video), `color=` or `anim=`.

| Write | What you get |
| --- | --- |
| `::bg{src=assets/room.png fade=1}` | a picture; it crossfades over `fade` |
| `::bg{src=clip.webm}` | a **video**, looped and muted |
| `::bg{src=clip.webm sound}` | a looping video that plays its own sound |
| `::bg{color=#102030 fade=0.5}` | a **solid colour** (`#rrggbb` or `#rgb`) |
| `::bg{anim=rain fade=1}` | an **animation** (see [Frame animations](#frame-animations)) |

- **Video sound.** `sound` is an error for a picture, a colour or an animation. The sound uses
  the music volume, like a cutscene's. It starts with the picture and stops when the
  background is replaced. Rollback and saves keep the flag.
- **A colour** needs no file. Use it where you would otherwise show a plain black picture:
  `::bg{color=#000000}`. A colour crossfades to or from a picture or another colour. Rollback,
  saves and hot reload keep it.
- `fade`, `wipe`, `mask` and `through` work for a colour and an animation as they do for a
  picture.

**Ways to change the background.** Without one of these, the new background crossfades over
`fade` seconds.

| Write | The change |
| --- | --- |
| `::bg{src=... fade=1 wipe=right}` | a **wipe**: the new picture slides in from one side |
| `::bg{src=... fade=1 through=#000000}` | **through a colour**: fade to the colour, swap, fade back |
| `::bg{src=... fade=1.2 mask=assets/masks/ink.png}` | following a **mask**, a greyscale picture |

- **Wipe.** `wipe` is `left`, `right`, `up` or `down`: the direction the edge travels.
- **Through a colour.** The stage fades to the colour over half of `fade`, the picture is
  swapped (no crossfade), and the colour fades away over the other half.
  - It waits for the first half, like `::fade{out}` does.
  - `through` on its own is black. It needs a `fade` above 0 and can't be combined with `wipe`.
  - Sprites are not touched: use the explicit `::fade` pair when sprites change too.
  - Use it on an uncovered screen: it ends uncovered.
- **Mask.** Over `fade` seconds the new picture appears first where the mask is darkest and
  last where it is white, with a soft edge between (see [Drawing a mask](#drawing-a-mask)).
  - The mask is stretched over the whole stage and stays put when the camera moves.
  - It works for pictures, colours and animations, like `wipe`.
  - `mask` needs a `fade` above 0 and can't be combined with `wipe` or `through`.
  - It is a plain picture file name: no `{variable}`, inside the game folder. `quillstone
    check` reads it like any picture and `package` ships it.
  - Like a wipe, it runs by itself while the story goes on. Rollback and loading show the
    finished background.

**Blur.** `::bg{src=room.png blur=12 fade=1}` blurs the background. `blur` is a radius in pixels
of the 1920x1080 stage, from 0 (sharp, the default) to 64.

- A `::bg` is sharp unless it says `blur=`. To blur or sharpen the picture that is already
  there, give `::bg` again with the same `src` and a new `blur`. With a `fade` the two looks
  crossfade; without one it changes at once.
- It works for pictures and animations. A video background blurs too, but more roughly. A
  solid colour has nothing to blur.
- The blur is part of the scene: rollback, saves and hot reload keep it.

### Characters

```
::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}
```

This declares a character: the name-plate text and colour, and a sprite path where `{pose}` is
filled in. Declare a character before using it. `**Mika:**` lines pick up the name and colour
automatically.

**A name from a variable.** `::character{id=hero name="{hero.name}" color=#9cc8ff}`: the name
may hold `{variable}`s of declared variables (an unknown one is an error).

- The name plate, History and the text-mode output show the variable's value at that line. So
  a character the player named (see `::input`) is shown by the typed name, and earlier lines
  keep the name they had after a rollback or a variable change.
- What the player typed never becomes formatting.
- Before a line, write `**hero:**`, or the name text exactly as declared: `**{hero.name}:**`.
- In a translation file, the `@ Speaker` context for such a character is the name as written
  in the script (`@ {hero.name}`), and the name's own entry is `> {hero.name}`.
- Auto-voice file names do not depend on the variable.

**Layers.** A character can be drawn from layers instead of whole poses (see
[Layered sprites](#layered-sprites)):

```
::character{id=mika name=Mika layers=body,eyes,mouth layer_sprites=assets/mika/{layer}_{value}.png}
```

`layers` lists the layer ids back to front (at most 32). `layer_sprites` is where each layer's
pictures are: `{layer}` is the layer id, `{value}` what is shown on it. A character may have
`sprites=` poses, layers, or both.

**A portrait.** A face shown by the textbox while the character speaks:

```
::character{id=mika name=Mika portrait=assets/mika_face.png}
```

- It shows while a line by that character is on screen, and nowhere else: narration and
  characters without a `portrait` show none.
- It is one picture (`.png`, `.jpg` or `.webp`). A `{...}` in the path is an error.
- It does not depend on the stage: no `::show` is needed, so it works for a phone call or a
  close-up where the character is not on stage.
- It follows the textbox: it is hidden with it (the `U` key, `::textbox{hide}`) and fades with it.
- Where it goes is the theme's `portrait_rect` (see [Themes](#themes)). `quillstone check`
  looks for the file; `package` ships it.

### Showing sprites

`::show` puts a sprite on stage, or changes one that is there. Whatever a later `::show`
doesn't give keeps its value.

| Write | What it shows |
| --- | --- |
| `::show{mika happy x=20% fade=0.4}` | a character in a pose; a new pose crossfades and keeps its position and size |
| `::show{src=assets/ghost.png id=ghost x=center}` | a sprite that isn't a character |
| `::show{mika anim=blink x=20%}` | a sprite whose picture is an **animation** |
| `::show{mika body=casual eyes=open mouth=smile}` | a **layered** sprite |
| `::hide{mika fade=0.5}` / `::hide{all}` | removes one sprite, or all |

- **An animation** (see [Frame animations](#frame-animations)). `mika` is the sprite's name; a
  character's name finds that character's sprite. The character's `sprites=` pattern and poses
  aren't used, so `anim=` can't be combined with a pose or with `src=`. Everything else (`x`,
  `scale`, `flip`, `y`, `z`, `fade`, `::move`, `::hide`) works as for any sprite.
- **A layered sprite** (see [Layered sprites](#layered-sprites)). Each layer id is an attribute
  whose value says what that layer shows: `eyes=none` hides it, `eyes=anim:blink` uses an
  animation.

What a sprite can be given:

| Attribute | Values | What it does |
| --- | --- | --- |
| `x` | `left` `center` `right`, a number or `30%` | where it stands across the screen |
| `y` | `top` `center` `bottom`, a number or `N%` | how high it stands |
| `z` | a whole number, -100 to 100 (default 0) | which sprite is in front |
| `scale` | 0.05 to 10 (default 1) | its size |
| `xscale`, `yscale` | 0.05 to 10 each (default 1) | stretch along one axis |
| `flip` | `flip`, or `flip=false` | mirror it left to right |
| `alpha` | 0 to 1 (default 1) | make it see-through |
| `xoffset`, `yoffset` | -3840 to 3840 pixels | shift it after `x` and `y` placed it |
| `blur` | 0 to 64 (default 0, sharp) | put it out of focus |
| `tint` | `#rrggbb`, `#rgb` or `none` | multiply its colours |
| `brightness` | -1 to 1 (default 0) | add that much to each colour after the tint |
| `crop` | `x,y,w,h` (each 0 to 1 or `N%`), or `none` | show only that part of the picture |
| `blend` | `normal` (default) or `add` | make it a light |
| `fade` | seconds | fade it in, or crossfade to the new pose |

- **`x` and `y`** are explained in [Placing things](#placing-things-x-and-y). A new sprite is
  centred and stands on the bottom (`y=bottom`). The screen cuts off what hangs outside.
- **`z`.** Sprites with a higher `z` are drawn in front, and equal ones in the order they were
  shown. `z` only orders sprites among themselves: the background stays behind them, and the
  textbox and menus stay on top.
- **`xscale` and `yscale`** stretch on top of `scale`. The sprite keeps its `x` and `y`
  placement, worked out again for its new size.
- **`alpha`** works on top of `fade=`, which fades the sprite in and out by itself.
- **`xoffset` and `yoffset`** are pixels of the 1920x1080 stage (negative: left, up), applied
  **after** `x` and `y` have placed the sprite. So `x=left xoffset=-100` puts it 100 px past
  the edge.
- **`blur`** is a radius in pixels of the 1920x1080 stage, whatever the sprite's size. Use it
  for a dreamlike flashback, or to put a character out of focus while another speaks. The blur
  stays inside the picture's own frame (nothing glows past it). So leave a few transparent
  pixels round the art of a sprite you blur: a figure that touches the edge of its file stays
  sharp there.
- **`tint`** multiplies the sprite's colours by the colour: `#ffffff` leaves it as it is and
  `#000000` makes a black silhouette (transparency is kept). `tint=none` takes it off.
  - Use it to make one character match the scene (night, sunset). `::tint` washes the whole
    stage.
  - It works for every kind of sprite; for a layered sprite it covers the whole stack.
  - It combines with the dimming of `speaker_highlight` and with `::filter`.
- **`brightness`** adds its number to each colour channel after `tint` has multiplied them
  (clamped at black and white), so `tint=#ffd8b0 brightness=0.1` is warm and brighter, and
  `brightness=-0.3` darkens. It works for every kind of sprite, whole stack included.
- **`crop`** shows only a rectangle of the picture, given as fractions of it (`0.25,0,0.5,0.4`)
  or percentages (`25%,0,50%,40%`); `crop=none` shows all of it again. The sprite is drawn at
  the cropped part's size times `scale`, so a crop with a bigger `scale` is a close-up:

  ```
  ::show{mika crop=25%,5%,50%,40% scale=1.6 y=0.6}
  ```

  For a layered sprite the one rectangle crops every layer.
- **`blend=add`** adds the sprite's picture to what is under it instead of covering it, so it
  only ever brightens. Use it for lamps, fire, god rays and neon: see
  [Light overlays](#light-overlays). It works with everything else a sprite has (`alpha`, `z`,
  `fade`, `::move`, `tint=`, `blur=`, `::hide`).

`::move` can animate `x`, `y`, the scales, `alpha`, the offsets, `blur`, `tint`, `brightness` and `crop`. It cannot
animate `z` or `blend`, which are switches: change them with `::show`.

### Moving sprites

`::move` animates a sprite from where it is to the values you give, over `time` seconds. The
values are the ones `::show` sets, and they can be combined in one move.

| Write | What it does |
| --- | --- |
| `::move{mika x=80% time=0.8}` | slide it sideways |
| `::move{mika y=0.9 time=0.3 ease=out}` | slide it up or down |
| `::move{mika scale=1.2 time=0.5 ease=out}` | grow or shrink it |
| `::move{mika xscale=1.4 yscale=0.7 time=0.5}` | stretch it along one axis |
| `::move{mika alpha=0 xoffset=-60 time=0.5}` | fade it to an opacity, slide it by pixels |
| `::move{mika blur=16 time=0.8}` | go out of focus (`blur=0`: back to sharp) |
| `::move{mika tint=#000000 time=1}` | fade its tint to the colour (`tint=none`: back to natural) |
| `::move{mika brightness=-0.4 time=1}` | dim it, or brighten it (`brightness=0`: as drawn) |
| `::move{mika crop=25%,5%,50%,40% scale=1.6 time=1}` | zoom in on a part of the picture (`crop=none`: all of it again) |
| `::move{eve x=30% time=1 then x=70% time=1}` | a **sequence** of steps (see [Move sequences](#move-sequences)) |
| `::move{mika y=0.85 time=1.2 loop}` | a **looping move** (see [Looping moves](#looping-moves)) |

- **`ease`** is `inout` (the default: slow at both ends), `in`, `out` or `linear`.
- **`flip`** (or `flip=false`) can be added to a move too, but a mirror can't be animated: it
  switches at the **start** of the move.
- **`alpha=0`** leaves the sprite on stage: `::hide` it when it should be gone.
- **`tint`**: each of red, green and blue eases on its own.
- **`blur`** costs a few extra texture reads per screen pixel it covers. It is fine for a
  sprite or a background, but not worth covering the whole screen with many blurred layers.
- **A looping move** goes to the target, back to where it started, and forth again, until
  something else moves the sprite.

### Camera, shake and flash

None of these wait: the story goes on while they run.

**`::camera{zoom=1.3 x=0.3 y=0.4 time=4}`** pans and zooms the stage: the background and the
sprites. The textbox and menus stay put.

- `zoom` is 1 to 4.
- `x` and `y` place the view over the stage the same way as a sprite (`left` `center` `right`,
  `top` `center` `bottom`, a number or `N%`), but only from 0 to 100%: the view never leaves
  the stage.
- Anything left out stays as it is. `ease=` works as for `::move`.
- `::camera{reset time=1}` goes back to the whole stage.

**`::shake{time=0.4 strength=20}`** shakes the stage, dying away over `time` seconds.

- `strength` is in pixels at 1920x1080, up to 200.
- While it lasts, the stage is zoomed in slightly about the centre, by just enough to hide the
  edges, easing in and out with the shake.
- `axis=x` shakes sideways only, `axis=y` up and down only, and `axis=both` (the default) both
  ways. A one-axis shake moves the stage by up to `strength` pixels along its axis and not at
  all along the other, and zooms in only as far as that axis needs.

**`::flash{time=0.4 color=#ffffff}`** flashes the whole screen: the colour jumps to full
strength and fades out over `time` seconds.

- `time` is more than 0, up to 10 (default 0.4). `color` is `#rrggbb` or `#rgb` (default
  white).
- It is drawn over the stage and inline images, but under the textbox and menus.

### Fading the stage out and in

**`::fade{out time=0.5 color=#000000}`** dips to a colour. The whole stage (background,
sprites, inline images, video and camera) fades to the colour and **stays covered**.

- `time` is more than 0, up to 10 (default 0.5). `color` is as for `::flash` (default black).
- It waits until the fade is done, like `::wait`. A click, or skip mode, ends it at once and
  the story goes on.
- The textbox, choices and menus are not covered, so a line of narration works over a black
  screen.

**`::fade{in time=0.5}`** fades the cover away over `time` seconds. It doesn't wait. With
nothing covering the screen it does nothing.

### Messages and unlocks

**`::notify{text="Saved" time=3}`** shows a small message in the top right corner, on the
theme's panel colour and text colour. It fades in, stays, and fades out.

- `time` is 0.5 to 30 seconds (default 3).
- `text` can hold `{variable}` values and the same markup as a `::hud` text (`**bold**`,
  `{color=...}`). The values are filled in when the command runs.
- `delta=` adds a signed amount after the text, coloured by its direction: `::notify{text="Trust" delta=2}`
  shows "Trust +2" and `delta=-1` shows "Trust -1". It is a whole number, a variable or an
  expression (`delta="gain - cost"`), worked out when the command runs; a `0` shows as "±0" in
  the text's own colour, and a value that isn't a whole number shows no amount. The colours are
  the theme's `notify_up_color` (default green) and `notify_down_color` (default red), see
  [Themes](#themes). The amount is not translated.
- It doesn't wait and doesn't stop the story. A new `::notify` replaces one still showing.
- It is a passing message: not saved, not shown again by rollback, and it ends at once when
  the player skips.
- Its `text` is translated like a dialogue line (looked up before the `{variable}` values go
  in).

**`::unlock{src=assets/cg1.png}`** unlocks a `gallery:` or `music_room:` entry for the Extras
screen without showing or playing it; see
[Kept across playthroughs](#kept-across-playthroughs). It doesn't wait or show anything.

### Effects over the whole stage

Each of these has its own section. None of them wait.

| Command | What it does |
| --- | --- |
| `::particles{snow}` / `{rain}` / `{src=assets/petal.png}` | ambient weather over the picture: snow, rain, or any small picture of yours falling and drifting |
| `::tint{color=#203060 alpha=0.35 fade=1}` | a colour wash that **stays** until it is changed or removed |
| `::vignette{strength=0.6 fade=1}` | dark edges and corners with a clear middle; it **stays** too |
| `::filter{gray=1 fade=2}` | a colour filter over the whole stage; it **stays** too |

`{none}` (or `{off}`) takes each of them away, and takes only `fade`.

**`::particles`** (see [Ambient particles](#ambient-particles)):

| Setting | Values | Default |
| --- | --- | --- |
| `amount=` | how many are on screen, 1 to 1000 | 60 |
| `speed=` | 0.2 to 5 | 1 |
| `wind=` | -2 to 2 (negative blows left) | 0 |
| `fade=` | seconds to ease in or out, 0 to 10 | 1 |
| `layer=` | `front` (over the sprites) or `back` (behind them) | `front` |

**`::tint`** (see [Colour wash](#colour-wash)):

| Setting | Values | Default |
| --- | --- | --- |
| `color` | `#rrggbb` or `#rgb` (required) | |
| `alpha` | how strong it is, 0 to 0.9 (`35%` also works) | 0.4 |
| `fade` | seconds to ease from the current tint, or from none, 0 to 10 | 0: at once |

`alpha` never goes above 0.9, so the story stays visible.

**`::vignette`** (see [Vignette](#vignette)) is for tension, a dream, a memory or someone
passing out. At least one of `strength`, `size` and `color` is required:

| Setting | Values | Default |
| --- | --- | --- |
| `strength` | how dark the corners get, 0 to 1 | 0.5 |
| `size` | how far in from the edges it reaches, 0 to 1 | 0.5 |
| `color` | `#rrggbb` or `#rgb` (red for an injury, white for a dream) | black |
| `fade` | seconds to ease from the vignette on screen (all three settings), or from none, 0 to 10 | 0: at once |

A `size` of 0 darkens little more than the corners; 1 reaches in from every edge.

**`::filter`** (see [Colour filter](#colour-filter)). At least one of the first five is
required:

| Setting | Values | Default |
| --- | --- | --- |
| `gray`, `sepia` | how far to drain the colour or turn it brown, 0 to 1 | 0 |
| `brightness`, `contrast`, `saturation` | 0 to 3 | 1: no change |
| `fade` | seconds to ease from the current filter, or from none, 0 to 10 | 0: at once |

For a vignette and a filter, what you leave out goes back to its default: a new one replaces
the old one.

### Hiding the dialogue box

`::textbox{hide}` hides the whole dialogue box (textbox, name plate, line and quick menu) for a
clean picture. `::textbox{show fade=0.5}` shows it again.

- `fade` is a number of seconds above 0, up to 10 (default: at once). It doesn't wait.
- A hidden box comes back by itself with the next line or choice, so the story never moves on
  unseen. Clicks still advance while it is hidden.
- On a `::hotspots` screen it stays hidden, and the line that is pending isn't drawn either,
  which gives a clean map. Show it again with `::textbox{show}`, or just say a line.
- Rollback, saves and hot reload keep it.

### Full-screen pages

For a letter, a diary or an opening narration, `::page` makes the lines stack up on a full-screen
page instead of replacing each other in the textbox. `::page{off}` brings the textbox back.

```markdown
::page

Dear Mika,

The lamp on the roof still works.

**Mika:** She always wrote like this.

::page{off}
```

- `::page` starts an empty page, and the lines written after it are page lines. Writing `::page`
  again is a page break.
- The mode follows the file, read from top to bottom, not the story's path. A jump into the
  middle of a page passage shows its lines on a page that starts empty; a jump out shows a normal
  textbox line. An included file starts in textbox mode, and the including file's mode goes on
  after the `::include`.
- Each line is a paragraph on a dimmed screen. A line with a speaker starts with the name in the
  speaker's colour. There is no textbox, name plate or portrait.
- Each line waits for the player as in the textbox: advance, auto, skip, `{nw}`, voice,
  the backlog and read-tracking all work per line. Only the newest line types out.
- When the next line doesn't fit the page, it starts a new screen page. A page also holds at
  most 64 lines, after which the next line starts a fresh page.
- The page is cleared by `::page`, by a textbox line, by a [centred line](#centred-lines) and by a `::video`. Choices, hotspots and
  other screens don't clear it: a choice asked after page lines appears over the page, and
  the page's text dims while the choice is up so the buttons can be read.
- `::textbox{hide}` and the `U` key hide the page too.
- Rollback, saves and hot reload keep the page. `quillstone check` warns about a page line that
  is too long for a page by itself.
- The page's area and dimming are set in the theme (`page_rect`, `page_color`).

### Centred lines

A title card or a bit of narration shown on its own, in the middle of the screen:

```
::bg{src=street.png}

::center Three days later…

**Mia:** You're back.
```

- `::center` is followed by the text, which runs to the end of the line. It takes the usual
  inline markup (`{u}`, `{color=..}`, `{w}`, `{nw}`, `{variables}`) like narration. No text, or a
  `{...}` attribute block right after `::center`, is an error.
- The textbox, name plate and quick menu are hidden for that line. The text is drawn centred on
  the screen, wrapped at 1400 px, in the theme's text font, size and colour, with its outline.
  It types out at text speed and waits for a click like any line.
- The next ordinary line brings the textbox back, and so does a choice.
- It is a line in every other way: it goes into the backlog (with no speaker), rollback and saves
  work on it, it has a translation entry like narration, voice and `{nw}` work on it, and skip,
  auto-forward and read-tracking count it.
- Over a [full-screen page](#full-screen-pages) it clears the page, as a textbox line does. It is
  not a page line.
- `quillstone check` warns about a line that is too long for the screen at that width.

### Inline images

`![alt](assets/x.png)` is an inline image. It appears **with the next line that waits** and
disappears when the player advances, so put it *before* the line it illustrates.
`![](x.webm)` plays a video.

### Drawing a mask

A mask is an ordinary greyscale picture (PNG, JPEG or WebP; only its brightness counts). Where
it is **dark** the new background appears **first**; where it is **white** it appears **last**.

- **Smooth gradients give soft spreads.** A radial gradient opens like an iris, an angle
  gradient around the centre is a clock wipe, and blotchy noise looks like ink spreading.
- **A hard-edged picture gives shapes**, such as stripes or a shatter pattern in several greys,
  which appear one after the other.
- Every pixel goes from the old picture to the new one over a short stretch of the transition
  (a tenth of it), so even a hard edge is slightly soft.
- To run a mask the other way round, invert the picture.
- It is stretched over the whole stage, so draw it at the stage's 16:9 shape. A small one like
  512x288 is enough.

### Placing things: `x` and `y`

One model places sprites (`::show`, `::move`), HUD elements (`::hud`) and the camera
(`::camera`). `x` and `y` each take a **keyword**, a **number** or a **percentage**:

- `x`: `left` (0%), `center` (50%) or `right` (100%). `y`: `top` (0%), `center` (50%) or
  `bottom` (100%).
- A plain number is a fraction, not pixels: `0.25` is the same as `25%`, as everywhere else in
  the language.

**The rule.** The point `N%` of the way **across the object** is put `N%` of the way across
the **screen**. So `x=0` is flush with the left edge, `x=right` flush with the right edge, and
`x=25%` a quarter of the way along the free room.

- **From 0% to 100%** the object stays wholly on screen whatever its size. The HUD and the
  camera are held to that range.
- **Outside it**, a sprite moves by whole screens. `x=-100%` puts its left edge one screen
  width left of the screen's, so a sprite no wider than the screen is wholly off to the left;
  `x=200%` puts it wholly off to the right. That is how you slide one in or out. Sprites may
  go from -200% to 300%, and `y` works the same way.
- A new sprite is placed `x=center y=bottom`. A sprite you show or move again keeps what you do
  not give.
- **There is no anchor.** This model cannot say "the centre of this sprite exactly at that
  spot", because where the centre ends up depends on the picture's size. A sprite as tall as
  the screen has no free room, so `y` does nothing to it until you give it a smaller `scale`.

**Sprite size.** Sprites are about 940 px tall at `scale=1` (on a 1920x1080 screen) and stand
on the bottom edge unless `y` says otherwise. The text box draws over their lower part. To
size them by their pictures instead, see the next section. To put one sprite behind another,
give it a lower `z`.

**What rollback and saves restore.** The camera, sprite sizes, heights, stretch, mirroring, `z`
and the colour cover are part of the scene, so rollback and saves restore them.

- A save made before `::fade` existed loads uncovered. One made before `flip` and the scales
  existed loads with unflipped, unstretched sprites.
- A shake, a flash or a `::notify` is a moment and isn't replayed.
- A flash stands still while a menu is open and ends at once when the player skips.

**The `::fade` cover.**

- It is drawn over the stage, inline images, video and hotspot pictures. It is under a
  `::flash`, the HUD elements, the textbox and everything else on the interface.
- Rollback goes past the wait of a `::fade{out}` (like a `::wait`), so going back never stops
  halfway through a fade. Roll forward does the same.
- The last line stays on screen while the screen fades out. Hide the box first
  (`::textbox{hide}`) for a clean fade.

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

`::particles` is cheap weather: no spawning, no physics, never more than 1000 particles.

```
::particles{snow}
::particles{rain amount=120 wind=0.6}
::particles{src=assets/petal.png amount=24 speed=0.9 wind=0.3}
::particles{snow layer=back}
::particles{none fade=2}
```

- **Snow** is small soft white squares of a few pixels that sway as they fall. They have
  different sizes: the bigger ones are nearer, so they fall faster and are brighter.
- **Rain** is thin, faint streaks that fall fast and lean with the wind.
- A **picture** (`src=`, a .png, .jpg or .webp inside the game folder; no `{variable}` in the
  name) falls and drifts like snow, at its own pixel size scaled a little for each particle,
  and without turning. Keep it small: 32 px or so is plenty, and `quillstone check` warns about
  one over 256 px.
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
sick green or a sepia memory. To colour a single sprite instead, give it `tint=` (see `::show`).

```
::tint{color=#203060 alpha=0.35 fade=1}
::tint{color=#8a6a3a alpha=0.5}
::tint{none fade=1}
```

- It is one flat colour at `alpha` opacity, so it does not remove colour: a grey or sepia tint gives
  a flashback look, but the picture is not truly drained, and bright parts stay bright. For that, use
  `::filter` (next section). There are no blend modes.
- A new `::tint` eases from the one on screen (colour and strength), or from nothing, over
  `fade`. `::tint{none fade=1}` eases the wash away. Removing a tint that isn't there does nothing.
- It is drawn over the background, sprites, particles, inline images and video, and under the
  `::fade` cover, `::flash`, the HUD, the textbox, choices and every menu, so the text is never
  tinted. It belongs to the screen: the camera and `::shake` don't move it.
- The colour and strength are saved, so rollback, saves and replays bring the tint back at once
  (without its fade). A save from before `::tint` loads without one. `{variable}` isn't supported in it.
- `quillstone run` prints `[tint: #203060]` and `[tint off]`.

## Colour filter

`::filter` changes the colours of the whole picture and keeps them changed, for a black-and-white
flashback, a faded memory or a scene that is slowly drained of colour.

```
::filter{gray=1 fade=2}
::filter{sepia=0.8 contrast=1.1}
::filter{saturation=0.4 brightness=0.8 fade=1}
::filter{none fade=1}
```

- `gray` and `sepia` go from 0 (none) to 1 (full).
- `brightness`, `contrast` and `saturation` go from 0 to 3, where 1 changes nothing.
  `saturation=0` is the same as `gray=1`, `brightness=0` is black, and `contrast=0` is flat
  grey.
- They are applied in that order, gray first. A number can also be a percentage (`gray=50%`).
  `fade` is in seconds, 0 to 10.
- At least one setting is required. The ones you leave out take their default, so a new
  `::filter` **replaces** the old one rather than adding to it.
- A new `::filter` eases from the one on screen over `fade` (each setting moves on its own), or from
  none. `::filter{none fade=1}` eases the colours back. Removing a filter that isn't there does nothing.
- It covers the background (pictures and solid colours), sprites, particles, inline images, video and
  hotspot art. It does **not** cover a `::tint`, the `::fade` cover, `::flash`, the HUD, the textbox,
  choices or menus, so the text keeps its colours and a tint sits on top of the filter: a gray filter
  with a blue `::tint` is a valid look. The camera and `::shake` move the pictures, not the filter.
- The settings are saved, so rollback, saves and replays bring the filter back at once (without its
  fade). A save from before `::filter` loads without one. `{variable}` isn't supported in it. The
  small thumbnails of a save slot are not filtered.
- `quillstone run` prints `[filter: gray=1]` (only the settings that are not at their default) and
  `[filter off]`.

## Light overlays

A picture shown with `blend=add` is added to the colours under it instead of covering them, so a
soft glow brightens the scene the way a lamp, a fire or a neon sign does. Draw the picture on black
or on a transparent background, bright where the light is: black adds nothing, and the brighter a
pixel, the more it lights what is behind it.

```
::show{glow src=assets/lamp_glow.png x=70% y=40% blend=add alpha=0.8 z=5}
::move{glow alpha=0.5 time=0.3 ease=inout loop}
```

- A light is a sprite like any other, so `x`, `y`, `scale`, `z`, `fade`, `alpha`, `::move` (also with
  `then` and `loop`, which is how a flicker is made), `blur=`, `tint=` and `::hide` work on it. `alpha`
  is how strong the light is; `tint=` changes its colour. Give it a `z` above the pictures it should
  light, and below the ones it should not (a character in front of the lamp).
- The colours never go beyond white, so several lights on the same spot saturate rather than wrap.
- `blend=` is kept like the other sprite values when a later `::show` doesn't give it, and it is
  saved, so rollback, saves and replays bring the light back at once. A save from before `blend=`
  loads with normal sprites.
- `::filter` colours a light like any other picture, and the small thumbnails of a save slot show the
  light too.

## Vignette

`::vignette` darkens the edges and corners of the picture and leaves the middle clear, for tension, a
dream, a memory or a character passing out.

```
::vignette{strength=0.6 fade=1}
::vignette{strength=0.8 size=0.7 color=#600000 fade=0.5}
::vignette{none fade=2}
```

- `strength` is how dark the corners get (0 is none, 1 is solid colour). The middle of each edge is
  lighter than a corner, and the dark fades out smoothly towards the centre, following the shape of
  the screen. `size` is how far in it reaches: at 0 only the corners are touched and the rest of the
  picture is clear, at 1 the dark comes in a long way from every edge.
  `color` is what it is dark with: black by default, red for an injury, white for a dream.
- A new `::vignette` replaces the old one: what you leave out goes back to its default. It eases from
  the one on screen (all three settings), or from nothing, over `fade`. `::vignette{none fade=2}`
  eases it away. Removing a vignette that isn't there does nothing.
- It is drawn over the background, sprites, particles, inline images, video, lights and the
  `::tint` wash, and under the `::fade` cover, `::flash`, the HUD, the textbox, choices and every
  menu. It belongs to the screen: the camera and `::shake` don't move it, and `::filter` does not
  colour it.
- The settings are saved, so rollback, saves and replays bring the vignette back at once (without its
  fade). A save from before `::vignette` loads without one. `{variable}` isn't supported in it. The
  small thumbnails of a save slot don't show it.
- `quillstone run` prints `[vignette: strength=0.6 size=0.5]` and `[vignette off]`.

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

A whole number works too: with `floor: 2`, `::bg{src="assets/room_{floor}.png"}` shows
`assets/room_2.png`.

**Where it works**

- The pose of `::show`: `::show{mika {mood}}`, also joined to other letters, like `a_{mood}`.
  It goes through the character's `sprites=` pattern exactly like a written pose.
- `src=` of `::show`, which then needs a sprite name: `::show{mika src="x/{mood}.png"}`.
- `src=` of `::bg`, and inline images.
- The value of a layer of a layered sprite: `::show{mika eyes={mood}}` (see
  [Layered sprites](#layered-sprites)).

Sounds, voices, videos, HUD pictures, item icons, animation frames and the gallery take plain
file names only (a brace there is an error). `anim=` and `color=` take no variables.

**The variable**

- It is a text variable or a whole number. Dotted names work (`{hero.mood}`).
- Only a whole `{name}` is replaced: no spaces inside the braces, no calculations.
- A variable that is not declared in `vars:` is allowed: a `::set` may create it, and
  `quillstone test` warns when nothing ever sets it. A declared `true`/`false` or list is an
  error.

**The name**

- It needs a written-out extension: `.png`, `.jpg`, `.jpeg` or `.webp`, nothing else.
- At most eight variables fit in one name.
- A name with a `{variable}` can only be a picture, not a video.
- Braces cannot be part of any file name: there is no way to write a literal one.

**What a value may be.** The text is put into a file name, and players can type text
(`::input`) or share saves, so only a safe fragment is used:

- 1 to 64 characters, made of letters (any alphabet), digits, `_`, `-`, spaces and dots;
- a dot or a space is never the first or last character, and `..` never appears;
- a number spells its digits (`0` and up, as in
  [Looking up by a variable](#looking-up-by-a-variable)).

Anything else does not make a path at all: a `/`, `\`, `:`, a control character, an empty text,
a longer one, a name Windows keeps for devices such as `con` or `nul`, a negative number, or
an unset variable. The picture is then treated as a file that is not there. It draws nothing,
and the game shows a short notice once, saying which variable and picture name were involved
(it never prints the value). Nothing outside the game folder can ever be reached this way.

**When the name is built.** At the moment the command runs, from the variables as they
are then. The game, its saves and rollback keep the finished file name, so a save
loads as before and going back shows the picture that was shown.

**`check`, `package` and loading.** A name with `{variable}` stands for every file it could
build. It is read as a pattern where each `{variable}` stands for one safe fragment inside its
own folder (never across `/`).

- `quillstone check` reports an error when a pattern matches no file
  (`assets/places/{place}.png matches no file`). It checks that every match is a picture the
  player can show, and warns when a pattern matches more than 200 files.
- `quillstone package` ships **every** file a pattern matches. So keep unrelated files out of
  the folder, or name them so the pattern cannot match them: `places/rooftop.png` is matched
  by `assets/places/{place}.png`; `assets/places/old/x.png` and `assets/places/notes.txt` are
  not.
- The game cannot know the value ahead of time, so it loads these pictures when they are first
  shown instead of in advance. A short pause the first time a picture appears is possible with
  very large files.

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
- A frame may be `none` instead of a picture: nothing shows for that hold. It makes an overlay
  that is hidden most of the time, such as a blink or a twinkle. An animation made only of
  `none` frames is an error.
- `loop` (on its own) starts over from the first frame after the last one. Without it
  the animation plays once and **stays on its last frame**.
- A one-frame animation is allowed: it is just a picture.
- `anim=` goes on `::show` (a sprite) and `::bg`. Anywhere else it is an error, and so
  is an animation that is not declared above the line that uses it.

An overlay that is hidden most of the time:

```
::anim{id=twinkle frames="none 6, assets/star.png 0.15, none 2, assets/star.png 0.15" loop}
```

An animation sprite is an ordinary sprite: it can be moved, scaled, flipped, faded, put
behind another, given a `y`, hidden, rolled back and saved.

- Showing another picture on the same sprite (`::show{mika happy}`) replaces the animation
  with a crossfade, and the old animation keeps playing while it fades out.
- Frames may differ in size; each is drawn as a sprite (or background) picture of its own size.
- Frames of an animation are not crossfaded into each other.

**Timing.** The animation starts at its first frame when it starts being shown.

- Time counts in seconds of the story's own clock, the same as fades and waits. It stands
  still under Settings, History, the save screens and dialogs. Skip mode does not speed it up:
  it runs at normal speed while lines go by.
- Where an animation is in its run is not part of a save or of rollback. After loading a save,
  rolling back or reloading the script, it starts again from its first frame.
- Saves remember *which* animation each sprite or background shows. A save that names an
  animation the script does not declare is refused, like one naming a file outside the game
  folder. A hot reload instead drops the picture of an animation you removed: a background
  goes, a sprite leaves the stage.

**Cost.** Every frame is loaded as an ordinary picture, all of them together, a few lines
before the animation can appear and for as long as it is on screen. A short loop of well-sized
pictures is best.

- `quillstone check` reports a frame that is missing or is not a picture as an error. It warns
  about an animation of more than 64 frames, or a frame bigger than 2048 pixels on a side.
- `quillstone package` includes every frame.

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

- `layers` is a comma-separated list of layer ids: letters, digits and `_`, at most 32, no
  repeats. An id can't be `none`, or the name of a `::show` attribute such as `x` or `scale`.
- `layer_sprites` must contain both `{layer}` and `{value}` and end in a picture type
  (`.png`, `.jpg`, `.jpeg` or `.webp`). With `layer_sprites=assets/mika/{layer}_{value}.png`,
  `eyes=open` shows `assets/mika/eyes_open.png`.
- A `::show` names the layers it changes. **A layer you leave out keeps what it showed**, and so
  do the sprite's position, size and everything else.
- The first `::show` of a character with layers must give a value for every layer. The script
  is refused otherwise, and the error lists the missing ones. Use `none` for a layer that
  starts empty.
- A bare `::show{mika x=80%}` on a character that has only layers just moves the sprite.
- `eyes=none` hides that layer. `eyes=anim:blink` shows the frame animation `blink` (declared
  above, see "Frame animations") on that layer, so blinking is just an animated eyes layer; its
  clock runs on while other layers change.
- A value may hold `{variable}` parts like a pose does (`mouth={feeling}`, `body=outfit_{day}`);
  the same rules apply as in "Picture names from variables": the variable must be a text or whole-number variable, and
  a value that is not a safe file-name fragment shows nothing and gives a notice.
- **Lip flap.** `talk=mouth:anim:flap` on the `::character` names a layer and an animation
  (declared above the character). While a line by that character is typing, that layer shows
  the animation instead of its value; when the line is fully typed, skipped, or instant text, it
  shows its value again, so `::show{mika mouth=smile}` still changes the resting mouth.
  A `{w=}` pause in the middle of a line counts as typing. Only the speaking character's sprite
  flaps, not narration or the other characters, and a layer that is `none` stays hidden.
  `quillstone check` reports a layer the character does not have and an animation not declared.
  The animation id may hold `{value}`, one animation for each value the layer shows: while `mouth` is
  `smile`, `flap_smile` plays; a value with no such animation keeps its picture, and `check` warns
  when no declared animation fits any value the script shows.

  ```
  ::anim{id=flap frames="assets/mika/mouth_smile.png 0.15, assets/mika/mouth_open.png 0.15" loop}
  ::character{id=mika name=Mika layers=body,eyes,mouth layer_sprites=assets/mika/{layer}_{value}.png talk=mouth:anim:flap}
  ```

  ```
  ::character{id=mika name=Mika layers=body,eyes,mouth layer_sprites=assets/mika/{layer}_{value}.png talk=mouth:anim:flap_{value}}
  ```

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
  (`::show{mika happy}` is an error). A character with both `sprites=` and layers can be shown
  either way:
  - a pose replaces the whole sprite, and drops its layers;
  - layers on a sprite that was shown as a pose replace it, so all of them need a value.
- When a sprite is hidden and shown again, the layers start over. One with no value stays
  hidden and the game shows a notice (`quillstone test` reports it).
- Layers are part of the scene: rollback, saves and hot reload restore them.
- A save names each layer's value, and is checked like a sprite's picture. A path outside the
  game folder, an unknown animation or a layer the character does not declare is refused.
  After a script edit that removed a layer or an animation, the load drops it. Saves from
  before layers existed load as they were.

`quillstone check` looks at every layer picture the script can show, including every file a
`{variable}` value can build, and `quillstone package` ships them all. A demo is in
`examples/layers.quill.md`.

## Move sequences

Put `then` between the steps of one `::move` and they run one after another, in order, without holding up the story:

```
::move{eve x=30% time=1 then x=70% time=1 wait=0.5 then alpha=0 time=0.5}
::show{eve alpha=0 xoffset=-80}
::move{eve alpha=1 xoffset=0 time=0.6}
```

**Steps**

- Each step is a small `::move` of its own. It names what it animates (`x`, `y`, `scale`,
  `xscale`, `yscale`, `alpha`, `xoffset`, `yoffset`, `blur`, `tint`, `brightness`, `crop`), a `time` (default 0.5)
  and an `ease`.
- A step starts from where the step before it ended. A property a step leaves out stays as it
  is.
- Separate steps with spaces and `then`; commas are not separators. At most 32 steps.
- `wait=` (seconds, default 0) waits **before** the step starts. A step may be just a pause
  (`then wait=2`). A `wait=` on the first step delays the whole move.
- `flip` belongs in the first step: it switches at the start, as in any move.

**The values**

- `alpha` runs from 0 (invisible) to 1. It multiplies with the fade of `::show{fade=}` and
  `::hide{fade=}`.
- `xoffset` and `yoffset` are pixels of the 1920x1080 stage, added after the placement of `x`
  and `y`.
- `blur` is the radius in stage pixels (0 to 64): `::move{eve blur=20 time=1 then blur=0
  time=1}` goes out of focus and back.
- `tint` is a colour, as in `::show`: `::move{eve tint=#000000 time=1}` darkens her to a
  silhouette.
- `brightness` and `crop` are as in `::show`; a crop eases its four numbers, so
  `::move{eve crop=none scale=1 time=1}` pulls back from a close-up.

**While it runs**

- It never waits: the next line shows at once.
- A new `::move` cuts a running sequence short, and the sprite glides on from where it is. So
  does `::hide`, or a `::show` that sets position, size, `alpha`, an offset, `blur`, `tint`, `brightness` or `crop`.
- Without `then`, a move is the one-step sequence it always was.
- The values are part of the scene. A rollback, a redraw (changing the language, loading a
  save) and skip mode put the sprite at the **end** of the sequence with no animation, the
  same as for any move.
- A save also keeps the sprite's `alpha`, offsets, `blur` and `tint`. An older save loads with
  1, 0, 0 and no tint.

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
- It animates any mix of `x`, `y`, `scale`, `xscale`, `yscale`, `alpha`, `xoffset`, `yoffset`, `blur` and `tint`; a move that changes
  none of them (only `flip`, say) can't loop. `flip` may be added: it switches at the start.
- With `then` steps, `loop` repeats **all** the steps, in order, for ever (put it anywhere; the
  end reads best): `::move{lamp alpha=0.4 time=1 then alpha=1 time=1 wait=0.5 loop}` pulses a
  lamp.
  - The repeat starts from where the last step ended: that is the sprite's resting value.
  - A `wait=` in a step is a pause on every round.
  - A single step with `loop` still goes back and forth and can't `wait=`.
- "Where it started" is the sprite's resting values: what it had when the move was given.
  The loop doesn't change them, so a pose change (`::show{mika happy}`) keeps the loop going.
- The loop ends when the sprite gets another `::move`, a `::show` that **sets** `x`, `y`,
  `scale`, `xscale` or `yscale` (whether or not the value differs), or `::hide`.
  - A plain `::move` (without `loop`) glides from wherever the sprite is at that moment to its
    new values. Anything it doesn't name goes back to the resting value.
  - A second `::move{... loop}` replaces the loop.
- It never holds up the story: the next line shows at once, rollback goes back one line
  as always, and the last line of the story ends with the sprite still bobbing.

Like animations, where a loop is in its cycle is not part of the state: a save, rollback or
reload restarts the sprite at its resting values and heading for the target.

A loop stands still while a menu is open, and skip mode does not speed it up.

## Moving backgrounds

There is no separate background move. `::camera` already pans and zooms the stage, and it
eases over time, so a background that slides or zooms is a camera move. The stage is the
background and the sprites together, so the sprites move with it:

```
::bg{src=assets/street.png}
::camera{zoom=1.4 x=left y=center}              // close in on the left of the picture, at once
::camera{x=right time=20 ease=linear}            // a slow pan to the right
::camera{zoom=1.8 x=60% y=40% time=6 ease=out}   // then a slow zoom in
::camera{reset time=1}
```

- The view never leaves the stage, so a pan needs `zoom` above 1: the picture is then bigger
  than the screen, as for any pan. `zoom` goes from 1 to 4.
- The camera stays put through `::bg`, `::show` and `::hide` until you `::camera{reset}`.
- To keep the sprites still while only the background moves, there is no way: the camera moves them too.
- Like sprites, it is part of the scene: rollback, saves and a redraw put it at the end of the move.

## HUD: text, bars and pictures over the story

`::hud{...}` puts an always-on element over the story: a counter, a clock, a health bar, a
title card.

```
::hud{id=hp bar=hero.hp max=hero.max_hp x=0.02 y=0.04 size=320,26 color=#cc3333 back=#333333 border=#ffffff}
::hud{id=clock text="Day {day}, {hero.gold} gold" x=0.98 y=0.04 size=32 back=#00000099 pad=12 fade=0.3}
::hud{id=low text="LOW HP" x=0.02 y=0.11 color=#ff5555 if="hero.hp <= 3"}
::hud{id=heart image=assets/heart.png x=0.02 y=0.9 scale=0.5}
```

An element has an `id` (letters, digits and underscores). Using the same `id` again
**replaces** the element where it stands. Which kind it is depends on which attribute is
there, and exactly one of these is required:

| Attribute | Kind |
| --- | --- |
| `text="Day {day}, {hero.gold} gold"` | **Text** |
| `bar=hero.hp max=hero.max_hp size=320,26` | **Bar** |
| `image=assets/heart.png scale=0.5` | **Picture** |
| `box=#000000aa size=800,500` | **Box** |

**Text** takes the same tags and `{variable}` values as dialogue (`**bold**`, `*italic*`,
`{color=..}`, `{size=..}`), but not `{nw}` or `{p}`. The values are read **every time the
screen is drawn**, so the text follows the variables by itself. A text that comes out empty
(or only spaces) draws nothing, not even its `back=` panel, and a button of it can't be
pressed; the text you write must still have something in it.

| Attribute | Values | Default |
| --- | --- | --- |
| `size` | 8 to 300 | the theme's text size |
| `color` | a colour | the theme's text colour |
| `back` | a panel behind the text, e.g. `back=#00000099` | none |
| `pad` | the padding around the text in pixels, 0 to 200 | 0 |
| `width` | wrap the text at this many pixels of the 1920x1080 screen, 8 to 1920 | none: one line |

- Colours here may have an alpha part: `#rrggbbaa` or `#rgba`.
- A text element's width is estimated. Without `back`, the text is aligned to the side its `x`
  leans to (left, centre or right) inside that estimated box, so at `left`, `center` and
  `right` it is placed exactly.
- With `back`, the panel is as wide as the text is *expected* to be, a little generously, so
  it may be a few pixels wider than the text.
- With `width=`, a longer text wraps on word boundaries like the textbox, and the element is
  as tall as its lines; `back` and `pad` wrap around the whole block, and `x` and `y` place the
  block. A text button's hit area is the block. A text that fits keeps its own size. A bar,
  picture or box can't take `width=`.

**A bar** needs `size`: width,height in pixels of the 1920x1080 screen. `bar` and `max` are
whole-number expressions: a variable, a number, or arithmetic like `hero.max_hp + 2`. The bar
is filled `bar / max` of the way from the left, kept between empty and full. With `max` 0 or
less it is empty.

| Attribute | What it is | Default |
| --- | --- | --- |
| `color` | the fill | the theme's accent colour |
| `back` | the empty part | `#333333` |
| `border` | an outline colour, drawn outside the bar's size | none |
| `border_size` | 1 to 3 pixels | 2 |

**A box** is a plain rectangle of a colour (alpha allowed, like `#000000aa`), a backdrop for
other elements. `size=width,height` is required, in pixels of the 1920x1080 screen like a
bar's, and `x` and `y` place it as usual. It can't be a button, and it takes none of the
attributes of text, bars or pictures.

**A picture** is a `.png`, `.jpg`, `.jpeg` or `.webp` inside the game folder, like a hotspot
image. It is drawn at its own pixel size times `scale` (0.05 to 10, default 1). `quillstone
check` tests it as a picture and `package` includes it.

**Attributes for every kind:**

| Attribute | What it does |
| --- | --- |
| `x=right` `y=4%` | where the element is (default `left` and `top`) |
| `if="hero.hp <= 3"` | draw the element only while the condition is true |
| `fade=0.3` | fade in over that many seconds (more than 0, up to 10) when the element is created |
| `call=` `jump=` `open=` `hover=` | make a text or picture element a button: see "Buttons" below |

- **`x` and `y`** place the element as a sprite is placed (see
  [Placing things](#placing-things-x-and-y)). `x` is `left` `center` `right`, or a number or
  percentage from 0 to 100%; `y` is `top` `center` `bottom` or the same. The point that far
  across the element is that far across the screen, so the element is always wholly on screen.
  Because a text element's width is estimated, in between the three keywords it can be a few
  pixels off.
- **`if`** is checked every time the screen is drawn. `random` isn't allowed, as in any
  condition.

**Buttons.** A text or picture element (not a bar or a box) becomes a button with one of these:

| Attribute | |
| --- | --- |
| `call=#phone` | a press **calls** that label |
| `jump=#map` | a press **jumps** to that label |
| `open=journal` | a press opens a built-in screen |
| `hover=assets/phone_hover.png` | for a picture button: shown instead of the picture while the mouse is over it |

- **`call=`** calls the label and, when it ends with `::return`, shows the same line again
  (like an item's Use). The label must exist and take no parameters; `check` says which line
  is wrong.
- **`jump=`** leaves the line the story waits at and goes on at the label, with nothing to
  return to. Unlike `::jump`, it also leaves every `::call` and `call` region the story is
  inside, so no `::return` ever comes back to the scene it left. Use it for a button that leaves
  wherever you are, such as a map button. The label must exist and take no parameters, and a
  press needs no room on the call stack.
- **`open=`** opens a screen as the quick menu and keys do: `journal` (J), `inventory` (I),
  `history` (H), `save` (S), `load` (L), `settings` (the quick menu's Settings button) or
  `extras` (the Extras screen: gallery, music room and scene replays; closing it returns to the
  story). `journal` and `inventory` need a game that has one, and `extras` a `gallery:`,
  `bonus:`, `music_room:` or `replays:`. `messages` opens the [Messages](#messages) screen, and needs a game
  that uses `::message` or `::reply`.
- **`hover=`** needs `call=`, `jump=` or `open=`, and is checked and packaged like the picture. A text
  button simply lights up under the mouse.
- `call=`, `jump=` and `open=` can't be combined.

**When a button works.** Only while the story waits at a normal line (the same rule as an
item's Use). Over a choice, a timed choice, `::input`, `::wait`, a shop, a `{nw}` line, a
replay or while skipping, buttons are drawn dimmed and a press only says "Can't use that now".

**Clicking.**

- A click on a button never also moves the story on.
- A choice or hotspot under the same spot wins over the button, and so does the quick menu.
- **Keyboard and gamepad.** At a normal line with no choices or hotspots, Up / Down (the D-pad)
  pick out a button, first to last in the order you wrote them and round again; one hidden by
  its `if=` is skipped. The picked button looks hovered (a picture without `hover=` gets the
  theme's `button_hover_color` panel behind it).
- Enter or Space (the gamepad's A) presses the picked button, as a click does. With none
  picked they advance as usual.
- The pick goes away when the story moves, any other key is pressed, the mouse moves or
  clicks, or the button can't be pressed any more.
- The click area is the rectangle the element is drawn at (for text, its estimated box), so
  keep a button clear of the textbox.

**In the scene and the tools.**

- You show, hide and change a button with `::hud` like any element. It follows the scene
  through rollback, saves and hot reload. A save can only name a label some `::hud` of the
  script offers.
- `quillstone test` presses each `call=` and `jump=` button once, where it can be pressed, so its label is
  not reported as unreached.
- `quillstone graph` draws a dotted edge from a "HUD" node to each such label.

```
::hud{id=phone image=assets/phone.png hover=assets/phone_hover.png call=#phone x=right y=0.12}
::hud{id=log text="Journal" open=journal x=0.02 y=0.12 back=#00000099 pad=10}
::hud{id=map image=assets/gui/map.png jump=#map x=0.98 y=0.02}

# phone
::set{calls += 1}
Mika: *rrring* Hello? ... Oh, it's you.
::return
```

**Removing elements.** `::hud{hide=clock fade=0.3}` removes one element, with an optional fade
out; hiding an id that isn't there does nothing. `::hud{hide=st_*}` removes every element whose
id starts with `st_` (`fade=` works too; the `*` goes only at the end, after a prefix, and
nothing matching does nothing). `::hud{clear}` removes all of them. None of them waits.

**Hiding all of them for a while.** `::hud{off}` hides every element but keeps them;
`::hud{on}` shows them again exactly as they were (`fade=` works as above). Use it for a cutscene
that shouldn't have the clock and buttons over it:

```
::hud{off fade=0.3}
::bg{src=assets/ending.png}
Mika: Nobody is going to see this.
::hud{on fade=0.3}
```

- While the HUD is off, an element that is created or replaced is kept but not drawn until
  `on`, and `hide=` and `clear` still remove elements.
- Buttons can't be clicked or picked with the keyboard while it is off.
- `off` when it is already off, and `on` when it isn't, do nothing.
- The textbox, the quick menu and the line are not part of the HUD: `off` doesn't touch them.
- Saves and rollback keep it, as they keep the elements.

**Where they are drawn.** Elements are drawn in the order they were created; an element that is
replaced keeps its place. They are above the background, sprites, inline images, video, hotspot
pictures and the colour cover and flash. They are below the textbox, quick menu and every other
piece of interface. That is why a title card can sit on a black screen:

```
::bg{color=#000000}
::hud{id=card text="{size=96}Chapter 1{/size}" x=center y=0.4 fade=1}
::wait{time=3}
::hud{hide=card fade=1}
```

They are part of the story's interface:

- They are not drawn on the main menu, nor under the full-screen screens (Settings, History,
  Save, Load, Extras, the shop, the inventory and text input).
- They **are** drawn (dimmed) under a confirmation question over the story.
- The `U` key hides them together with the textbox; `::textbox{hide}` does not.
- Text in a `{color=..}` tag doesn't fade with the element.
- Putting an element in a text-heavy corner is up to you: the textbox and the quick menu are
  drawn over it.

**Saves and testing.** Elements, like the background and sprites, are part of the scene:
rollback, saves and hot reload restore them. A save from before `::hud` existed shows none, and
fades are not saved. `quillstone test` ignores `::hud` itself but counts the variables its
text, `bar`, `max` and `if` read, so a typo is reported as a variable that is never set.

`examples/hud.quill.md` shows text, bars and pictures.

### Custom panels

A panel of your own, such as a status screen, needs nothing new: a HUD button calls a label, the
label draws the panel with `::hud` and waits on a `::hotspots` row, and closing removes the
elements and returns.

```
---
vars: { who: "mika", name.mika: "Mika", name.ren: "Ren", l1.mika: "Likes tea.", l1.ren: "Runs the news stand and knows every rumour in town before it is printed." }
---
# start
::hud{id=status_btn text="Status" x=0.98 y=0.03 back=#00000099 pad=10 call=#status}
Press Status.
::end

# status
::hud{id=st_back box=#000000cc size=900,700 x=center y=center}
::hud{id=st_name text="{name.{who}}" size=56 x=center y=28%}
::hud{id=st_1 text="{l1.{who}}" width=700 x=center y=40%}
::textbox{hide}
::hotspots
- [Mika](#status){rect=30%,66%,10%,7% text="Mika", who="mika"}
- [Ren](#status){rect=45%,66%,10%,7% text="Ren", who="ren"}
- [Close](#close){rect=45%,74%,10%,7% text="Close"}
::

# close
::hud{hide=st_*}
::return
```

- A tab sets a variable and goes to the panel's own label, which draws it again: the elements
  keep their ids, so they are replaced in place.
- `{name.{who}}` reads the variable named by the current `who`. A line whose variable is empty
  draws nothing, so a character with fewer lines simply shows fewer.
- `width=700` wraps a long line inside the box instead of letting it run on.
- Naming every element of the panel `st_...` lets `::hud{hide=st_*}` close it in one line.
- The story's line is still pending while the panel is up, so `::textbox{hide}` keeps it from covering the panel.
- `::return` goes back to the story, which shows the line it was on again (the box comes back with it).

`examples/panel.quill.md` is a working one.

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
| `::voice{src=line1.ogg volume=0.8}` | Voice clip for the **next** line; stops when the player advances. `volume` (0 to 1, default 1) is this clip's loudness within the Voice volume setting, like `::sfx`'s. Rollback and saves keep it. An automatic voice file (below) plays at full volume. |
| `::video{src=clip.webm fit=cover loop skippable}` | Fullscreen clip; waits until it ends or the player skips it. `fit`: `contain` (default), `cover`, `stretch`. A `loop` video is always `skippable` (it never ends by itself). |
| `::wait{time=1.5}` | Pause (`time` above 0); a click skips it. |
| `::wait{click}` | Stop with the screen as it is until the player clicks (or presses Enter or Space, or confirms on a gamepad). Auto-forward goes on after its delay, skip mode passes it, and rollback can stop on it. `quillstone run` waits for Enter. |

### Auto-voice

For a voiced game, put `voice: voice` in the front matter and drop recordings into that
folder, named after their lines. Each line with a recording plays it, no `::voice` needed (an
explicit `::voice` still wins).

- **The names** look like `rooftop-1a2b3c4d.ogg`: the line's label, and a code made from its
  speaker and text. So a recording stays with its line when other lines change, and a line
  that is reworded needs recording again (it gets a new name).
- **The recording script.** `quillstone voices script.quill.md > lines.tsv` lists every line's
  file name, speaker and text, ready for a spreadsheet and the voice actors, with a "recorded"
  column once the folder exists.
- **Checking.** `quillstone check` says how many lines are voiced, and warns about recordings
  that match no line (their line was reworded or removed). `package` copies only the
  recordings in use.

### Formats

| Kind | Formats |
| --- | --- |
| Audio | Ogg Vorbis (`.ogg`, recommended for music), Opus (`.opus`, smallest for voice lines), WAV, MP3 and FLAC |
| Images | **PNG**, **JPEG** or **WebP**, at most 8192 pixels on a side |
| Video | **WebM** (`.webm`) with VP9 (recommended) or VP8 video, and Opus or Vorbis sound |

`quillstone check` refuses anything else.

- **Audio.** Opus is decoded with FFmpeg, the others built in. AAC/`.m4a` is patent-licensed,
  so it isn't supported.
- **Video.** The WebM formats are royalty-free and they are what a shipped game can play. For
  other videos (MP4, AV1, H.264, Ogg Theora...) `check` shows how to convert them, e.g.
  `ffmpeg -i in.mp4 -c:v libvpx-vp9 -crf 32 -b:v 0 -c:a libopus out.webm`.

### How sound behaves

**Music**

- `::music` with the track that is **already playing** doesn't restart it; only the volume
  changes (fading over `fade`). So it's safe to repeat a scene's `::music` line. A
  `loop=false` track that has finished does start again.
- Rollback keeps the music going if the earlier moment had the same track, and switches or
  stops it otherwise.

**A track with an intro** is the pair of files.

- The same `src` and `intro` already playing never restarts, so repeating the line is safe.
  Rollback, or loading a save into a scene where that pair is playing, leaves it alone.
- The playing position is not saved. Whenever the pair is started afresh (a save loaded from
  the main menu, another track playing before it), it starts from the intro.
- `src` with a different `intro`, or none, is another piece and crossfades.
- In the music room an entry is unlocked by its `src` file, and playing it there plays that
  file alone, without the intro.

**Voice and one-shot sounds**

- A voiced line's clip plays again when you roll back to it.
- One-shot sound effects are never replayed by rollback.

**Looping sounds.** A looping `::sfx` is part of the scene, like the music.

- Starting a loop that is **already playing** doesn't restart it (only its `volume` changes),
  so repeating a scene's `::sfx{... loop}` line is safe.
- Rollback, loading a save and reloading the script keep the loops the earlier moment had
  playing and stop the others.
- Loops follow the sound volume slider. Skipping mutes one-shots but not loops.
- A loop keeps playing until a `::sfx{stop}`, and the main menu ends it.
- `loop` needs a file that exists and plays, like music, and `quillstone package` includes it.
- Loops are streamed from disk, like music, so a long ambience costs no memory or start-up
  time.
- If a loop can't start (a missing or damaged file), it is tried again at the next
  `::sfx{loop}` of it, or when the scene is redrawn (rollback, load).
- There are no other channels: one-shots, loops and `stop` all use the sound channel.

**Voice ducking.** With `voice_duck` in the theme (for example `0.5`), music and looping sounds
sink to that fraction of their volume (0.3 s) while a voice clip plays, and come back (0.6 s)
when it ends.

- One-shot sounds and the voice itself are never ducked.
- A line whose voice follows another stays ducked.
- Skipping, which plays no voices, never ducks.
- The volume sliders and each `::music`/`::sfx` `volume` still apply on top.
- Without the key (or with `1`) nothing changes.

**A video's own sound** plays with its picture.

- A `::video` cutscene and a clip opened in the Extras gallery have sound. A looping `::bg`
  video is muted unless it says `sound`, and a video menu background is always muted.
- The sound uses the **music** volume setting. Voice lines don't turn it down.
- Music already playing keeps playing under it: stop the music before a cutscene if you want
  the clip alone.
- It starts with the first picture, and stops when the clip is skipped, rolled back past,
  replaced or closed. A menu over the story or a minimised window holds it with the picture.
- A clip without a sound track is simply silent.

An unskippable video only blocks clicks while it is really playing, so a file that can't be
decoded never traps the player.

## Flow

````markdown
# somewhere
<!-- jump to a label -->
::jump{to=ending}
<!-- only if true -->
::jump{to=ending if="trust > 1"}
<!-- run a label like a subroutine... -->
::call{to=flashback}
<!-- ...which may take parameters (see below) -->
::call{to=heal amount=5 who="Mika"}
<!-- ...and come back after the ::call -->
::return
<!-- finish the story -->
::end
<!-- end a scene replay (see below); otherwise does nothing -->
::end_replay

<!-- blocks may nest -->
::if{trust > 1}
**Mika:** Great.
::elif{trust > 0}
**Mika:** Fine.
::else
**Mika:** Hm.
::endif

::choice
- [Tell the truth](#truth){trust+=1}
- [Lie](#lie){trust-=1, seen=true}
- [Confess everything](#everything){if met and trust > 0}
::
````

- Falling off the end of a label continues into the next one, so end each scene
  with `::jump`, `::return` or `::end` when that isn't what you want.
- `::call` can nest up to 256 deep. `::return` with nothing to return to ends the story.

### Choices

A choice option is `- [text](#label){effects, if condition}`. The braces are optional.

- An option with an `if` is only offered while it is true. If no option is on offer, the story
  carries on past the block.
- Many options shrink to fit above the textbox. Keys 1-9 pick the first nine.

**`once`** makes an option go away after it has been picked:
`- [Ask about the scarf](#scarf){once}`. A menu that jumps back to itself then offers fewer and
fewer questions, and when all of them are `once` and used up, the story carries on past it.
Options with the same text and label count as the same option.

**Locked options.** An option with an `if` is hidden while the condition is false. Add `show` to
draw it greyed instead: it can't be picked, but it tells the player there is more to find. Add
`show="reason"` to also print a reason in small dim text under it:

````markdown
::choice
- [Open the door](#door){if has_key, show}
- [Lift the gate](#gate){if str >= 5, show="Needs STR 5"}
- [Read the runes](#runes){if lore >= 3, show="???"}
::
````

- A `show` needs an `if` (what unlocks the option), and the reason goes in double quotes.
- The reason is optional on purpose. If you don't want to give the game away, use plain `show`
  (just the greyed text) or write `show="???"` yourself.
- The reason can use `{variables}` and markup like the option text, and is translated like it.
- A greyed option can't be clicked, chosen with a number key or picked when a timed choice
  runs out, and the arrow keys and a gamepad skip it. If the `default` option is locked then,
  the first pickable one is picked.
- A choice where every option is locked counts as one with nothing on offer, so the story
  carries on past it.
- A `{once}` option that was already picked stays hidden, not greyed.
- `show` isn't for `::hotspots`. `quillstone test` never picks a locked option.

**A timed choice** runs out. Write `::choice{time=8}` and mark exactly one option `default` in
its braces. When the time is up, that option is picked as if it had been clicked, so a silent
answer is an answer too:

````markdown
::choice{time=8}
- [Tell the truth](#truth)
- [Say nothing](#silent){default}
::
````

- **A label instead of an option.** `default=#label` in the braces jumps to a label when time
  runs out, with no option picked, like a timed `::hotspots`:

  ````markdown
  ::choice{time=8 default=#too-slow}
  - [Tell the truth](#truth)
  - [Lie](#lie)
  ::
  ````

  It needs `time=`, and can't be combined with an option marked `default`.
- **The time** is in seconds, a plain number, 0.5 and up. `quillstone check` warns when it is
  under 2 seconds (too quick to read) or over 600 (a typo?); 86400 is the most.
- **`default`** combines with the other settings (`{default, once, trust-=1, if met}`). If the
  default option's `if` is false when the choice shows, the first visible option is picked
  instead.
- **The countdown bar** over the options shows the time left. It starts when the options
  appear (not while the line above is still being typed), and starts full again after a
  rollback to the choice or a load. Its look is a theme setting (see [Themes](#themes)).
- **It waits** while Settings, the history or a question is open, and while the window is not
  the active one.
- **Picking** by click or key works as usual, and hovering changes nothing. Auto-forward never
  picks for the player, and skipping stops at the choice.
- **Errors.** A timed choice with neither a `default` option nor `default=#label` (or with two
  default options, or with both kinds), and `default` without `time`.
  `::shop` can't be timed; a `::hotspots` block can (see [Hotspots](#hotspots)).
- `quillstone test` plays the default like any other option, and the `default=#label` jump as
  one more branch.

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
  letters, digits and `_`, with no types (a default value is optional, see below). The label is still `heal`
  (the heading text before the parenthesis), so `seen(heal)` works as usual.
- **The rule for parentheses in a heading.** The `(` must touch the name *and* the heading
  must end with `)` for it to be a parameter list.
  - `# Round 1(a)` and `# Café(night)` are therefore labels with parameters: `round-1` taking
    `a`, `café` taking `night`.
  - With a space before the `(`, like `# Chapter 1 (the start)` or `# heal (amount)`, the
    whole heading is just a title. Its label is the slug of all of it: `chapter-1-the-start`,
    `heal-amount` (the parentheses and spaces turn into dashes).
  - If you meant a plain title and wrote `# Round 1(a)`, the errors that follow ("can run
    into", "can only be reached by `::call`") say so: put a space before the `(`.
- **Passing values.** A `::call` passes every parameter as `name=value`, with exactly the names
  the heading lists. The value is an ordinary expression, as on the right of a `::set`: a
  number, `"text"` in quotes, `true`/`false`, a variable, a calculation in parentheses
  (`n=(gold - 5)`), or a parenthesised condition. `random` is not allowed: assign it first.
- Spaces are fine inside quotes or parentheses (`who="Mika Tanaka"`, `ok=(gold >= 5 and met)`).
  Elsewhere a space starts the next argument.
- **Default values.** A parameter may be written `name=value` with a literal: a number,
  `"text"` in quotes, or `true`/`false` (not an expression or a variable). A `::call`
  that leaves it out gets the default; one that gives it overrides it. A parameter
  without a default must be given.

  ````markdown
  ::call{to=hit target="Ren"}
  ::call{to=hit target="Ren" magnitude=3}

  # hit(magnitude=1, target)
  {target} takes {magnitude}.
  ::set{hp -= magnitude}
  ::return
  ````

  A default is stored like a passed value, so the label cannot tell the two apart.
- **Parameters are ordinary variables.** At the call, every value is worked out first (so
  `a=b b=a` swaps them) and then stored into the variables of those names before the label
  runs.
  - Nothing is restored at `::return`: they keep their values afterwards.
  - Rollback and saves treat them like any variable, and `{amount}` in a line shows them.
  - Pick parameter names that don't clash with variables the story uses for something else.
  - A list variable can't be a parameter.
- **A label with parameters can only be reached by `::call`.** `quillstone check` reports,
  with the line:
  - a call that leaves out a parameter that has no default, passes one the label doesn't have
    (a misspelling shows up as both), or passes arguments to a label without parameters;
  - a `::jump`, choice link or hotspot link to it;
  - a label above it that runs on into it. End the label above with `::jump`, `::return` or
    `::end` (put subroutines after the story, as in `examples/shop.quill.md`).
- `quillstone test` plays them like any other label.

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

An entry is `- [label](#target){rect=x,y,w,h ...}`.

- **`rect`** is the region's left, top, width and height on the screen. Each is a number from
  0 to 1 or a percentage (no spaces inside it), and the region must lie inside the screen. It
  is required, unless the region follows a sprite (see below).
- **The label** shows as a small tooltip next to the mouse while the region is hovered.
- **`image=assets/key.png`** (PNG, JPEG or WebP) is drawn stretched into the region. Without
  one the region is invisible until the mouse or keyboard reaches it, when it is outlined.
- **Effects, `if condition` and `once`** work as in a `::choice`, and so does the `#label` it
  jumps to. Put these settings after `rect` and `image`, separated by commas.
- A region that is `once` or whose `if` is false disappears; with none left the block is
  skipped. Where regions overlap, the one listed last is on top.

**Using a hotspot screen**

- The block normally follows a `::bg` with no line, so the regions are the only way on. A line
  shown just before it stays on screen.
- To make a hub, jump back to a label that sets the background and the hotspots again (see
  `examples/hotspots.quill.md`).
- Mouse: click a region. Keyboard and gamepad: the arrow keys (or D-pad) move between the
  regions, and Enter or Space takes the focused one. The number keys don't pick hotspots.
- Skip and auto-forward stop at the block. It is an auto save point like a choice, and the
  pick isn't added to the history. Going back returns to the screen before the click.

**Checking.** `quillstone check` checks the images. It warns about a region that can never be
clicked because a later one covers it entirely, and about a block whose regions are all
`hidden`. `quillstone test` follows every region like a choice option.

### Hidden regions

`hidden` makes a region secret: no outline, tint or tooltip when the mouse is over it, so a
collectible can't be found by sweeping the mouse. An `image=` is still drawn.

```
- [Old key](#found){rect=60%,70%,5%,8%, hidden, once, found=true}
```

Keyboard and gamepad focus skip hidden regions, so a block needs at least one visible region
(such as an exit) for those players. `hidden` is an error in a `::choice`.

### Hover pictures

`hover=assets/door_hover.png` (the same kinds of file as `image=`) is drawn into the region
instead of `image=` while it is hovered or the keyboard is on it.

```
- [Door](#door){rect=10%,20%,8%,30% image=assets/door.png hover=assets/door_hover.png}
```

- With it, the outline and tint are off: the picture is the highlight. The tooltip stays.
- Without an `image=`, the region shows nothing until hovered.
- It works for regions that follow a sprite. A `hidden` region never shows it.
- `check` tests it as a picture and `package` includes it.

### Text on regions

`text="YES"` draws text inside the region's rect, centred: a button, a menu entry or an option on
a settings screen, with or without a picture (text is drawn over `image=`).

| Attribute | |
| --- | --- |
| `text="YES"` | the text; quote it when it has spaces or commas |
| `color=#ffffff` | its colour (default: the theme's text colour) |
| `size=40` | its size, 8 to 300, in pixels of the 1920x1080 screen like a HUD text (default: the theme's text size) |
| `hover_color=#ffff00` | its colour while the mouse or keyboard is on the region |

- The text takes `{name}` values, `**bold**`, `*italic*` and colour and size tags like a line
  does (not `{nw}` or `{p}`). It is worked out each time the screen is shown, so a screen the
  story returns to (a `call` region, or a jump back) shows the new values.
- Without `hover_color=`, a hovered or focused region gets the theme's button hover panel behind its
  text, as a HUD text button does. A text region has no outline, tint or tooltip: the text is
  its label. (The label in `[...]` is still what `quillstone test` and translations use.)
- Keyboard and gamepad reach it like any region. `hidden` stops the highlight and the keyboard
  focus but not the text.

A phone's settings screen with a YES / NO toggle: two regions per option, the one that is on
in white (it only returns to the screen) and the other grey (it sets the variable):

````markdown
---
vars: { minimap: true }
---
# phone
::hud{id=label text="Minimap:" x=0.3 y=0.3}
::hotspots
- [Minimap is on](#phone){rect=50%,28%,10%,8% text=YES color=#ffffff, if minimap}
- [Turn the minimap on](#phone){rect=50%,28%,10%,8% text=YES color=#808080, minimap=true, if not minimap}
- [Minimap is off](#phone){rect=62%,28%,10%,8% text=NO color=#ffffff, if not minimap}
- [Turn the minimap off](#phone){rect=62%,28%,10%,8% text=NO color=#808080, minimap=false, if minimap}
- [Back](#out){rect=50%,50%,10%,8% text=Back}
::
````

### Imagemaps

`::hotspots{ground=assets/map.png hover=assets/map_hover.png}` takes two full-screen pictures,
drawn stretched to the screen like a background. They should match in size and content, the
second one lit up.

- While a region is hovered or focused, the part of the hover picture inside its rect is drawn
  over it. So the regions need no pictures of their own; one with its own `image=` or `hover=`
  uses that instead.
- `ground` is optional when the background already shows the picture.
- It combines with `time=` and `default=`.
- Nothing about it is saved: it comes from the script.
- `::choice` has no `ground=` or `hover=`.

### Call regions

`call` after the rect makes picking the region call its label, like `::call`. The label's
`::return` comes back to this same screen, shown again with its `once` and `if` worked out
afresh, so a hub of rooms needs no `::jump` back. A region without `call` jumps as before.

A two-room map (`examples/hotspots.quill.md` has the same pieces):

````markdown
# map
::hotspots{ground=assets/map.png hover=assets/map_hover.png}
- [Kitchen](#kitchen){rect=10%,20%,25%,30%, call}
- [Cellar](#cellar){rect=60%,50%,30%,40%, call, once}
- [Leave](#out){rect=90%,2%,8%,8%}
::
# kitchen
The kettle is still warm.
::return
# cellar
A cold draught comes up the steps.
::return
````

- The label can't take parameters.
- A rollback or a load inside it comes back to the screen.
- A click does nothing while the call stack is full.
- The screen's own line (if any) is not restored. The dialogue box is hidden again on the
  return (as `::textbox{hide}` does), so the label's last line doesn't stay over the screen;
  the next line or choice shows it as usual.
- The background is not restored. An imagemap's `ground=` covers it, but on a block without
  `ground=`, end the label with `::bg` if it changed the picture.
- `::choice` has no `call`.

### Timed screens

`::hotspots{time=3 default=#too-slow}` gives the player 3 seconds and jumps to the label when
time runs out. Both are needed together. The time is in seconds, 0.5 and up, a plain number, as
for `::choice`.

It counts down like a timed `::choice` (see [Choices](#choices)):

- the same bar, along the top of the screen since there are no buttons;
- it starts when the screen appears, and again after a rollback or a load;
- it waits while Settings, the history or a question is open, and while the window is not the
  active one;
- auto-forward never picks for the player, and skipping stops at the screen.

The jump is a label, not one of the regions, because running out of time is the player
clicking nothing: a region's own effects don't run. `quillstone test` plays the timeout as one
more outcome, and `quillstone graph` draws it as a "(time out)" edge.

### Regions that follow a sprite

`sprite=firefly` instead of `rect=` binds the region to a sprite: it is wherever that sprite is
drawn right now. A lamp that bobs across the screen (`examples/minigame.quill.md`):

````markdown
::show{id=lamp src=assets/lamp.png x=10% y=40%}
::move{lamp x=90% time=2 ease=inout loop}
::hotspots{time=5 default=#too-slow}
- [The lamp](#caught){sprite=lamp}
::
````

- The region follows the sprite through `::move` (also a looping one), `::anim` frames,
  `scale`, `xscale`, `flip`, `sprite_size: pixels` and the camera. Highlight, tooltip and an
  `image=` follow the sprite too.
- The name is the one a `::show` gave the sprite (`::show{id=firefly src=...}` or a
  character's name). `quillstone check` reports a name no `::show` uses.
- With a `rect=` too, the rect is a fraction (0 to 1, or a percentage) of the sprite's own
  rectangle, say the lower half: `rect=0,50%,1,50%`. Without one it is the whole sprite.
- While the sprite is not on stage (not yet shown, hidden, fading out), the region can't be
  clicked or reached with the arrow keys.
- `quillstone test` counts the region as offered while the sprite is shown, since it can't
  tell where it is.

## Free roam

A sandbox where the player picks places and the day moves on is built from parts you already
have. `examples/freeroam.quill.md` is a small one.

**The clock.** Two variables and a HUD text that shows them:

````markdown
---
vars: { day: 1, time: "morning", talked_ren: false }
---
# start
::hud{id=clock text="Day {day}, {time}" x=0.98 y=0.04 size=36 back=#00000099 pad=12}
````

**The map.** A `::hotspots` block of `call` regions. Each place comes back to the map when its
label returns, and the regions are worked out again every time, so a place can have an `if`:

````markdown
# map
::bg{src=assets/town.png}
::hotspots
- [The cafe](#cafe){rect=14%,52%,8%,14% image=assets/pin.png, call}
- [The park](#park){rect=70%,58%,8%,14% image=assets/pin.png, call, if park_open and time != "morning"}
- [Wait a while](#pass-time){rect=40%,90%,20%,7% text="Wait a while", call}
::
````

**Moving time on.** One label does it, and every place and the "wait" region end up there. It
steps `time`, rolls the day over and resets what is daily:

````markdown
# pass-time
::if{time == "morning"}
::set{time="afternoon"}
::elif{time == "afternoon"}
::set{time="evening"}
::else
::set{time="morning", day += 1, talked_ren=false}
::endif
::if{day > 3}
::jump{to=ending}
::endif
::return
````

**What happens at a place.** An `::if` / `::elif` chain tries the events in order and the first
one that holds runs, so put the rarest first. `seen(label)` makes an event happen once without a
flag of your own, and a plain variable does for something that comes back every day:

````markdown
# cafe
::if{day == 2 and time == "evening" and not seen(mia-coffee)}
::call{to=mia-coffee}
::elif{not seen(mia-hello)}
::call{to=mia-hello}
::elif{not talked_ren}
::call{to=ren-news}
::else
The cafe is quiet.
::endif
::call{to=pass-time}
::return
````

The events are labels that end with `::return`. For a scene that shouldn't have the clock over
it, wrap it in `::hud{off}` and `::hud{on}` (see [HUD](#hud-text-bars-and-pictures-over-the-story)).
`quillstone test` plays every place and every event, whatever time it needs.

## Timing bar

A bar with a marker that sweeps back and forth. The player stops it, and where it stops
decides the result: inside the *perfect* zone, inside the *good* zone, or a miss. Use it for
lockpicking, a golf swing, a fishing strike.

It is one step of the story, like a choice: the result is stored in a variable and the story
goes on with the next line, where `::if` can branch on it. `examples/minigame.quill.md` has a
lock with three pins:

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

| Attribute | | What it is |
| --- | --- | --- |
| `result` | required | the text variable that gets `"perfect"`, `"good"` or `"miss"` |
| `good` | required | the zone to stop in |
| `perfect` | optional | a smaller zone, which must lie inside `good` |
| `speed` | optional, default 1 | how many times the marker crosses the bar in a second, 0.2 to 5 |
| `time` | optional | the limit in seconds |
| `prompt` | optional | a text shown above the bar |

- **`result`** is declared in `vars:` with a text default, like `hit: ""`. Without a `perfect`
  zone it is never `"perfect"`.
- **Zones** are written `from..to`, as percentages of the bar's length (`40%..60%`) or
  fractions (`0.4..0.6`). `from` must be below `to`, and the ends count as inside.
- **`speed`.** The marker starts at the left end, runs to the right end and back again, and so
  on for ever. So `speed=1` is a back-and-forth every two seconds.
- **`time`** is 0.5 and up, a plain number, as for `::choice`. When it runs out without a
  press, the result is `"miss"`. Without it the bar waits for ever.
- **`prompt`** takes `{name}` values, `**bold**`, `*italic*`, colour and size tags, like an
  `::input` prompt (not `{nw}` or `{p}`). It is translated through `lang export` like one.

**Playing it**

- The player stops the marker with a click, Space, Enter or Right, or the gamepad's confirm
  button: the same presses that advance a line.
- The marker starts moving once the line before it is fully typed.
- It stands still while the window is not in focus or minimised, and while Settings, History
  or a confirmation is open, like the countdown of a timed choice.
- Skip mode stops at the bar, and auto-forward never presses it.

**Rollback and saves.** Rollback goes back to before the bar, which starts afresh: marker at
the left end, full time limit. Roll forward uses the result that was recorded. A save made at
the bar loads at the bar, started afresh. Nothing about the marker is saved.

**The tools.** `quillstone test` plays each result as a branch: miss and good, plus perfect
when there is a perfect zone. `quillstone graph` draws no extra arrows: the story simply goes
on, and your `::if` jumps show as usual. `quillstone run` asks you to type `p`, `g` or `m` for
perfect, good or miss.

**The look** comes from the theme (see [Themes](#themes)). By default it is a track in
`panel_color`, the good zone in `accent_color` (see-through), the perfect zone in `name_color`
and a thin marker in `text_color`, in the middle of the screen.

````
# stretched over the track instead of panel_color
timing_bar_image = assets/gui/lockbar.png
# the marker, drawn at its height with its own proportions
timing_marker_image = assets/gui/needle.png
# the good zone (default accent_color, made see-through)
timing_good_color = #ffd98c80
# the perfect zone (default name_color)
timing_perfect_color = #ff5555
# the plain marker (default text_color)
timing_marker_color = #ffffff
````

The zones are always drawn over the track, so a picture should leave them visible. The
pictures are checked by `quillstone check` and packed by `quillstone package` like other theme
pictures.

## Key prompts

Ask for a key within a time: a quick-time event ("Duck!") or, with `count`, button mashing
("Force the door!").

Like the timing bar, it is one step of the story: the result is stored in a variable and the
story goes on with the next line, where `::if` can branch on it. `examples/minigame.quill.md`
has a dodge and a door at the end:

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

| Attribute | | What it is |
| --- | --- | --- |
| `key` | required | the key to press |
| `time` | required | the limit in seconds: 0.5 and up, a plain number, as for `::choice` |
| `result` | required | a text variable declared in `vars:`, set to `"hit"` or `"miss"` (time ran out first) |
| `count` | optional, 1 to 200, default 1 | how many presses it takes |
| `prompt` | optional | a text shown above the key cap |

- **`key`** is one of `space`, `enter`, `left`, `right`, `up`, `down`, `click` (a mouse click)
  or a letter `a` to `z`. Anything else is an error that lists these. The prompt draws a key
  cap with the key's name.
- `space` and `enter` are the same key here (either one counts), which is what lets the
  gamepad's confirm button press them.
- **`count`.** A bar under the time bar fills with each press. All of them within `time` is a
  hit.
- **`prompt`** takes `{name}` values and styling like the timing bar's, and is translated
  through `lang export` too.

**Which presses count** (the same goes for the timing bar)

- A wrong key does nothing. It is not a miss, so a slip of the finger is forgiving.
- While the prompt is up, Space, Enter, the arrows, Backspace, PageUp and the letters do not
  advance, go back, open the history, switch auto-forward and so on, whatever the key asked
  for. Esc still opens the Save screen, and the quick menu buttons still work.
- A key that was already held down when the prompt appeared does not count.
- Holding a key does not repeat it: every press is a new one.

**The clock** starts once the line before it is fully typed. It stands still while the window
is not in focus or minimised, and while Settings, History or a confirmation is open. Skip mode
stops at the prompt, and auto-forward never presses it.

**Rollback and saves.** Rollback goes back to before the prompt, which starts afresh: full
time, no presses. Roll forward uses the recorded result. A save made at the prompt loads at
the prompt, started afresh.

**The tools.** `quillstone test` plays both results as branches; `quillstone graph` draws no
extra arrows. `quillstone run` asks you to type `h` (hit) or `m` (miss).

**The look.** A key cap in `panel_color` (or the picture `press_key_image`), the time bar in
`choice_timer_color` (default `accent_color`) and the presses bar in `timing_perfect_color`
(default `name_color`), in the middle of the screen.

````
# the key cap, 160x120 on the 1920x1080 screen (checked and packed like other theme pictures)
press_key_image = assets/gui/keycap.png
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
  Both may hold `{name}` values and the tags a line takes (`**bold**`, colours...; not `{nw}` or
  `{p}`), put in again each time a screen shows them: `name="{hero}'s sword"`,
  `text="{mika.hint}"` with a `::set{mika.hint = "..."}` somewhere in the story.
  A name in the grid is one plain row (tags are dropped); the description keeps them.
- `icon` is a picture (PNG, JPEG or WebP) drawn above the name. `price` is a whole
  number, 0 or more: what a shop asks unless its line says otherwise.
- `count=potions` makes a stack. It names a whole-number variable declared in `vars:` (or
  `persistent:`).
  - The item is shown with "x3" under its name while that number is above 0, and not at all
    at 0 or less.
  - For an item with `count=` the list is not looked at: set the number, not
    `pack += "potion"`. Items without it work as before.
  - Counted items are listed after the others, in the order they are declared.
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

  - A `use=` item with no `count=` is used the same way, and stays in its list unless the
    label does `::set{pack -= "scroll"}`.
  - When the label returns, the line the player used the item at comes back as it was. Its
    inline pictures come back too; its voice is not replayed.
  - History keeps that line once: it is not logged a second time.
  - A HUD button's `call=` works the same way.
- A list may hold an id that was never declared: the screens show it under its id, with no
  picture.

`quillstone check` checks the icons, and warns about:

- a `+= "x"` or `has "x"` on the inventory list that names no item (usually a typo);
- an icon wider or taller than 512 px (icons are drawn at about 100 px, so a smaller file
  loads faster and saves memory);
- a description too long for the description box.

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
**The screen** shows the items in the list, in the order they were added, as a grid of cells
with the item's picture and name.

- Pointing at an item, or moving to it with the arrow keys, shows its description (and its
  price if it has one) in a box under the grid.
- With more items than fit, the mouse wheel, Page Up/Down and the arrow keys turn pages.
- Selecting an item (a click, or the arrow keys) only shows it. An item with
  [`use=`](#items) also shows a **Use** button next to Close, and Enter or Space on the
  selected item presses it.
- To let another item do something, offer a choice with `if pack has "key"`.

**Use**

- It works only when the player opened the inventory over the story (the Items button or I)
  and the story is waiting for them to advance a line.
- It closes the inventory and **calls** the label as a normal step. So when the label ends
  with `::return`, the same line is shown again.
- Rollback goes back to before the use, and a save made inside the label loads inside it.
- Over a choice, `::wait`, `::input`, a shop, a line ending in `{nw}`, a scene replay, or in
  an inventory opened by `::inventory`, the button is shown greyed and a press only says
  "Can't use that now".
- `quillstone test` uses each such item once, so its label counts as reached.

**Opened by the player**, the inventory sits over the story, which waits: a `::wait` doesn't
run out and nothing is skipped. Esc, a right click, the I key or the Close button closes it.

**Opened by the script.** `::inventory` opens the same screen. The story waits until the
player presses its Close button (or I, or Backspace) and then carries on.

- Esc or a right click opens the Save screen instead, which returns to the inventory. S, L, H,
  F5 and F9 work there too.
- Rollback, saves and hot reload work with it like with any other waiting step, and
  `quillstone test` treats it as one click.

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

**Turning it on.** `journal: quests` names the list variable the journal shows. It must be
declared as a list in `vars:` (or `persistent:`), or the error names the front matter line.
The game then has:

- a **Journal** button in the quick menu, and the **J** key to open it;
- `::journal` to open it from the script (an error in a game without `journal:`).

Theme picture buttons use the id `journal` (`quick_journal_icon`, `_hover`, `_pos`, `_size`),
and `quick_buttons = listed` handles it like the others.

**Entries** are declared like items, in the same `::item` lines. A quest id and an item id
share one namespace.

- Only `name`, `text` and `icon` matter here: the journal has no Use button, prices or counts,
  even if the item has them.
- An id with no declaration is shown under its id. `quillstone check` warns about a literal
  `+= "x"` or `has "x"` on the journal lists that names nothing declared.

**Finished quests.** `journal_done: quests_done` (optional, needs `journal:`) names a second
list of finished quests. They are shown after the open ones, greyed, with "Done" under the
name.

- Marking one done is one line: `::set{quests_done += "find-cat"}`. You may also take it out
  of the first list with `::set{quests -= "find-cat"}`, which changes nothing on screen.
- Without `journal_done:`, a quest is finished by removing it from the list.
- Nothing else is tracked. There is no state machine, only plain list variables, so saves,
  rollback and `quillstone test` treat them like any other.

**The screen** is otherwise the inventory's: the same grid, descriptions, pages, theme keys and
close keys.

- Only J, not I, closes the journal.
- `journal_title` (a theme key) sets its title (default "Journal"), and the quick-menu button
  says the same. The look of a done quest is `inventory_cell_disabled`.
- Opened by the player it sits over the story. `::journal` leaves the story waiting until it
  is closed, and `quillstone test` treats it as one click.
- The title and the "Done" label are not translated. The quest names and descriptions are,
  like any item's (see [Translating a game](#translating-a-game)).

**A hints list.** The journal can hold one entry per character, with a face and a line about what
to do next, kept up to date by the story. Name it "Hints" with `journal_title = Hints` in the theme:

````markdown
---
vars: { hints: [mika, ren], mika.hint: "I need to get stronger first. Maybe the gym?", ren.hint: "He wants to see the city at night. Meet him on the rooftop." }
journal: hints
---
::item{id=mika name="Mika" icon=assets/faces/mika.png text="{mika.hint}"}
::item{id=ren name="Ren" icon=assets/faces/ren.png text="{ren.hint}"}

# gym
::set{mika.hint = "Now ask Ren about the key."}
````

The grid shows one description at a time, under the cells. For a hints list, where every entry's whole text
should be readable at once, switch the screen to a list in the theme:

```text
# one row per entry instead of the grid (default grid)
journal_layout = list
# row pictures (and a hover one), drawn as nine pieces with this corner size in the picture's pixels;
# without them the rows are button_color rectangles. The Close button wears the same picture
journal_row_image = gui/row.png
journal_row_hover = gui/row_hover.png
journal_row_border = 26
```

- A row is the entry's picture on the left, its name, and its whole text (values filled in, tags kept) wrapped
  under the name. A row is as tall as its text, so long hints are never cut.
- When the rows do not fit, the wheel, Up/Down (the D-pad), Page Up/Page Down, Home and End scroll; the arrows
  move a highlight and keep it in view. A finished entry (`journal_done:`) is greyed and says "Done", as in the grid.
- The grid stays the default because it also suits entries that are only a picture and a name; the layout is a
  theme key like the screen's title (`journal_title`) because it is about how the screen looks, not what is in it.

A HUD button with `open=journal` shows it (see [HUD](#hud-text-bars-and-pictures-over-the-story)).

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
- If no item is on offer, the block is skipped.

**The screen.** The story waits at the shop, which shows every offered item with its picture,
name and price, and how much the player has.

- The Leave button (or Backspace, the gamepad's X) ends the block, and the story continues
  after it.
- Esc or a right click opens the Save screen instead, as everywhere else. Closing it returns
  to the shop, with the page, selection and purchases kept.
- S, L, H, F5 and F9 work there too (save, load, history, quick save and load).

**Buying.** The player buys an item by clicking it, or by moving to it with the arrow keys and
pressing Enter or Space.

- The price is taken from `pay`, the item is added to `into`, a notice says "Bought Brass
  key", and the shop stays open with the new money.
- An item already in the list is marked **Owned** and can't be bought again.
- An item that costs more than the player has is greyed out, and a click only says "Not enough
  gold". Money can never go below zero.

**Counted items.** An item with [`count=`](#items) is never "owned". Buying it adds 1 to its
count variable (and nothing to `into`), as often as the player can pay.

- Its cell says how many the player has: `5 gold (owned: 2)`.
- Its price must be above 0, and its count variable must be `persistent:` exactly when `pay`
  is.

**Rules and tools**
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

| Attribute | | What it is |
| --- | --- | --- |
| `var` | required | the text or number variable that gets the answer |
| `prompt` | required | the question |
| `default` | optional | the answer used when the player confirms an empty field |
| `max` | optional, 1 to 1024, default 24 (a number: 1 to 18, default 9) | how many characters (digits, for a number) the answer may have |

- **`var`** is declared in `vars:` or `persistent:` with a text default or a number default (dotted names
  work). Its declared type decides what is asked: see "A number answer" below.
- **`prompt`** takes `{name}` values, `**bold**`, `*italic*`, colour and size tags like a line
  does (not `{nw}` or `{p}`).
- **`default`**: the field shows it dimmed as a hint.
- **`max`** counts characters, not bytes, and `default` may not be longer. Over 64, `check`
  warns of a typo.

**The screen.** The story waits at a centred panel with the prompt, a text field with a
blinking caret and an OK button.

- Typing adds characters at the end, and Backspace removes the last one. There is no moving
  the caret inside the text.
- Ctrl+V pastes text from the clipboard at the end (Cmd+V on a Mac). Line breaks are dropped
  and the text is cut at the limit.
- A long text scrolls so its last characters stay in view. The panel counts what is typed
  against the limit.
- Enter or OK confirms.

**A number answer.** When `var` is a number variable (`vars: { code: 0 }`), the field asks for a whole
number and stores it as a number:

````markdown
---
vars: { code: 0 }
---
::input{var=code prompt="Enter the door code" max=4}

::if{code == 1234}
**Mika:** The lock clicks open.
::else
**Mika:** Nothing happens.
::endif
````

- The field takes only digits, and a `-` as the first character. Anything else typed or pasted is
  dropped, so a stored answer is always a whole number (leading zeros go: `007` is 7).
- `max` counts the digits (a `-` is free). It is at most 18, so the number always fits; the default is 9.
- `default` must be a whole number (`default=1234`, quoted or not), and `check` refuses anything else.
  An empty field stores it; with no default, OK is dimmed and Enter does nothing until a number is typed
  (a lone `-` is not one).

**What is stored**

- Control characters (newlines, tabs) are dropped, spaces at both ends are trimmed, and the
  text is cut to `max`.
- An empty answer stores the `default`. With no default, OK is dimmed and Enter does nothing
  until something is typed.
- A text variable gets text, even when it looks like a number: `n == "42"`, not `n == 42`. Use a number
  variable (above) for a number.
- Whatever the player types shows exactly as typed: `{`, `}`, `*` and `\` in a name can't
  become text tags when `{hero.name}` appears in a line or a prompt.

**Keys while the field is up**

- Letters and digits go to the field, so shortcuts like A, H, S, L, I, U and Tab do nothing
  there. Backspace deletes and never goes back.
- Esc and a right click open the Save screen; closing it returns to the field with the text
  kept. F5 and F9 quick save and load.
- On a gamepad, A confirms (the default when the field is empty), X deletes, and B or Start
  opens the Save screen. There is no on-screen keyboard.

**Rollback, saves and history**

- The input is an auto save point and stops skip and auto-forward.
- Neither the question nor the answer is added to the history.
- Rolling back from the line after it lands on the prompt with an empty field. The variable
  is back to its earlier value until the new answer.
- Saves hold the stored value; the text typed but not yet confirmed is not saved. A
  `persistent:` text keeps the last name for the next playthrough.

**The tools**

- `quillstone test` answers with the `default` (or `Player` when there is none) and goes on.
  So a line that only another answer reaches, like `::if{hero.name == "Zed"}`, is reported as
  never shown. For a number, it answers `0` and the numbers the script compares the variable to.
- `quillstone run` in the terminal asks on the command line. An empty line takes the default;
  `!back` and `!quit` are the commands there.

The demo `examples/greeting.quill.md` asks for a name and uses it in a few lines.

## Messages

Phone-style chats with the game's characters: messages arrive from the story, the player reads
them on a Messages screen, and answers either right now or later.

````markdown
---
vars: { going: false }
---
::character{id=mika name=Mika portrait=assets/mika_face.png}
::hud{id=phone text="Phone" open=messages x=0.9 y=0.05 back=#00000099 pad=10}

::message{from=mika text="Hi! The roof at eight?"}
::message{from=mika image=assets/selfie_cafe.png text="Look where I am"}
::message{to=mika text="Sure, I'll bring tea."}

::reply{to=mika}
- [Sure, see you at 8](#yes){going=true}
- [Can't tonight](#no)
::
````

**Sending.** `::message` is an instant step: the message joins the thread with a character and the
story goes on.

| Attribute | |
| --- | --- |
| `from=mika` | the character's message (left bubble). One of `from=` and `to=` is required |
| `to=mika` | the player's message to that character (right bubble) |
| `text="..."` | the text: `{name}` values, `**bold**`, colour and size tags like a line (not `{nw}` or `{p}`) |
| `image=assets/selfie.png` | a picture (PNG, JPEG or WebP), shown in the bubble |
| `video=assets/clip.webm` | a video, shown in the bubble as its thumbnail with a play mark |
| `thumb=assets/clip.png` | the video's thumbnail picture (without one the bubble shows a plain tile) |
| `quiet` | no notice for this message (`from=` only) |

- The contact is a `::character` (declared first), and the thread, the list and the portrait use
  its name and `portrait=`.
- At least one of `text`, `image` and `video` is needed; a text may go with a picture or a video as
  its caption, but not `image` and `video` together. `check` tests the files and `package` ships them.
- A message `from=` a character shows the notice "New message from Mika" (the same corner notice as
  `::notify`, in the player's language). The player's own messages never do.
- The text is worked out when it is sent: a later change to a variable does not change it.
  A thread keeps its last 100 messages.

**The Messages screen.** A HUD button with `open=messages` opens it (see [HUD](#hud-text-bars-and-pictures-over-the-story)); there is no key. It needs
a game that uses `::message` or `::reply`, and, like other HUD buttons, works while the story waits at a normal line.

- The **contact list** shows each character with a message or a reply: the portrait, the name, the last message,
  and how many are unread, or "Waiting for your reply". The most recent conversation is first.
- A **thread** shows the bubbles, theirs on the left and the player's on the right, with the newest at the
  bottom. Opening a thread marks it read. The wheel, Up, Down, Page Up, Page Down, Home and End scroll
  (on a gamepad the D-pad), and Back or Esc return to the list, then to the story.
- A **picture or video** in a bubble opens in a centred panel (up to two thirds of the screen, in its own shape)
  over the dimmed screen when clicked, or with Left/Right and Enter. A video plays there once with its
  sound; its clip is not played in the bubble. Click it to play it again; Back, Esc or a click outside closes
  the panel (and stops the video). The story's music keeps playing under it.
- Messages are not added to the History screen: the Messages screen is their history.

**Answering right now.** `::reply{to=mika}` followed by options and a closing `::` makes the story wait, like a
`::choice`: the Messages screen opens on that thread with the options at the bottom. The options are written
as in a choice (`- [text](#label){effects, if condition, once}`); the picked option's text joins the thread
as the player's message and the story goes to its label. Esc or a right click open the game menu, as
at a choice.

**Answering later.** `::reply{to=mika later}` has the same options, but the story does not wait: the
options sit in Mika's thread (the list marks it "Waiting for your reply") until the player opens it and picks one.

```
::reply{to=mika later}
- [Sounds good](#thanks){going=true}
- [Sorry, no](#sorry)
::
```

- The pick adds the player's message and **calls** the option's label, then the story comes back to the line it
  was at, like a HUD `call=` button: the label ends with `::return`, and can't take parameters.
  If the label only sends messages (and maybe a new reply), the Messages screen stays open on the thread, so a chat
  can go back and forth; if it shows a line or a choice, the story screen comes back.
- Like those buttons, it works only while the story waits at a normal line; otherwise a pick says "Can't use that now".
- A new `::reply{to=mika later}` for the same character replaces a pending one. `::reply{to=mika none}` (one
  line, no options) takes it away. `later` options have no `{default}` or `{show}`.

**Rollback, saves and replays.** Threads, unread marks and a waiting reply are part of the game: they are saved,
and going back un-sends what was sent after that point. A scene replay starts with no messages, and what it sends is not kept.

**Tools.** `quillstone check` names the line of every mistake (an unknown character, a missing picture, both `image=` and
`video=`, an option without a label...). `quillstone test` follows every option of a reply as a branch, a later one
too (once per option, like a HUD call button). `quillstone run` prints messages as `[Mika] text` (and
`[you to Mika] text`) and asks a reply the story waits for like a choice. Message texts and reply options are in
the translation export; a message whose text holds `{name}` values keeps the language it was sent in.

### Your own phone

Without any key the screen uses the theme's colours. These theme keys (see [Themes](#themes)) dress it:

```text
# a picture drawn behind the screen, covering it like a background (a phone body, a tablet...)
messages_image = gui/phone.png
# x, y, width, height of the area inside it where the list, thread, options and Back are laid out
# (at least 320x420; default the whole screen)
messages_rect = 722, 100, 476, 880
# the text size of bubbles, names and options (8 to 200, default 34)
messages_text_size = 26
# bubble colours (default: button_color for theirs, a see-through accent_color for the player's)
bubble_them = #46587c
bubble_me = #2f8f6bdd
# bubble pictures, used instead of the colours. With a border (the corner size in the picture's own pixels)
# they are drawn as nine pieces, so rounded corners keep their shape at any bubble size; without one the
# picture is stretched whole
bubble_them_image = gui/bubble_them.png
bubble_me_image = gui/bubble_me.png
bubble_border = 24
# the contact rows, the reply options and the Back button (a picture for hovering too), nine-sliced the same way
messages_button_image = gui/row.png
messages_button_hover = gui/row_hover.png
messages_button_border = 26
# a small picture (a "‹") instead of the labelled Back button; Esc and a right click still go back
messages_back_image = gui/back.png
messages_back_hover = gui/back_hover.png
# drawn over the contact's portrait (`portrait=` of the character) in a chat's header: an opaque picture with a
# round hole in the colour of the header shows the portrait as a circle. Without a portrait the header is the name only
messages_avatar_frame = gui/avatar_frame.png
# text colours in bubbles (default text_color)
bubble_text_them = #ffffff
bubble_text_me = #ffffff
```

- A frame picture and a rect are enough for a custom phone: draw the screen where the rect is.
- The enlarged picture panel is not bound to the rect: it stays centred over everything.
- The font is the theme's `font`; there is no separate font for the phone.
- A chat's header shows the contact's portrait left of the name when the character has one.
- `check` tests the keys and pictures like any other theme key, and `package` ships the pictures.

The demo `examples/phone.quill.md` sends messages and has both kinds of reply.

## Kept across playthroughs

````markdown
---
persistent: { saw_good_end: false }
gallery: [assets/cg_rain.png, assets/cg_kiss.png]
music_room: [assets/calm.ogg, assets/theme.ogg]
---
````

| Key | What it does |
| --- | --- |
| `persistent:` | variables that outlive a playthrough |
| `gallery:` | pictures and video clips for the main menu's **Extras** screen |
| `bonus:` | pictures and video clips in Extras that are open from the first launch |
| `music_room:` | music tracks for the Extras screen |
| `replays:` | scenes players can watch again from Extras: see [Scene replays](#scene-replays) |
| `chapters:` | tiles on the main menu's Chapters screen, each starting a new game at a label: see [Chapter select](#chapter-select) |

### Persistent variables

`persistent:` declares variables that outlive a playthrough: endings seen, routes unlocked.

- Use them like any variable. Whatever the story sets them to is still there in every new game
  and every loaded save, and rolling back doesn't undo it.
- Replaying a line that *adds* to one adds again, so yes/no flags are the safe kind.
- `quillstone test` plays the story from a first playthrough and also from later ones (each
  value the script gives a persistent variable).

### The gallery

`gallery:` lists pictures and video clips. Pictures and clips can be mixed freely:
`gallery: [assets/cg1.png, assets/clip1.webm]`.

**When an entry unlocks**

- A picture unlocks the first time the story shows it: `::bg` or an inline image. A
  `::bg{anim=...}` unlocks every listed picture that is one of its frames.
- A video (`.webm`, or any format `quillstone check` accepts for `::bg`) unlocks the first time
  the story plays it, as a looping `::bg` background or as a `::video`.

**Thumbnails.** An entry can name the picture its tile shows after a `|` (spaces around it are
optional):

```
gallery: [assets/cg1.png, assets/clip1.webm | assets/thumbs/clip1.png]
```

- The Extras screen can have a picture behind it: `extras_background` (see The Extras and Chapters screens).
- A thumbnail is a picture (PNG, JPEG or WebP) in the game folder. `quillstone check` reports
  a missing or unreadable one, and `package` ships it.
- It is only the tile of an unlocked entry. A locked tile stays a `?`, and the entry itself
  unlocks as above, never the thumbnail.
- Without a thumbnail, a picture's tile shows the picture. A clip's tile is a dark placeholder
  with a play symbol and the clip's name: no thumbnail is made from the video.

**Always open: `bonus:`.** Bonus art (wallpapers, concept art, a making-of clip) needs no story
event. `bonus:` takes the same entries as `gallery:`, thumbnails included, and its entries are
unlocked from the first launch:

```
gallery: [assets/cg1.png]
bonus: [assets/art1.png, assets/art2.png | assets/thumbs/art2.png]
```

- They show in the Extras gallery after the `gallery:` entries, and open, play and move with
  Left/Right just like an unlocked one. A game with only `bonus:` still has an Extras screen.
- The key may repeat. An entry can't be in both `gallery:` and `bonus:`.
- `quillstone check` looks at the files like any gallery entry, but doesn't ask the story to
  show them. `::unlock` on one is harmless: `check` warns that it is already unlocked.

**Watching a clip.** Opening a clip plays it on repeat, full screen and fitted like a picture,
with its sound. Click, Esc or Enter closes it; Left/Right move to the next unlocked entry,
picture or clip. The clip stops as soon as it is closed.

### The music room

`music_room:` does the same for `::music` tracks. They are named after their file:
`rainy_day.ogg` shows as "Rainy day".

For both lists:

- The keys may repeat to make long lists.
- `quillstone check` complains about entries the story never shows or plays, since they could
  never unlock.

### Unlocking by hand

`::unlock{src=assets/clip1.webm}` unlocks a `gallery:` or `music_room:` entry without showing
or playing it. Use it for a picture that is a composed scene, a clip or track unlocked by a
story flag, or a bonus.

- `src` is the entry's own file path, not its thumbnail. It must be a listed entry:
  `quillstone check` and the parser report anything else, with the line.
- `check` counts it as showing or playing the entry.
- Like any unlock it is kept across playthroughs: rollback and loading a save never take it
  away. `quillstone test` simply goes past it.

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

**Declaring.** `replays:` lists scenes the player can watch again. Each entry is a label and
the title shown in the menu.

- The label is written the way a `::jump` writes it: `# The rooftop` is `the-rooftop` (spaces
  and underscores become dashes). It must exist and take no parameters.
- Titles must be different and not empty. Write `\"` for a quote mark inside one. A long title
  is shrunk or cut to fit its row.
- Any number of scenes. Entries show in the order written, and several `replays:` lines add
  up.

**Unlocking.** A scene unlocks the first time the story reaches its label, however it gets
there: falling in from above, `::jump`, `::call`, a choice. Like gallery pictures, it stays
unlocked for good: rolling back or loading an older save never takes it away.

**The Scenes tab.** The main menu's **Extras** screen then has a third tab, **Scenes**.
(Extras appears when the game has a gallery, a music room or replays; Tab switches tabs.)
Unlocked scenes show their title, locked ones show `???`. Enter, a click or the gamepad's A
starts a scene.

**What a replay is.** A replay starts a **fresh game at that label**:

- the script's starting variables, and the player's persistent variables as they are;
- an empty stage: nothing carries over from whatever the player did;
- no menu music.

Everything works as in the story: choices, waits, hotspots, shops, text input, videos,
rollback (back as far as the start), history and settings.

**How it ends.** At **`::end_replay`**, or when the story ends (`::end`, `::return` with
nothing to return to, the end of the script). The player is then back on the Scenes tab, with
the menu music resumed. In the normal game `::end_replay` does nothing, so you can put it
where a scene ends and the story carries on after it. It takes no attributes.

**A replay changes nothing that lasts.**

- There is no auto save, and Save, Load, Quick save and Quick load show "Not available during
  a replay".
- Variables the scene sets, persistent variables, gallery, music room and scene unlocks, and
  which lines were read are all thrown away when it ends.
- Skip uses the player's real read record, so it stops at lines the player has not read yet.
- Settings' "Main menu" asks "Leave this replay?" and goes back to Extras. Quit still quits
  the program.

**From inside the game.** A HUD button with `open=extras` opens the same Extras screen over the
running story. A scene started there is a replay like any other, and the running game is kept
aside: when the replay ends (or the player leaves it) the Extras screen is back, and closing it
returns to the story on the same line, with the same variables, stage, music and HUD.

**The tools.** `quillstone test` warns about a replay whose label no branch can reach ("can
never be unlocked"). `quillstone check` warns when no `::end_replay` follows a scene, so it
would run to the end of the game; that is a scan along every jump and choice, so approximate.

The demo `examples/replay.quill.md` has two scenes.

## Chapter select

`chapters:` gives the main menu a **Chapters** button. Its screen shows one tile per chapter, and
picking an open tile starts a new game at that chapter's label:

```
chapters: { part-1: "Part 1" | assets/ch1.jpg, part-2: "Part 2" | assets/ch2.jpg {open} }
```

- **Each entry** is a label (as in `replays:`, written like a `::jump` target), a title in quotes,
  an optional thumbnail picture after a `|` (PNG, JPEG or WebP, in the game folder), and an
  optional `{open}`. The key may repeat to make a long list, and tiles show in the order written.
- **Unlocking.** A chapter unlocks the first time the story reaches its label, and stays unlocked
  in every later game; rollback and loading a save never take it away. `{open}` makes it
  available from the first launch. A locked tile shows `?`, like the gallery. `--unlock-all` opens
  every tile.
- **Starting.** An open tile starts a new game: the variables begin as in the front matter (as
  for New Game) and the story starts at the label. Persistent variables are kept. It plays
  normally, so saving, loading and rollback work as always.
- **The screen** shows 12 tiles a page, with arrows and "Page 1 of 2" when there are more. The
  thumbnail fills an open tile (a tile without one shows its title); the title is under it. Mouse,
  arrow keys with Enter, and a gamepad all work; Esc goes back to the main menu. A picture can
  go behind the screen: `chapters_background` (see The Extras and Chapters screens).
- **Errors** (line-numbered): a label that doesn't exist or takes parameters, the same label or
  title twice, or a thumbnail that is not a picture inside the game folder.
- `quillstone check` reads each thumbnail and warns about a chapter the story can never reach
  from its start (it could never unlock) unless it is `{open}`; `package` ships the thumbnails.
  `quillstone test` also plays on from every chapter label.

The demo `examples/chapter_select.quill.md` shows it.

## Translating a game

````markdown
---
title: My Story
languages: en=English, sv=Svenska
---
````

`languages:` lists the languages the game is offered in, as `code=Name`. Without it nothing
changes anywhere.

- The **first** is the one the script is written in. Every other language gets a translation
  file.
- Codes are short: letters, digits, `-` and `_`, at most 16. At most 64 languages.
- The name is what the player sees in Settings, so write it in that language itself.

With more than one language, Settings gets a **Language** row (click, Left/Right or the
gamepad cycle it). The choice applies at once to the line on screen, the choices, the history
and every other text, and is kept with the player's other settings.

Saves and rollback never contain translated text or the language, so they work the same
whichever language is chosen.

### The translation file

Each language besides the first has `lang/<code>.txt` in the game folder, like `lang/sv.txt`.
The same folder rules as every other game file apply: inside the script's folder, no `..`. Let
the tool create it:

```
quillstone lang export my_story.quill.md sv
```

It writes every translatable text of the script, once each, in script order. Run it again
whenever the script changes:

- new texts are added;
- texts that are gone are marked `! obsolete` and kept, with their translation, which comes
  back if the text returns;
- nothing a translator wrote is removed.

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

**What can be translated**

- dialogue and narration;
- choice and hotspot texts, with the reason of a locked option;
- character names, and item names and descriptions;
- `::hud` texts, `::input` prompts and `::notify` texts;
- the title, `replays:` titles and `chapters:` titles;
- the file names listed in `gallery:` and `music_room:`.

A text with no translation shows in the script's language, without any message.

**The game's own screens**

The texts the engine draws itself ("Settings", "Save", "Back", the settings rows, hints, questions
and short messages like "Loaded") are translated through the same file:

- `lang export` adds them after the script's texts, each with a `#:` place like `settings screen`
  or `message`. A text the script also has is one entry.
- A text with a changing part has a `{name}` in it, like `Page {n} of {pages}`. Keep it as in the
  rule below.
- A text with no entry, or an empty `=`, shows in English. A file made before these entries existed
  keeps working: run `lang export` again to add them.
- Changing the language in Settings changes the screens at once.
- Not translated: the developer console, the notices meant for the writer (a script that can't be
  read, a story that stopped), the names of keys, and dates and times.
- A translation longer than the English text is drawn smaller where there is a label that fits
  its space (the settings values, the quick menu); elsewhere keep it short.

**Rules for a translation**

- Keep every `{name}` and every text tag in the translation, as often as in the original:
  `{nw}`, `{p}`, `{w=..}`, `{size=..}`, `{color=..}`, `{u}`, `{s}`, `{speed=..}`, `{whisper}`,
  `{shake}`, `{wave}`, `{glitch}`, `{ruby=..}` and their closing tags. `{name}` values are put in after the translation is chosen, and `{nw}` must still end
  the line.
- A tag's own value (a colour, a pause, a shake strength, a ruby reading) may differ, so a translation can give
  its own reading.
- The theme's `font` has to cover the characters the translations use.

**Checking and shipping.** `quillstone check` reads every translation file:

- syntax errors are reported with `file:line`;
- a translation that drops, adds or misspells a `{name}` or tag is an error;
- texts the script no longer has are warnings;
- a count of untranslated texts (the game's own screens included) is printed for each language.

`quillstone package` ships the `lang/*.txt` files of the languages the script lists.
Translation files are limited to 8 MB, 100,000 entries and 16 KB per text.

### Patching text with a mod

A mod can change texts of the game without copying a whole script. Every `*.txt` in `mods/text/`
(in the `mods/` folder beside the player or the `.qsa`, see the README) is a **text patch**, in
exactly the translation file format above: `>` the game's text, `=` what to show instead, and
`@ Speaker` for one character's lines only.

```
# mods/text/10-fixes.txt
@ Mika
> You came after all.
= You came. I'm glad.
```

- A patch changes the texts of the game's first language (the one the script is written in), and
  everything a translation can change, the game's own screens included.
- Several patches apply in file name order and a later file wins for the same text; start the
  names with numbers (`10-fixes.txt`, `20-more.txt`) to choose the order.
- When the player picks another language, that language's own translation shows and patches do
  not apply. To change a translation, replace `lang/<code>.txt` with a file of the same name in
  `mods/`.
- An entry whose text is not in the game any more (after a game update) is ignored. A patch that
  is not valid is reported like a translation file, with `file:line`, and left out.
- Saves never hold the text, so patches can be added and removed between sessions.

`quillstone mod export my_story.quill.md > mods/text/10-mine.txt` writes every text with an empty
`=` to start from. `quillstone mod check my_story.quill.md mods/text/*.txt` lists the entries that
match nothing in the current script and the errors in the files, to see what a game update broke.
While developing, a `mods/` folder inside the game folder works the same; `package` leaves it out.

## What rollback restores

Going back one step restores:

- **The story:** variables, the random seed (so a roll repeats), the call stack, and the line
  itself with its inline images and voice.
- **The stage:** the background; sprites and their positions, heights, depths, sizes, stretch,
  mirroring, `alpha` and pixel offsets, `blend=add` and layers; animations and looping moves
  (from the start of their cycle); the camera.
- **The page:** the lines of a full-screen page.
- **Effects that stay:** the colour cover of `::fade{out}`, the ambient particles' settings,
  the colour wash of `::tint`, the dark edges of `::vignette`, the colour filter of
  `::filter`, and the HUD elements (and whether `::hud{off}` is in effect).
- **Sound:** music (an intro is not resumed part-way) and looping sound effects.

One-shot sound effects, shakes and flashes are not replayed.

**How far.** Rollback, and roll forward (the redo after a rollback), keep the last 500 steps.
Going back further simply stops there, and the backlog lists what that history holds.

**Steps that move on by themselves.** Rollback goes past a timed `::wait`, the wait of a
`::fade{out}`, a video and a line ending in `{nw}`. Roll forward is dropped when the state
rolled back to is one of those, because there is nothing to come back to. A `::wait{click}` is
a step like a line: rollback stops there.

**Blocking rollback.** `::rollback{block}` (a command of its own, on its own line) stops the
player going back past it, for a decision that should stay made or a fight that can't be
re-played:

```
**Mika:** That's the last of them.
::rollback{block}
**Mika:** Let's go.
```

- When the story runs it, the history before it is dropped: the Back button, the Backspace
  and Left keys and the mouse wheel stop at the line after the block, and a notice says
  "Can't go back further" (translatable like the engine's other texts). Going forward, and
  going back to anything after the block, work as before.
- The backlog (History) lists only what came after the block, since it is built from that same
  history.
- Saves and loads keep it: a save holds the shortened history, so a load can't go back past
  the block either. A new playthrough starts with no block.
- Replays and `quillstone test` are unaffected.

**Persistent variables** are never rolled back. A rollback or a loaded save shows the screen
(choices, hotspots, shop stock, a line's `{variables}`) rebuilt with their current values.

## Saves and edits

Saves remember positions as *label + how far past it*, so editing a script doesn't break
existing saves.

- Anything added or removed above a label leaves saves after that label where they were.
- If a saved spot no longer matches the script (say, a line was inserted just before it), the
  game picks up from that spot instead.
- A save whose label was renamed or removed can't be loaded.
- Moving part of a story into an `::include` (or back) changes nothing for saves, since they
  are checked against the one compiled story.
- A save that was waiting at a choice, hotspot block or shop where nothing is on offer any
  more (an edited condition) carries on past it.
- Choice texts, a timed choice's time limit and default, hotspots, shop stock and input
  prompts always come from the script, never from the save.
- Skip mode remembers read lines by their label and text, so rewording a line makes it
  "unread" again.

The same rules apply when the player reloads a script you just saved (hot reload): it carries
on where you are.

**Shared saves.** Saves are shared between players, so loading is strict:

- A save is unloadable if it holds a file path outside the game folder, a list of more than
  10,000 items, more than 1,000 stored states or more than 256 return addresses.
- A variable of the wrong kind (a list where the script declares a number, or the reverse) is
  ignored.
- Numbers like positions, sizes, volumes and waits are clamped to what the language allows.

## Themes

The look of the game lives in a small text file named by `theme:` in the front matter.

- Every line is `key = value` and optional. Anything left out keeps the built-in look.
- A line starting with `#` is a comment. A `#` after a value is part of the value, so put
  comments on their own line.
- Paths are relative to the script.
- `quillstone check` validates the file. It checks that everything it names exists, is inside
  the game's folder, and is the right kind of file: fonts are `.ttf` or `.otf`, pictures are
  PNG, JPEG or WebP.

```text
# used for all text; else the system font
font = assets/fonts/MyFont.ttf
# for **bold** (same family)
font_bold = assets/fonts/MyFont-Bold.ttf
# for *italic*
font_italic = assets/fonts/MyFont-Italic.ttf
font_bold_italic = assets/fonts/MyFont-BoldItalic.ttf
# dialogue and buttons (8 to 200)
text_size = 38
# a border round dialogue and name text, 0 to 8 px (default 0: none)
text_outline_size = 2
# its colour (default black)
text_outline_color = #000000
# the speaker's name plate (48 px high: bigger names shrink to fit, down to 60%, so keep it under about 60)
name_size = 36
text_color = #ffffff
# name plates of characters without color=
name_color = #ffd98c
# highlights, the title on the main menu
accent_color = #ffd98c
# hints and secondary text
dim_color = #ffffff8c
# the textbox, if there is no textbox_image
panel_color = #000000b8
button_color = #000000b3
button_hover_color = #40336be0
# behind settings, history and save/load
overlay_color = #050510e0
# stretched over the textbox (1600x260 unless textbox_rect says otherwise)
textbox_image = assets/gui/textbox.png
# over the whole screen under a full-screen page's text (default #000000cc)
page_color = #000000cc
# the signed amount of a ::notify{delta=}: up (default #73d980) and down (default #ff7373)
notify_up_color = #73d980
notify_down_color = #ff7373
# x, y, width, height the page's text fills (default 260, 120, 1400, 840)
page_rect = 260, 120, 1400, 840
# visible (default), hover or hidden (see below)
quick_menu = visible
# all (default) or listed: only buttons with a quick_<id>_icon or _pos (see below)
quick_buttons = all
# right (default), center or left: where the row sits along the textbox (see below)
quick_align = right
# a picture, or a video file that loops silently on the main menu
menu_background = assets/gui/menu.png
# a frame, vignette or shading drawn over menu_background, under the menu's buttons
menu_overlay = assets/gui/menu_frame.png
# the same behind Save, Load, Settings and History, over their dimming
game_menu_overlay = assets/gui/game_frame.png
menu_music = assets/audio/menu.ogg
# music and sound loops play at this fraction of their volume while a voice clip does, 0 to 1 (default 1: off)
voice_duck = 0.5
# sprites of those not speaking are drawn at this brightness, 0 to 1 (default 1: off; see below)
speaker_highlight = 0.6
# the countdown bar of a timed ::choice (default accent_color)
choice_timer_color = #ffd98c
# its thickness, 2 to 40
choice_timer_height = 8
# the ::timing bar's track and marker pictures (zone and marker colours too: see "Timing bar")
timing_bar_image = assets/gui/lockbar.png
timing_marker_image = assets/gui/needle.png
# the key cap of a ::press prompt (see "Key prompts")
press_key_image = assets/gui/keycap.png
```

Colours are `#rrggbb` or `#rrggbbaa` (the last pair is opacity).

**`speaker_highlight`** dims the sprites of those not speaking while a character who is on
stage speaks. The speaker is the character whose `name` is the line's speaker and whose id is
a shown sprite's id. Narration, and lines by a character with no sprite on stage, dim nobody.
The change eases over a quarter of a second.

### The dialogue box

The textbox, the name plate and the quick menu under them can be moved and styled.
Positions and sizes are in the 1920x1080 virtual screen; without these keys the box looks as
it always has (a 1600x260 box at 160, 780).

```text
# x, y, width, height; inside the screen, at least 400x120
textbox_rect = 360, 700, 1200, 340
# text inset: left, top, right, bottom (or one number for all four)
textbox_padding = 40, 36, 40, 20
# stretched over the name plate, under the speaker's name
name_plate_image = assets/gui/plate.png
# x, y, width, height of the name plate
name_plate_rect = 380, 640, 400, 64
# x, y, width, height the speaker's portrait is fitted into
# (left out: a 320x320 square standing on the textbox's left end)
portrait_rect = 160, 460, 320, 320
# the dialogue text: left (default) or center
text_align = center
# the speaker's name: left (default) or center
name_align = center
```

- The padding must leave at least 200x60 px for the text inside the textbox, otherwise it
  is reported and the default is used. The top inset is for a line without a speaker; with
  one, the text starts 10 px below the name plate when the plate is inside the box.
- Without `name_plate_rect` the plate is a 48 px strip at the top of the textbox, 22 px down and
  inset by the padding's left and right. The rect is absolute, so it can sit above the box.
- `portrait_rect` is at least 32x32 and inside the screen. The picture is fitted inside it whole, keeping
  its proportions, centred and standing on the bottom edge. It is drawn over the textbox and under the
  text, so it may sit inside the box. The engine does not move the text: when you place it inside
  the textbox, widen `textbox_padding` on that side (for example `textbox_padding = 260, 36, 40, 20`).
- Choice buttons stack centred above the top edge of the textbox (and shrink if there is little
  room, so leave some above it), and inline pictures are drawn above it too.
- `page_rect` is the area the text of a full-screen page (see [Full-screen pages](#full-screen-pages))
  is laid out in: inside the screen, at least 600x300. `page_color` is drawn over the whole screen
  under it.
- `quillstone check` lays lines out with the same text engine and fonts as the player, and warns about
  lines the player would shrink or cut off in the real textbox and padding (the warning says
  how much it is shrunk), and checks
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
# stretched over the panel instead of panel_color
input_image = assets/gui/input.png
# x, y, width, height; inside the screen, at least 600x460
input_rect = 360, 310, 1200, 460
```

The prompt, field and OK button keep their sizes and sit inside the panel (a taller panel centres
them; a wider one widens the prompt and the field). Clicking OK follows the panel. `check` reports a
missing or unreadable picture; `package` includes it.

### The settings screen

The Settings screen (not the main menu, Save, Load or History) can be moved, made smaller and
dressed with pictures. Without these keys it looks as it always has.

```text
# x, y, width, height of the panel; at least 640x420 (default 400, 90, 1120, 910)
options_rect = 80, 60, 960, 960
# the screen's text size: rows, buttons and hint scale with it (8 to 200, default 36)
options_text_size = 30
# full-screen picture instead of overlay_color
options_background = assets/gui/options.png
# drawn over the panel as a 9-slice
options_panel_image = assets/gui/frame.png
# the 9-slice corner size in the picture's pixels (0 to 400; 0: stretch whole; default 0)
options_panel_border = 40
```

- The title, the rows, the buttons and the hint are laid out inside `options_rect`: the title
  at its top, the hint at its bottom, the slider tracks across the middle, the buttons
  centred. Mouse and keyboard follow the new layout.
- The panel must be tall enough for the rows: about 860 px at the default text size (less for
  a smaller `options_text_size`). It must be wide enough for the values: about 800 px at the
  default size.
- A 9-slice keeps the four corners (`options_panel_border` pixels square, shown at that size in the
  1920x1080 screen) and stretches the edges and the middle, so rounded corners come from the picture.
  The border is cut back if it is more than half of the picture or of the panel.
- `check` reports a missing or unreadable picture; `package` includes the pictures.
- Besides text speed, volumes and the rest, the screen has an **Animated text** on/off button (on by
  default, saved as `animated_text`; see [Text tags](#text-tags)) and a **Text size** slider: 100% to 150%
  in steps of 10%, 100% by default. It is saved in `settings.txt` as `text_scale = 1.2`.
  It scales the story's text that the theme sizes with `text_size`: dialogue, choices and the
  like, not the menus, Settings or History. If a line is too long for the textbox at the bigger
  size, it is shown at the largest smaller size that fits (never below `text_size`), so it never
  spills out of the textbox. Writers need to do nothing: `quillstone check` still judges lines
  at `text_size`.

### The main menu

The main menu's buttons and title can be moved and scaled. Without these keys it looks as it always has.

```text
# x, y, width, height of the area the buttons stack in; at least 200x240 (default 750, 400, 420, 556)
menu_rect = 120, 420, 360, 480
# where the game's title is drawn, centred (default 0, 180, 1920, 160)
menu_title_rect = 120, 200, 360, 160
# hide: no title, for a background picture that already shows the logo (show or hide, default show)
menu_title = hide
```

- The buttons are as wide as `menu_rect`. Their height, the gap between them and their label size
  scale with its height: the default 556 px holds six buttons, and a smaller area makes them smaller.
- The title is drawn centred in `menu_title_rect`, its text size scaling with that rect's height
  (96 px at the default 160).
- Mouse and keyboard follow the new places. Save, Load, Settings and the other screens opened
  from the main menu are not moved by these keys.

### The Extras and Chapters screens

The Extras screen (gallery, music room, scenes) and the Chapters screen can each have a picture
behind them. Without these keys they look as they always have.

```text
# full-screen picture instead of overlay_color
extras_background = assets/gui/extras.png
# the same for the Chapters screen
chapters_background = assets/gui/chapters.png
```

- The picture fills the screen, cropped to fit. A picture opened full-size from the gallery keeps
  its own black backdrop.
- `check` reports a missing or unreadable picture; `package` includes the pictures.

### The Save and Load screens

The Save and Load screens can be moved and scaled. Without this key they look as they always have.

```text
# x, y, width, height of the area they are laid out in; at least 760x495 (default 200, 40, 1520, 990)
slots_rect = 300, 100, 1000, 650
```

- The title, the page tabs, the six slots (three across, two down) and the Back button are laid
  out inside `slots_rect`. Positions and sizes scale with it across and down. Text sizes scale with the
  smaller of the two.
- A slot's picture keeps its 16:9 shape: it is the biggest one that fits in the slot, at its top.
  The time and the preview text sit under it.
- The row of game-menu tabs along the top (Save, Load, Settings, ...) stays where it is, so keep
  `slots_rect` below y = 40. Extras keeps its own layout.
- Mouse and keyboard follow the new places.

### The History screen

The History screen can be moved, scaled and given another text size. Without these keys it looks as it always has.

```text
# x, y, width, height of the area it is laid out in; at least 800x495 (default 160, 40, 1600, 990)
history_rect = 300, 100, 1200, 800
# the log's text size (8 to 200, default 32)
history_text_size = 28
```

- The title is at the top of `history_rect`, the hint at its bottom, and the log fills the space
  between. As many whole entries are shown as fit; the wheel and the keys scroll through the rest.
- The title and hint sizes scale with the rect, the smaller of its two scales.
- `history_text_size` sets the log's text. The line spacing, the gap between entries and the
  column of speaker names grow with it. The name column is never wider than 40% of the rect.
- A click anywhere closes the screen, as before. The tab row along the top stays where it is, so
  keep `history_rect` below y = 40.

### The game menu

Esc and a right click during the story (also over a shop, an inventory or a text input) open
the Save screen. A row of tabs runs along the top of the Save, Load, Settings and History
screens: Save, Load, Settings, History, Main menu and Quit.

- A click on a tab, or the Tab key (next screen, round again), moves between them. Esc or a
  right click closes back to where the player came from.
- Main menu and Quit ask first, like the Settings buttons.
- From the main menu the row shows Load, Settings and Quit only.
- During a scene replay Esc opens Settings instead, where Main menu leaves the replay.
- The tabs use the theme's `button_color`, `button_hover_color` (current tab and hover),
  `text_color` and `accent_color` (the current tab's label).
- A Settings panel placed with `options_rect` above y = 40 is covered by them.

```text
# full-screen picture behind Save, Load, Settings and History instead of overlay_color
game_menu_background = assets/gui/game_menu.png
```

- On Settings, `options_background` still wins when it is set; `game_menu_background` is the
  fallback. `game_menu_overlay`, if set, is drawn over the picture.

### The quick menu

The quick menu sits in a row directly below the textbox: Back, History, Skip, Auto, Save,
Load, Q.Save, Q.Load, Settings, and Items and Journal in a game with an inventory or a journal.

`quick_align` says where along the textbox the row sits:

| `quick_align` | The row |
| --- | --- |
| `right` | ends at the textbox's right edge (default) |
| `left` | starts at its left edge |
| `center` | is centred on it |

The row is always moved as far as needed to stay on the screen. When there is no room below
(the textbox ends less than 36 px above the bottom of the screen), the row is drawn just
inside the bottom edge of the textbox instead.

`quick_menu` says when it shows:

| `quick_menu` | The row shows |
| --- | --- |
| `visible` | always (default) |
| `hover` | only while the cursor is over the textbox or the row, fading in and out in about 0.15 s; buttons that aren't shown can't be clicked |
| `hidden` | never: it is not drawn or clickable |

The keyboard and gamepad shortcuts work in every mode. Players can override the mode in
Settings (Quick menu: Game default, Always, On hover, Hidden).

### Picture buttons

A theme can replace the quick menu's text labels with pictures and put buttons where it likes.
Every key is optional and names a button by its id: `back`, `history`, `skip`, `auto`, `save`,
`load`, `qsave`, `qload`, `settings`, `items` or `journal` (Q.Save and Q.Load are `qsave` and
`qload`).

```text
# idle picture: drawn instead of the text label
quick_back_icon = assets/gui/back.png
# while the cursor is on it (needs _icon)
quick_back_hover = assets/gui/back_hover.png
quick_skip_icon = assets/gui/skip.png
# while Skip or Auto is switched on (needs _icon)
quick_skip_on = assets/gui/skip_on.png
# top left in the 1920x1080 screen
quick_back_pos = 160, 976
# width, height (16 to 512 each)
quick_back_size = 56, 56
```

**The files.** `_icon`, `_hover` and `_on` are PNG, JPEG or WebP files. An id that isn't in
the list, a bad number, or a `_hover` or `_on` without its `_icon` is reported like any other
theme mistake.

**Size**

- Without `_size`, a picture button is as big as its picture (kept between 16 and 512 px on a
  side). A text button is 150x32.
- A text label wider than its button is drawn smaller (down to 10 px) so that it stays inside
  it. The button keeps the size the theme gave.

**Position**

- A button with `_pos` leaves the row and sits there, pulled back in if it would leave the
  screen.
- The other buttons keep the row below the textbox, placed by `quick_align`. Those to the left
  of the gap shift right to close it.
- A picture button without `_pos` stays in the row, as wide and tall as its picture.
- `_pos` and `_size` work on a text button too.
- Where placed buttons overlap, the one later in the list `back, history, skip, auto, save,
  load, qsave, qload, settings, items, journal` is on top and gets the click.

**How a button is drawn**

- Without `_hover`, the picture is drawn a little dimmed (80%) until the cursor is on it. With
  one, it is drawn whole and swapped for the hover picture.
- While Skip or Auto is on, the `_on` picture shows. Without one, the idle picture stays lit
  and gets a thin accent line along its bottom edge.
- A button that does nothing right now (Back on the first line, Q.Load before any quick save)
  is drawn at 40%.

**Showing only some buttons.** `quick_buttons = listed` shows only the buttons that have an
`_icon` or a `_pos` of their own and hides the other built-in ones; their keyboard and gamepad
shortcuts still work. The default `quick_buttons = all` keeps every button.

**Everything else is unchanged:** `quick_menu` (`hover` mode fades pictures too, and the menu
also shows while the cursor is on a placed button), the keyboard and gamepad shortcuts,
`::textbox{hide}`, hiding the interface with U, and the screens that have no quick menu.

**Checking.** `quillstone check` reports a missing or unsupported picture as an error, and
warns when two buttons overlap or a placed button reaches past the screen. `package` includes
the pictures.

`examples/iconmenu.quill.md` and `examples/icons_theme.txt` show it.

### The inventory and shop screens

One grid layout serves both screens (and the [journal](#journal)), so they share the `inventory_` keys. Without
any of them the screens look like the other built-in ones. Positions and sizes are in
the 1920x1080 virtual screen.

```text
# full-screen picture (else the screen is dimmed)
inventory_background = assets/gui/bag.png
# the shop's own background, if different
shop_background = assets/gui/stall.png
# stretched over the panel, under the grid
inventory_panel = assets/gui/panel.png
# x, y, width, height of the panel (default 210, 90, 1500, 900)
inventory_panel_rect = 160, 70, 1600, 940
# cells per row (1 to 12; fewer if they don't fit)
inventory_columns = 4
# width, height (or one number for a square); width 48 to 600, height 120 to 600
inventory_cell_size = 270, 210
inventory_cell_gap = 24
# the icon is drawn in a square this big
inventory_icon_size = 104
# a cell: a colour #rrggbb[aa] ...
inventory_cell = #4a3020
# ... or a picture, stretched over the cell
inventory_cell_hover = assets/gui/hover.png
# an owned item in a shop
inventory_cell_owned = #2f4a2f
# an item the player can't afford
inventory_cell_disabled = #2a221cb0
# default "Items"
inventory_title = Your pack
# the journal's title (default "Journal")
journal_title = Quests
# default "Shop"
shop_title = Night market
# what the shop calls its money (default: the pay variable's name)
shop_currency = Gold
# where the hovered item is described
inventory_description_rect = 300, 760, 1300, 170
# item names (default text_color)
inventory_name_color = #f6e7cf
# prices and the money line (default accent_color)
inventory_price_color = #f2b35c
# descriptions (default text_color)
inventory_text_color = #e6d3b3
```

| Key | Default | Range |
| --- | --- | --- |
| `inventory_columns` | 5 | 1 to 12 |
| `inventory_cell_size` | 240, 200 | width 48 to 600, height 120 to 600 |
| `inventory_cell_gap` | 24 | 0 to 100 |
| `inventory_icon_size` | 96 | 16 to 400 |

The panel rect holds the title, the grid, the description box and the buttons, laid out from
its top left. So a bigger panel, or cells that don't fit it, give fewer rows and more pages
rather than overflowing.

`quillstone check` validates the pictures and the colours. A line that can't be read is
reported with its line number and ignored.

## Developer tools

For trying a story, not for players: a game you ship has none of this unless it is started with `--dev`.

### The console

`quillstone-player <game> --dev` turns on the console. Press F12 while the story is showing: a
line opens at the top of the screen, and the game stands still until Esc (or F12 again) closes
it.

- Without `--dev` the key does nothing. Folder mode does not turn it on by itself, so you test
  the game the way players get it.
- The console opens only over the story, not over a text input, shop, settings or a scene
  replay.

Commands are one line, then Enter. Errors show as a line and leave the console open.

| Command | What it does |
| --- | --- |
| `get <var>`, or just the name | shows the value (`gold = 5`, text in quotes, lists in brackets) |
| `vars [prefix]` | lists the variables, sorted, at most 10; the rest is counted, and a prefix narrows it |
| `set <var> = <expression>` | the same assignment `::set` takes (`=`, `+=`, `-=`, `random(1,6)` too) |
| `jump <label>` | goes there now, as `::jump` does |
| `where` | shows the label the story is in and the script file and line (`start (story/02.quill.md:120)`), and with `::call`s open, `called from` their labels, innermost first |
| `help` | lists the commands |

- **`set`.** A variable the script declares keeps its kind: a number can't become text, and a
  list changes with `+=` / `-=` or `= [..]`. The line on screen is redrawn with the new value.
- **`jump`.** Variables, call stack and the picture stay. It is an ordinary step, so rollback
  comes back to where you were.
- **What the console changes is real:** saves made afterwards include it.
- Setting a variable is not a rollback step: rolling back to an earlier screen brings that
  screen's variables back.

### Starting somewhere else

```
quillstone-player <game> --dev --start <label> --set gold=50 --set met=true
```

This starts a new game at a label with those variables.

- `--set` repeats. The value is read by the variable's declared type, and text needs no
  quotes. The variable must be declared in `vars:`.
- `--start` and `--set` without `--dev` are refused.
- `quillstone run <script> --start <label> --set var=value` does the same in the terminal.

### Unlocking the Extras

`quillstone-player <game> --unlock-all` shows every gallery, music room and scene entry as
unlocked for that run, to test the Extras screen. Nothing is saved, and it needs no `--dev`.

## Not (yet) in the language

Pasting into a text input,
character voices per speaker, and transitions other than fades, wipes and masks.
The runtime is built so these slot in without changing what is written above.
