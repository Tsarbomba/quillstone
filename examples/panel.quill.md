---
title: Status panel
theme: theme.txt
vars: { who: "mika", name.mika: "Mika", name.ren: "Ren", name.jo: "Jo", l1.mika: "Met on the roof.", l2.mika: "Likes tea.", l3.mika: "", l1.ren: "Runs the news stand and knows every rumour in town before it is printed.", l2.ren: "", l3.ren: "", l1.jo: "Nothing known yet.", l2.jo: "", l3.jo: "" }
---

<!-- A panel of your own from existing pieces: a HUD button calls the label `status`, which draws a
     backdrop `box=`, texts that read `{name.{who}}`, and a `::hotspots` row of tabs and Close. A
     line whose text is empty draws nothing, and `width=` wraps a long one. Closing hides every `st_` element and `::return`s.
     See "Custom panels" in SCRIPT.md. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}
::character{id=ren name=Ren color=#9ad1ff sprites=assets/ren_{pose}.png}

# start

::bg{src=assets/rooftop.png}
::show{mika neutral x=0.3 scale=0.8}
::show{ren neutral x=0.72 scale=0.8}
::hud{id=status_btn text="Status" x=0.98 y=0.03 back=#00000099 pad=10 call=#status}
**Mika:** See the Status button at the top right? It opens a panel I made from a HUD button, a few texts and some hotspots.
**Ren:** Press it, then pick a name on the tabs. Jo has nothing to show yet, so the panel only shows the lines that are not empty.
**Mika:** Close it, and we carry on with the line we were on.
**Ren:** The panel hides every one of its texts at once with a prefix hide, and then it just returns.
::end

# status

::hud{id=st_back box=#000000cc size=900,700 x=center y=center}
::hud{id=st_name text="{name.{who}}" size=56 x=center y=28%}
::hud{id=st_1 text="{l1.{who}}" width=700 x=center y=40%}
::hud{id=st_2 text="{l2.{who}}" x=center y=46%}
::hud{id=st_3 text="{l3.{who}}" x=center y=52%}
::textbox{hide}
::hotspots
- [Mika](#status){rect=30%,66%,10%,7% text="Mika", who="mika"}
- [Ren](#status){rect=45%,66%,10%,7% text="Ren", who="ren"}
- [Jo](#status){rect=60%,66%,10%,7% text="Jo", who="jo"}
- [Close](#status-close){rect=45%,74%,10%,7% text="Close"}
::

# status-close

::hud{hide=st_*}
::return
