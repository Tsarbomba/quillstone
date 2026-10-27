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
  main menu), `theme:` (a theme file, see [Themes](#themes)), `vars:`, and
  `persistent:`, `gallery:` and `music_room:` (see
  [Kept across playthroughs](#kept-across-playthroughs)).
- **Headings** (`#` to `######`) are *labels*: places to jump to. `# The Roof` is
  the label `the-roof` (lowercase, words joined by dashes).
- **`**Name:** text`** is dialogue. Any other prose is narration. Consecutive
  lines join into one paragraph; a blank line, a new `**Name:**` or a command ends it.
- **`<!-- comments -->`** may span several lines and are ignored. Inside a
  `::choice` block they must fit on one line.
- **Commands** are one line: `::name{key=value key="text with spaces" flag}`.
  Times are in seconds. Paths are relative to the script file and must stay in its
  folder or below it (no `..`, no drive letters), with `/` between folders.

## Variables

Values are whole numbers, `true`/`false`, or `"text"`. Declare starting values in
`vars:`; any other variable springs into existence at `0` when first read.
Names are letters, digits and `_`, but not `true`, `false`, `and`, `or` or `not`.

- `::set{trust+=1, met=true, name="Alex"}` sets one or more (`=`, `+=`, `-=`;
  `+=` on text appends). The right-hand side may be another variable: `gold -= price`.
- `{name}` inside a line or choice text is replaced by the variable's value.
  `\{` writes a literal brace; an unknown `{name}` is left as written.

## Text tags

Inside lines and choice texts:

| Write | Shows |
| --- | --- |
| `**bold**`, `*italic*`, `***both***` | bold, italic, bold italic |
| `{color=#ff8080}words{/color}` | words in that colour (`#rrggbb` or `#rgb`; they nest) |
| `Well...{w=0.5} no.` | the typing pauses for half a second there (up to 10; a click skips it) |
| `\*`, `\{`, `\\` | a literal `*`, `{` or `\` (any other `\` is shown as written, so `C:\temp` is fine) |

A `{` with no `}` after it is just a brace, and an unknown `{name}` is left as
written. Mistakes (an unclosed `**`, an unknown tag like `{size=40}`) are errors
with the line number. A variable's value always shows as it is, even if it
contains `*` or braces. The history and save previews show the words without
styling. For bold and italic in a theme font, give the theme its bold and
italic files too (`font_bold`, `font_italic`, see [Themes](#themes)).

## Conditions

Used by `::if`, `::jump{if=...}` and choice options `{if ...}`:
`==  !=  <  <=  >  >=` (a lone `=` also means equals), `and` / `or` / `not`
(also `&&` `||` `!`), parentheses, and a bare name means "is not 0/false/empty".
`trust > 1 and not met`, `name == "Mika"`, `(a or b) and c`.

`seen(label)` is how many times the story has passed that label (0 if never), so
it works both as a yes/no and as a count: `::if{seen(cafe)}`, `seen(cafe) >= 3`.
The label is written like a link target (`seen(the-roof)`, or `seen(#The Roof)`).
Rollback and saves keep the counts; they belong to one playthrough.

## Scene

| Command | What it does |
| --- | --- |
| `::bg{src=... fade=1}` | Background; crossfades over `fade`. May be a **video** file (plays looped). |
| `::bg{src=... fade=1 wipe=right}` | Wipe instead of crossfading: the new picture slides in from one side over `fade` seconds. `wipe`: `left`, `right`, `up`, `down` (the direction the edge travels). |
| `::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}` | Declare a character: name-plate text and colour, and a sprite path where `{pose}` is filled in. Declare before use. `**Mika:**` lines pick up the name and colour automatically. |
| `::show{mika happy at=left fade=0.4 scale=1}` | Show a sprite (or change its pose; the new pose crossfades and keeps its position and size). `at`: `far-left` `left` `center` `right` `far-right`, a number 0 to 1, or `30%`. Without `at` it keeps its position, or centres if new; without `scale` (0.05 to 10) it keeps its size, or is 1 if new. |
| `::show{src=assets/ghost.png id=ghost at=center}` | A sprite that isn't a character. |
| `::hide{mika fade=0.5}` / `::hide{all}` | Remove one sprite or all. |
| `::move{mika at=right time=0.8}` | Slide a sprite. |
| `::move{mika scale=1.2 time=0.5 ease=out}` | Grow or shrink it (can be combined with `at`). `ease`: `inout` (default: slow at both ends), `in`, `out`, `linear`. |
| `::camera{zoom=1.3 x=0.3 y=0.4 time=4}` | Pan and zoom the stage (background and sprites; the textbox and menus stay put). `zoom` 1 to 4; `x`, `y` (0 to 1, or `30%`) is the point to centre on, kept so the view never leaves the stage. Anything left out stays as it is; `ease=` as for `::move`. Doesn't wait. |
| `::camera{reset time=1}` | Back to the whole stage. |
| `::shake{time=0.4 strength=20}` | Shake the stage, dying away over `time` seconds (`strength` in pixels at 1920x1080, up to 200). Doesn't wait. |
| `![alt](assets/x.png)` | An inline image. It appears **with the next line that waits** and disappears when the player advances, so put it *before* the line it illustrates. (`![](x.webm)` plays a video.) |

Sprites stand on the bottom edge of the screen, about 940 px tall at `scale=1`
(on a 1920x1080 screen), and the text box draws over their lower part. The
camera and sprite sizes are part of the scene, so rollback and saves restore
them; a shake is a moment and isn't replayed.

## Sound and video

| Command | What it does |
| --- | --- |
| `::music{src=... fade=1 volume=0.8 loop=false}` | Background music; loops unless `loop=false`. `volume` 0 to 1. A different track crossfades over `fade` seconds. |
| `::music{stop fade=2}` | Stop it. |
| `::sfx{src=door.ogg volume=0.5}` | One-shot sound effect. |
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
- Rollback keeps the music going if the earlier moment had the same track, and
  switches or stops it otherwise. A voiced line's clip plays again when you roll
  back to it. Sound effects are never replayed by rollback.

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

Going back one step restores variables, the call stack, background, sprites and
their positions and sizes, the camera, music, the inline images and voice of the line, and the line
itself. Sound effects are not replayed.

## Saves and edits

Saves remember positions as *label + how far past it*, so editing a script
doesn't break existing saves: anything added or removed above a label leaves
saves after that label where they were. If a saved spot no longer matches the
script (say, a line was inserted just before it), the game picks up from that
spot instead. A save whose label was renamed or removed can't be loaded. Skip
mode remembers read lines by their label and text, so rewording a line makes it
"unread" again. The same rules apply when the player reloads a script you just
saved (hot reload): it carries on where you are.

## Themes

The look of the game lives in a small text file named by `theme:` in the front
matter. Every line is optional; anything left out keeps the built-in look.
Paths are relative to the script. `quillstone check` validates the file and
checks that everything it names exists.

```text
font = assets/fonts/MyFont.ttf      # used for all text; else the system font
font_bold = assets/fonts/MyFont-Bold.ttf          # for **bold** (same family)
font_italic = assets/fonts/MyFont-Italic.ttf      # for *italic*
font_bold_italic = assets/fonts/MyFont-BoldItalic.ttf
text_size = 38                      # dialogue and buttons (8 to 200)
name_size = 36                      # the speaker's name plate
text_color = #ffffff
name_color = #ffd98c                # name plates of characters without color=
accent_color = #ffd98c              # highlights, the title on the main menu
dim_color = #ffffff8c               # hints and secondary text
panel_color = #000000b8             # the textbox, if there is no textbox_image
button_color = #000000b3
button_hover_color = #40336be0
overlay_color = #050510e0           # behind settings, history and save/load
textbox_image = assets/gui/textbox.png   # stretched over the 1600x260 textbox
menu_background = assets/gui/menu.png
menu_music = assets/audio/menu.ogg
```

Colours are `#rrggbb` or `#rrggbbaa` (the last pair is opacity).

## Not (yet) in the language

Player text input (asking for a name), multiple script files / `::include`,
character voices per speaker, and transitions other than fades and wipes.
The runtime is built so these slot in without changing what is written above.
