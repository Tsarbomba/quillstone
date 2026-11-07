---
title: Staging
---

<!-- Vertical placement (`y=`), drawing order (`z=`), the `{size}`, `{u}`, `{ruby}`, `{p}` and `{nw}`
     text tags, `blur=` on the background and on a sprite, `tint=` on a sprite, a light overlay (`blend=add`), `::flash`, a looping `::sfx` that is started and later stopped, a shake along
     one axis, a flipped and a stretched sprite, a move sequence with `alpha` and `xoffset`, a dip to black (`::fade` and `through=`), a `::tint` wash, a `::vignette`, a `::filter` and a timed choice.
     See SCRIPT.md. Ren is Mika's picture with red and blue swapped, so overlaps are easy to tell apart. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}
::character{id=ren name=Ren color=#9ad1ff sprites=assets/ren_{pose}.png}

# stage

::bg{src=assets/rooftop.png fade=0.5}
::show{mika neutral x=0.3 scale=0.8 fade=0.4}
**Mika:** Sprites stand on the bottom edge. That is `y=bottom` (`y=1`), the default.

::move{mika y=0.4 time=0.5 ease=out}
**Mika:** Now I'm hovering. `y` is how far down the free room above and below me I am: `y=0` is the top of the screen, `y=1` the bottom.

::move{mika y=1.15 time=0.5 ease=inout}
**Mika:** And now I'm sinking below the floor. The screen cuts me off.

::move{mika y=1 time=0.4 ease=out}
::flash{time=0.5}
::show{ren happy x=0.42 scale=0.8 z=1 fade=0.4}
**Ren:** I'm standing in front of you, because my `z` is higher.

::show{mika happy z=2}
**Mika:** Not any more. My `z` is higher now; equal ones go by who arrived first.

::hide{ren fade=0.3}
::shake{time=0.7 strength=28 axis=y}
**Mika:** That was a shake along one axis: `axis=y` moves the stage up and down and never sideways. `axis=x` is the other way.

::show{ren happy x=0.72 scale=0.8 flip fade=0.3}
**Ren:** I'm mirrored with `flip`: same picture, facing the other way.

::move{ren xscale=1.5 yscale=0.75 time=0.6 ease=out}
**Ren:** And now I'm stretched: `xscale` and `yscale` change one direction only.

::move{ren xscale=1 yscale=1 flip=false time=0.4}
**Mika:** Back to normal. Now Ren walks a sequence.

<!-- `then` steps run one after another without holding up the story; `wait=` pauses before a step. `alpha` dims, `xoffset` shifts by pixels after `x` places. -->
::move{ren x=30% time=1 then x=72% time=1 wait=0.5 then alpha=0.3 time=0.5 then alpha=1 xoffset=-60 time=0.5}
**Ren:** One `::move` with `then`: over, a pause, back, a fade down to 0.3 and up again, and nudged 60 pixels left with `xoffset`. The story never waited for it.

::move{ren xoffset=0 time=0.4}
**Mika:** Now a scene change through black.

::fade{out time=0.8}
The screen is black, and this narration is still readable: the dialogue box is never covered.

::hide{all}
::bg{src=assets/night.png}
::fade{in time=0.8}
**Mika:** That was an explicit pair: `::fade` with `out`, then with `in`. The first waits until it is finished; a click ends it at once.

::bg{src=assets/rooftop.png fade=1.2 through=#000000}
::show{mika happy x=0.3 scale=0.8 fade=0.4}
**Mika:** And that was the short form: `through=` dips to a colour, changes the picture and comes back, all in one line.

<!-- A colour wash over the stage. It stays until `none`, saves with the game, and sits under the text. -->
::tint{color=#203060 alpha=0.45 fade=1}
**Mika:** And this is `::tint`: a night wash over the whole picture, eased in over a second. It stays until it is removed, and the text box is never tinted.

::tint{none fade=1}
**Mika:** Gone again. A tint only lays a colour over the picture: it does not drain it.

<!-- `::vignette` darkens the edges and corners and leaves the middle clear. It stays until `none`, and the text box is never darkened. -->
::vignette{strength=0.7 size=0.6 color=#400000 fade=1}
**Mika:** Whoa, the room is closing in. That is `::vignette`: dark edges, here a deep red, for a dizzy moment or an injury.

::vignette{none fade=1}
**Mika:** Better. The edges open up again.

<!-- `::filter` really changes the colours: gray, sepia, brightness, contrast, saturation. The textbox is never filtered. -->
::filter{gray=1 fade=1}
**Mika:** This is `::filter` with `gray=1`: the colour is truly gone, not covered, and the text box kept its colours. A tint would sit on top of it.

::filter{none fade=1}
**Mika:** And back, easing out over a second.

<!-- `blur=` softens a picture; the radius is in pixels of the stage (0 to 64). A `::bg` is sharp unless it says `blur=`. -->
::bg{src=assets/rooftop.png blur=16 fade=1}
**Mika:** The background is blurred with `blur=16`, over a one second fade. It stays like that until the next `::bg`.

::move{mika blur=12 time=0.6 then blur=0 time=0.6}
**Mika:** And I just went out of focus and back with `blur=12` in a `::move`, and `blur=0` in the `then` step after it.

<!-- `tint=` multiplies one sprite's colours: `#ffffff` is unchanged, `#000000` a silhouette, `none` takes it off. -->
::move{mika tint=#6070a0 time=0.6}
**Mika:** Now only I have a night-blue tint, with `tint=#6070a0` in a `::move`. The background is untouched.

::move{mika tint=none time=0.6}
**Mika:** And `tint=none` brings my colours back.

<!-- `blend=add` adds a picture to what is under it: a lamp glow drawn on transparent, bright in the middle. -->
::show{src=assets/lamp_glow.png id=glow x=75% y=45% scale=1.2 blend=add alpha=0.9 z=5 fade=0.5}
::move{glow alpha=0.5 time=0.3 ease=inout loop}
**Mika:** That warm glow is a light overlay: `blend=add` adds the picture to the scene, and a looping `::move` of its `alpha` makes it flicker.

::hide{glow fade=0.5}

<!-- `mask=` changes the background following a greyscale picture: dark parts show the new picture first, white parts last. -->
::bg{src=assets/rooftop.png fade=2 mask=assets/masks/clock.png}
**Mika:** That was a clock wipe: `mask=` made the new background appear where a greyscale picture is dark first. A different mask gives ink spreading or any shape you draw.

::sfx{src=assets/calm.ogg loop volume=0.4 fade=1}
**Mika:** Hear that quiet pad? It is a looping `::sfx`: it plays on by itself, line after line, and keeps playing across rollback.

::sfx{stop=assets/calm.ogg fade=1.5}
**Mika:** And an `::sfx` with `stop=` and the file name fades it out again.

<!-- A CG moment: the dialogue box is hidden for a few seconds (a click goes on). The next line shows it again by itself. -->
::textbox{hide fade=0.4}
::wait{time=3}

**Mika:** A paragraph break starts a new line inside one line of dialogue.{p}It never waits, and it works with {color=#ffd98c}colour{/color} and {size=+14}size{/size} too.

**Mika:** Words can be {u}underlined{/u}, and a word can carry a small reading above it, like {ruby=かんじ}漢字{/ruby}.

**Mika:** Text can change size too: {size=+14}bigger{/size}, {size=24}smaller{/size}, and {size=+14}big {color=#ff8080}and red{/color}{/size} together.

**Mika:** This line moves on by itself once it is typed, with no click...{nw}

::choice
- [Replay the staging](#stage)
- [That's all](#done)
::

# done

**Mika:** That's all.

**Mika:** Well, one more thing: a timed choice. Answer before the bar runs out, or stay quiet and the default answer is picked.
::choice{time=8}
- [Wave goodbye](#wave)
- [Say nothing](#silent){default}
::

# wave
**Mika:** Goodbye!
::jump{to=bye}

# silent
**Mika:** ...Goodbye, then.

# bye
::end
