---
title: HUD Demo
vars: { hero.hp: 10, hero.max_hp: 10, hero.gold: 5 }
---

<!-- `::hud` puts always-on elements over the story: text with {variables}, bars
     that follow a variable, and pictures. An element keeps its `id`; using the
     same id again replaces it. They update by themselves when variables change. -->

# start

::bg{color=#000000}
::hud{id=title text="{size=96}Chapter 1{/size}" x=0.5 y=0.38 color=#ffffff fade=1}
::wait{time=2}
::hud{id=subtitle text="{size=40}The Quiet Harbour{/size}" x=0.5 y=0.52 color=#ffd98c fade=1}
::wait{time=2}
::hud{hide=title fade=1}
::hud{hide=subtitle fade=1}
::wait{time=1.2}
::bg{src=assets/rooftop.png fade=1}

::hud{id=hp bar=hero.hp max=hero.max_hp x=0.02 y=0.04 size=320,26 color=#cc3333 back=#333333 border=#ffffff fade=0.5}
::hud{id=gold text="Gold: {hero.gold}" x=0.98 y=0.04 size=32 color=#ffd98c back=#00000099 pad=12}
::hud{id=low text="LOW HP" x=0.02 y=0.11 size=28 color=#ff5555 if="hero.hp <= 3"}
::hud{id=key image=assets/gui/key.png x=0.98 y=0.14 scale=0.5}

You stand on the roof. The bar at the top left is your health, the text on the right is your gold.

# camp

You have {hero.hp} of {hero.max_hp} hit points and {hero.gold} gold.

::choice
- [Rest and heal](#rest){hero.hp = min(hero.hp + 3, hero.max_hp), hero.gold -= 1, if hero.gold > 0}
- [Fight a rooftop pigeon](#fight){if hero.hp > 0}
- [Find a coin](#coin){once}
- [Leave](#leave)
::

# rest

You sit in the sun. The bar fills, and the coin is gone.
::jump{to=camp}

# fight

The pigeon wins. Ouch.
::set{hero.hp = max(hero.hp - 4, 0)}
::if{hero.hp == 0}
You are out of hit points. The warning shows below the bar while you are at 3 or less.
::set{hero.hp = 1}
::endif
::jump{to=camp}

# coin

Something glints between the tiles.
::set{hero.gold += 3}
::jump{to=camp}

# leave

::hud{id=gold text="Gold: {hero.gold} (final)" x=0.98 y=0.04 size=32 color=#ffffff back=#00000099 pad=12}
The gold element was created again with the same id: it was replaced where it stood.
::hud{clear fade=0.5}
All the elements are gone.
