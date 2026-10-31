---
title: Animation
---

<!-- Frame animations (`::anim`, shown with `anim=`) and looping moves (`::move{... loop}`).
     See SCRIPT.md. The pictures are the ordinary demo art: Mika's two poses make the
     "talking" mouth, rooftop and night make a flickering background. -->

::anim{id=talk frames="assets/mika_neutral.png 0.2, assets/mika_happy.png 0.2" loop}
::anim{id=arrive frames="assets/ren_neutral.png 0.6, assets/ren_happy.png 0.1"}
::anim{id=flicker frames="assets/rooftop.png 1.5, assets/night.png 1.5" loop}

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}
::character{id=ren name=Ren color=#9ad1ff sprites=assets/ren_{pose}.png}

# start

::bg{src=assets/rooftop.png fade=0.5}
::show{mika neutral x=0.3 scale=0.8 fade=0.4}
**Mika:** A frame animation is a list of pictures and how long each one stays up. Declare it once with `::anim`, then use it like a picture.

::show{mika anim=talk}
**Mika:** This is "talking": my two poses swap every fifth of a second, for as long as I am on stage.

::show{ren anim=arrive x=0.72 scale=0.8 fade=0.3}
**Ren:** I'm a one-shot animation. I waited a moment, then I changed my pose, and now I just stay like this.

::show{mika neutral}
**Mika:** A pose goes back to a still picture whenever you `::show` one. The animation clock starts again each time an animation appears.

::bg{anim=flicker fade=1}
**Mika:** The background can be an animation too. It fades in like any other picture, and keeps flickering between day and night.

::hide{ren fade=0.3}
::bg{src=assets/rooftop.png fade=0.8}
::move{mika y=0.85 time=0.8 ease=inout loop}
**Mika:** A looping move: I go to the target, back to where I started, and forth again, until something else moves me. That is a bob.

::move{mika scale=0.84 time=1.5 ease=inout loop}
**Mika:** Moves with `loop` can animate anything a move can. This one makes me breathe, by size alone.

::move{mika y=0.85 scale=0.84 time=0.8 ease=inout loop}
**Mika:** Both together. Open Settings: I freeze while a menu is up, like every timer.

::move{mika x=0.5 scale=0.8 y=1 time=0.6 ease=out}
**Mika:** A move without `loop` ends the loop and glides me back to where the new values are. That is all.

# the end

**Mika:** Thanks for watching.
