---
title: Free Roam
theme: theme.txt
vars: { day: 1, time: "morning", square: false, talked_ren: false }
---

<!-- A time-of-day sandbox built from plain features: `day` and `time` are variables, the map is a
     `::hotspots` screen of `call` regions, `pass-time` moves the clock on, and each place tries its
     events in order with `seen()` doing the remembering. See "Free roam" in SCRIPT.md. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}
::character{id=ren name=Ren color=#8cc8ff sprites=assets/ren_{pose}.png}

# start

::bg{src=assets/night.png}
::hud{id=clock text="Day {day}, {time}" x=0.98 y=0.04 size=36 color=#ffd98c back=#00000099 pad=12}
Welcome to a small free-roam town. The clock in the corner is a HUD text that shows two variables, the day and the time. When the story changes them, it changes too.
You have two days, with three parts each: morning, afternoon and evening.
Pick a place on the map. Every visit costs one part of the day.

# map

<!-- Each place is a `call` region: its label runs, then `::return` comes back here and the
     regions are worked out again, so the `if` of a place is checked on every return. -->
::bg{src=assets/night.png}
::textbox{hide}
::hotspots
- [The rooftop](#rooftop){rect=14%,52%,8%,14% image=assets/gui/pin.png hover=assets/gui/pin_hover.png, call}
- [The square](#square){rect=70%,58%,8%,14% image=assets/gui/pin.png hover=assets/gui/pin_hover.png, call, if square and time != "morning"}
- [Wait a while](#pass-time){rect=40%,90%,20%,7% text="Wait a while", call}
::

# rooftop

::bg{src=assets/rooftop.png fade=0.5}
::show{mika neutral x=80% fade=0.3}
<!-- The first event whose condition holds runs, so the rarest one goes first. -->
::if{day == 2 and time == "evening" and not seen(mika-secret)}
::call{to=mika-secret}
::elif{not seen(mika-intro)}
::call{to=mika-intro}
::else
**Mika:** Nothing new up here. Try the square, or wait for the evening of the second day.
::endif
::hide{all fade=0.3}
::call{to=pass-time}
::return

# mika-intro

**Mika:** You're new here! This talk is an event: the rooftop calls it as long as the story has never passed it.
**Mika:** The story counts how often the story has passed a label, so I never needed a flag of my own to remember we met. Visit again and you'll get the plain chat instead.
::set{square=true}
**Mika:** I put the square on your map: its region only shows once I've told you, and only after the morning.
::hud{id=map text="Map" x=0.02 y=0.04 size=36 color=#ffd98c back=#00000099 pad=12 jump=#map}
**Mika:** And the Map button in the corner is a HUD button that jumps: it goes straight to the map from whatever line you are on and doesn't come back, so it costs no part of the day.
::return

# mika-secret

<!-- This event has a time window, and `seen()` makes it happen once. -->
**Mika:** Day two, in the evening: this event waits for exactly that, and it only runs once.
::hud{off fade=0.5}
**Mika:** Look, the clock is gone. Turning the HUD off hides every element without losing them, which suits a quiet scene.
**Mika:** Here is the secret: I come up here for the view, not the people.
::hud{on fade=0.5}
**Mika:** And turning it on brings the clock back exactly as it was.
::return

# square

::bg{src=assets/night.png fade=0.5}
::show{ren neutral x=20% fade=0.3}
::if{not seen(ren-hello)}
::call{to=ren-hello}
::elif{not talked_ren}
::call{to=ren-news}
::else
**Ren:** I told you today's news already. Come back tomorrow.
::endif
::hide{all fade=0.3}
::call{to=pass-time}
::return

# ren-hello

**Ren:** First time in the square? That makes this an event that happens once, like Mika's.
::return

# ren-news

**Ren:** I have news once a day. This chat is guarded by a daily variable.
::set{talked_ren=true}
**Ren:** The label that moves the clock on resets it when the day rolls over, so tomorrow I will talk again.
::return

# pass-time

<!-- The one place where time moves: every visit and every "Wait" ends up here. -->
::if{time == "morning"}
::set{time="afternoon"}
::elif{time == "afternoon"}
::set{time="evening"}
::else
::set{time="morning", day += 1, talked_ren=false}
::endif
::if{day > 2}
::jump{to=finale}
::endif
::return

# finale

::hud{hide=clock}
::hud{hide=map}
::bg{src=assets/night.png}
::if{seen(mika-secret)}
The two days are over. You heard Mika's secret, and the story's count of seen labels is how this ending knew.
::else
The two days are over. You never heard Mika's secret, and the story's count of seen labels is how this ending knew.
::endif
::end
