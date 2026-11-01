---
title: Catch the Lamp
theme: theme.txt
sprite_size: pixels
art_scale: 1.5
---

<!-- A real-time screen is one step of the story: the player clicks, or time runs out, and the story
     goes on from there. Here a lamp bobs across the night sky (a looping `::move`); the hotspot is
     bound to the sprite with `sprite=lamp`, so it is wherever the lamp is drawn. `time=5` with
     `default=#too-slow` is the timeout: after 5 seconds the story jumps there. -->

# start

::bg{src=assets/night.png}
A lamp drifts over the town. Catch it before it gets away.
::show{id=lamp src=assets/gui/lamp.png x=10% y=40%}
::move{lamp x=90% time=2 ease=inout loop}

# chase

::hotspots{time=5 default=#too-slow}
- [The lamp](#caught){sprite=lamp}
- [The old bridge](#bridge){rect=55%,64%,16%,12%, once}
::

# caught

::hide{lamp fade=0.3}
You catch the lamp. It is warm.
::end

# bridge

Only the bridge, and the lamp is still bobbing past.
::jump{to=chase}

# too-slow

::hide{lamp fade=0.3}
The lamp drifts out of reach. Too slow.
::end
