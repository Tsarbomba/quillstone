---
title: The Night Market
theme: market_theme.txt
inventory: pack
vars: { gold: 40, pack: [], polite: false }
---

<!-- Items, an inventory screen and a shop. `::item` declares what can be held;
     `inventory:` (above) names the list variable the Items button and the I key
     show. The `::shop` block sells items for gold, and `::inventory` opens the
     same screen from the script. Both screens are styled by market_theme.txt. -->

::item{id=key name="Brass key" icon=assets/gui/key.png price=15 text="Opens the gate of the clock tower."}
::item{id=lamp name="Oil lamp" icon=assets/gui/lamp.png price=12 text="Burns for hours. Good for dark tunnels."}
::item{id=map name="Town map" icon=assets/gui/map.png price=8 text="Every street, and one that is on no other map."}
::item{id=charm name="Lucky charm" icon=assets/gui/charm.png price=20 text="She only sells it to people who were polite."}

# start

::bg{src=assets/night.png}
The night market opens at midnight. You have {gold} gold and a feeling that something at the clock tower is waiting for you.
Press I at any time to look in your pack.

::choice
- [Greet the stallholder](#greet){polite=true}
- [Go straight to the stalls](#stalls)
::

# greet

**Stallholder:** Good evening! Polite customers get my best prices.
::jump{to=stalls}

# stalls

::shop{pay=gold into=pack}
- key
- lamp
- map
- charm{price=18, if polite}
::

You have {gold} gold left.
You check your pack before the walk to the tower.

::inventory

# tower

::bg{src=assets/rooftop.png fade=0.8}
::if{pack has "key"}
The brass key turns in the gate with a soft click.
::else
The gate of the clock tower is locked. Perhaps one of the stalls sold something for it.
::endif

::if{pack has "lamp"}
Your lamp lights the stairs all the way up.
::endif

::if{pack has "charm"}
The charm is warm in your pocket. Tonight, nothing can go wrong.
::endif

::choice
- [Climb the tower](#top){if pack has "key"}
- [Walk back to the market](#stalls)
- [Go home](#home)
::

# top

::bg{src=assets/night.png fade=1}
From the top you see the whole town, every light of the market, and the river like a line of ink. It was worth the gold.
::end

# home

::bg{src=assets/night.png fade=1}
You go home with your purchases, and the market closes behind you.
::end
