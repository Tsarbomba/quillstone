---
title: Quillstone Demo
theme: theme.txt
vars: { trust: 0, met: false, place: "the rooftop" }
persistent: { finished: false }
gallery: [assets/rooftop.png, assets/night.png]
music_room: [assets/calm.ogg]
---

<!-- Headings are labels. Prose is narration. **Name:** starts dialogue.
     Everything below is in one file; see SCRIPT.md for the full reference. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

# rooftop

::bg{src=assets/rooftop.png fade=0.5}
::music{src=assets/calm.ogg fade=1 volume=0.8}
::show{mika neutral at=right fade=0.5}
<!-- A slow push-in while she talks; it doesn't wait. -->
::camera{zoom=1.15 x=0.6 y=0.4 time=6}

<!-- `finished` is persistent: set at the ending, it is still true in the next playthrough. -->
::if{finished}
It feels like you have stood here before.
::endif
**Mika:** You came *after all*.

![Her scarf, close up](assets/scarf.png)
The wind pulls at her scarf. She doesn't look at you.

::call{to=remember}

**Mika:** Remember that day on {place}?
Her voice is quieter now.

# answer
<!-- `once`: offered until picked. Picking it comes back here without it. -->
::choice
- [Tell the truth](#truth){trust+=1}
- [Lie](#lie){trust-=1}
- [Tell her everything](#everything){trust+=2, if met}
- [Look at the city first](#city){once}
::

# city
The city hums below, a thousand windows still awake.
::jump{to=answer}

# remember
<!-- A subroutine: runs, then ::return comes back to the line after ::call. -->

::video{src=assets/flashback.webm fit=cover skippable}
::set{met=true}
::camera{reset time=1}
**Mika:** That was the day {color=#ffb070}everything{/color} changed.
::return

# truth
::show{mika happy}
**Mika:** Thank you.
::jump{to=ending}

# lie
::shake{time=0.4 strength=16}
<!-- Text tags: **bold**, *italic*, {color=#rrggbb}...{/color}, and {w=0.6} pauses the typing. -->
**Mika:** ...{w=0.6}I see.
::jump{to=ending}

# everything
::show{mika happy}
::move{mika at=center scale=1.1 time=0.8 ease=out}
**Mika:** You remembered all of it?
::jump{to=ending}

# ending
::bg{src=assets/night.png fade=1.5 wipe=right}
::wait{time=0.6}
::if{trust > 1}
**Mika:** I'm glad it was you. I really am.
::elif{trust > 0}
**Mika:** Thank you for being honest.
::else
**Mika:** I guess I expected that.
::endif
<!-- seen(label): how often the story passed that label. -->
::if{seen(city)}
Below, the lights you watched together are still burning.
::endif
::hide{mika fade=0.5}
::set{finished=true}
::end
