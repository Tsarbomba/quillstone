---
title: Staging
---

<!-- Vertical placement (`y=`), drawing order (`z=`) and the `{size}` and `{nw}`
     text tags. See SCRIPT.md. Ren is Mika's picture with red and blue swapped, so overlaps are easy to tell apart. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}
::character{id=ren name=Ren color=#9ad1ff sprites=assets/ren_{pose}.png}

# stage

::bg{src=assets/rooftop.png fade=0.5}
::show{mika neutral at=0.3 scale=0.8 fade=0.4}
**Mika:** Sprites stand on the bottom edge. That is `y=1`, the default.

::move{mika y=0.82 time=0.5 ease=out}
**Mika:** Now I'm hovering. `y` is where my feet are, as a fraction of the screen height.

::move{mika y=1.12 time=0.5 ease=inout}
**Mika:** And now I'm sinking below the floor. The screen cuts me off.

::move{mika y=1 time=0.4 ease=out}
::show{ren happy at=0.42 scale=0.8 z=1 fade=0.4}
**Ren:** I'm standing in front of you, because my `z` is higher.

::show{mika happy z=2}
**Mika:** Not any more. My `z` is higher now; equal ones go by who arrived first.

::hide{ren fade=0.3}
**Mika:** Text can change size too: {size=+14}bigger{/size}, {size=24}smaller{/size}, and {size=+14}big {color=#ff8080}and red{/color}{/size} together.

**Mika:** This line moves on by itself once it is typed, with no click...{nw}

::choice
- [Replay the staging](#stage)
- [That's all](#done)
::

# done

**Mika:** That's all.
::end
