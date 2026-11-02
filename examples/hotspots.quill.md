---
title: Hotspots Demo
theme: theme.txt
vars: { crossed: false }
persistent: { found_scarf: false }
gallery: [assets/scarf.png]
---

<!-- Clickable regions over the background: a map, a collectible and an exit.
     Regions are x,y,w,h of the screen (0 to 1, or percentages). Without `image=` a
     region is invisible until the mouse (or the arrow keys) reach it. `hover=` is the picture
     shown while a region is hovered; `ground=`/`hover=` on the block make an imagemap (the hovered
     part of one full-screen picture is shown); `call` comes back to this screen afterwards. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

# start

::bg{src=assets/night.png}
You stand in a small town at night. Everything is within walking distance.

# map

<!-- No line here: the box with the last line is hidden, so the map just shows and the
     regions are the only way on. Any line (or choice) shows the box again by itself. -->
::bg{src=assets/night.png}
::textbox{hide}
<!-- A real game ships a lit-up copy of the ground as `hover=`; here the other room's picture stands in for it. -->
::hotspots{ground=assets/night.png hover=assets/rooftop.png}
- [The rooftop](#rooftop){rect=14%,52%,8%,14% image=assets/gui/pin.png hover=assets/gui/pin_hover.png, call}
- [The old bridge](#bridge){rect=55%,64%,16%,12%}
- [The clock tower](#tower){rect=38%,18%,8%,14% image=assets/gui/pin.png hover=assets/gui/pin_hover.png, once}
- [A scarf in the grass](#scarf){rect=84%,40%,5%,9%, hidden, found_scarf=true, if not found_scarf}
- [Leave town](#leave){rect=88%,88%,10%,8%}
::

# rooftop

::bg{src=assets/rooftop.png fade=0.5}
::show{mika neutral x=80% fade=0.3}
**Mika:** You found my rooftop. Come back any time.
::hide{all fade=0.3}
<!-- A `call` region comes back to the map when this label returns: the box hides by itself, and the
     map's `ground=` covers the rooftop picture. -->
::return

# bridge

::bg{src=assets/rooftop.png fade=0.5}
The old bridge creaks under your feet. The river below is black and quiet.
::set{crossed=true}
::jump{to=map}

# tower

<!-- The scarf region is `hidden`: no outline or tooltip, and the arrow keys skip it. -->
<!-- `once` hides the region after it was picked, in this playthrough. -->
The clock tower is locked, but the clock says it is almost midnight.
::jump{to=map}

# scarf

![A scarf, left behind](assets/scarf.png)
It is Mika's scarf. You take it with you; it is in the gallery now.
::jump{to=map}

# leave

::bg{src=assets/night.png fade=1}
::if{crossed}
You cross the bridge one last time and leave town, a little lighter.
::else
You leave town without looking back.
::endif
::end
