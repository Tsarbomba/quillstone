---
title: Staging
---

<!-- Vertical placement (`y=`), drawing order (`z=`), the `{size}`, `{p}` and `{nw}`
     text tags, `::flash`, a looping `::sfx` that is started and later stopped, a shake along
     one axis, a flipped and a stretched sprite, a dip to black (`::fade` and `through=`) and a timed choice.
     See SCRIPT.md. Ren is Mika's picture with red and blue swapped, so overlaps are easy to tell apart. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}
::character{id=ren name=Ren color=#9ad1ff sprites=assets/ren_{pose}.png}

# stage

::bg{src=assets/rooftop.png fade=0.5}
::sfx{src=assets/calm.ogg loop volume=0.4 fade=1}
::show{mika neutral at=0.3 scale=0.8 fade=0.4}
**Mika:** Sprites stand on the bottom edge. That is `y=1`, the default.

::move{mika y=0.82 time=0.5 ease=out}
**Mika:** Now I'm hovering. `y` is where my feet are, as a fraction of the screen height.

::move{mika y=1.12 time=0.5 ease=inout}
**Mika:** And now I'm sinking below the floor. The screen cuts me off.

::move{mika y=1 time=0.4 ease=out}
::flash{time=0.5}
::show{ren happy at=0.42 scale=0.8 z=1 fade=0.4}
**Ren:** I'm standing in front of you, because my `z` is higher.

::show{mika happy z=2}
**Mika:** Not any more. My `z` is higher now; equal ones go by who arrived first.

::hide{ren fade=0.3}
::shake{time=0.7 strength=28 axis=y}
**Mika:** That was a shake along one axis: `axis=y` moves the stage up and down and never sideways. `axis=x` is the other way.

::show{ren happy at=0.72 scale=0.8 flip fade=0.3}
**Ren:** I'm mirrored with `flip`: same picture, facing the other way.

::move{ren xscale=1.5 yscale=0.75 time=0.6 ease=out}
**Ren:** And now I'm stretched: `xscale` and `yscale` change one direction only.

::move{ren xscale=1 yscale=1 flip=false time=0.4}
**Mika:** Back to normal. Now a scene change through black.

::fade{out time=0.8}
The screen is black, and this narration is still readable: the dialogue box is never covered.

::hide{all}
::bg{src=assets/night.png}
::fade{in time=0.8}
**Mika:** That was an explicit pair: `::fade` with `out`, then with `in`. The first waits until it is finished; a click ends it at once.

::bg{src=assets/rooftop.png fade=1.2 through=#000000}
::show{mika happy at=0.3 scale=0.8 fade=0.4}
**Mika:** And that was the short form: `through=` dips to a colour, changes the picture and comes back, all in one line.

**Mika:** The quiet pad under us is a looping `::sfx`. It keeps playing across rollback, and it will stop at the end of this scene.

<!-- A CG moment: the dialogue box is hidden for a few seconds (a click goes on). The next line shows it again by itself. -->
::textbox{hide fade=0.4}
::wait{time=3}

**Mika:** A paragraph break starts a new line inside one line of dialogue.{p}It never waits, and it works with {color=#ffd98c}colour{/color} and {size=+14}size{/size} too.

**Mika:** Text can change size too: {size=+14}bigger{/size}, {size=24}smaller{/size}, and {size=+14}big {color=#ff8080}and red{/color}{/size} together.

**Mika:** This line moves on by itself once it is typed, with no click...{nw}

::sfx{stop=assets/calm.ogg fade=1.5}
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
