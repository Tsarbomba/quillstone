---
title: Catch the Lamp
vars: { pins: 0, slips: 0, hit: "" }
theme: theme.txt
sprite_size: pixels
art_scale: 1.5
---

<!-- A real-time screen is one step of the story: the player clicks, or time runs out, and the story
     goes on from there. Here a lamp bobs across the night sky (a looping `::move`); the hotspot is
     bound to the sprite with `sprite=lamp`, so it is wherever the lamp is drawn. `time=5` with
     `default=#too-slow` is the timeout: after 5 seconds the story jumps there.
     The lock at the end uses `::timing`: a marker sweeps back and forth over a bar and the player
     stops it. Where it stops ("perfect", "good" or "miss") is stored in `hit`, and the story goes on
     with the next line like after any other step, so `::if` can branch on it. -->

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
You catch the lamp. It is warm, and its light falls on the rusty lock of the old gate.
::hud{id=pinbar bar=pins max=3 x=0.5 y=0.06 size=320,26 color=#ffd98c back=#333333 border=#ffffff}
::hud{id=pintext text="Pins set: {pins} / 3" x=0.5 y=0.12 size=32 color=#ffd98c}

# pick

::timing{result=hit speed=1.2 good=35%..65% perfect=47%..53% time=6 prompt="Pin {pins} of 3: stop the needle!"}
::if{hit == "perfect"}
::set{pins += 1}
The pin clicks home, clean.
::elif{hit == "good"}
::set{pins += 1}
The pin catches, just.
::else
::set{slips += 1}
The pick slips.
::endif
::if{pins >= 3}
::jump{to=open}
::endif
::if{slips >= 2}
::jump{to=jammed}
::endif
::jump{to=pick}

# open

The lock springs open. The gate swings wide.
::end

# jammed

The pick snaps. The lock stays shut.
::end

# bridge

Only the bridge, and the lamp is still bobbing past.
::jump{to=chase}

# too-slow

::hide{lamp fade=0.3}
The lamp drifts out of reach. Too slow.
::end
