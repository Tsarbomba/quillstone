---
title: Layers
vars: { look: "casual", feeling: "smile" }
---

<!-- Layered sprites: one character, several stacked pictures (body, eyes, mouth) that change
     one at a time. The pictures of every layer live in assets/layers/ and are all the same
     size. Blinking is an ordinary frame animation (`::anim`) used as the eyes layer.
     See "Layered sprites" in SCRIPT.md. -->

::anim{id=blink frames="assets/layers/eyes_open.png 3, assets/layers/eyes_closed.png 0.12" loop}

::character{id=mika name=Mika color=#ffd98c layers=body,eyes,mouth layer_sprites=assets/layers/{layer}_{value}.png}

# start

::bg{src=assets/rooftop.png fade=0.5}
::show{mika body=casual eyes=open mouth=smile x=0.3 scale=0.9 fade=0.4}
**Mika:** I am drawn from three layers: a body, eyes and a mouth. The first time I appear, I need a value for every one.

::show{mika mouth=open}
**Mika:** Now only my mouth changed. A layer you leave out keeps what it had, and only the layer that changed crossfades.

::show{mika eyes=happy fade=0.4}
**Mika:** Eyes too, with a fade of their own.

::show{mika eyes=anim:blink mouth=smile}
**Mika:** Now my eyes are an animation: they stay open for three seconds, close for a blink, and go round again.

::show{mika body=formal}
**Mika:** A new outfit does not touch my eyes, so I keep blinking.

::move{mika x=0.7 time=0.8 ease=inout}
**Mika:** Moves, scale, flips and fades work on the whole of me, all the layers at once.

::show{mika eyes=none mouth=none}
**Mika:** A layer can be turned off with `none`. This is me with no face.

::show{mika body={look} eyes=open mouth={feeling}}
**Mika:** Layer values can come from variables too: `look` is "{look}" and `feeling` is "{feeling}".

# pick

::choice
- [Casual, smiling](#again){look="casual", feeling="smile"}
- [Formal, mouth open](#again){look="formal", feeling="open"}
- [That is enough](#end)
::

# again

::show{mika body={look} mouth={feeling}}
**Mika:** Changed. Pick again?
::jump{pick}

# end

::hide{mika fade=0.4}
**Mika:** Thanks for watching.
