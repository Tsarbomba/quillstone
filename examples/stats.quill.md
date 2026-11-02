---
title: Stats Demo
vars: { hero.hp: 12, hero.max_hp: 12, hero.gold: 20, base: 8, discount: 2, cost: 0, dmg: 0, roll: 0, gap: 0, items: 0, inventory: [], companion: "mia", bond.mia: 0, bond.zoe: 0 }
---

<!-- Variables may have dotted names (`hero.hp`): they are ordinary variables, the
     dot just groups them. Expressions can calculate: `+ - * / %`, parentheses,
     `min`, `max`, `abs`, and `count(list)`. `random(lo, hi)` picks a whole number
     and may only be assigned to a variable first (`dmg = random(2, 5)`), never
     used in a condition. Text always shows a variable (`{dmg}`), never a sum, so
     the results are stored first. A `{}` in a name looks a variable up by another
     one: `bond.{companion}` is `bond.mia` while `companion` holds "mia". -->

# start

You reach the mouth of a cave with {hero.hp} hit points and {hero.gold} gold.

# camp

You sit at the camp: {hero.hp} of {hero.max_hp} hit points, {hero.gold} gold.

::choice
- [Rest by the fire](#rest){hero.hp = min(hero.hp + 4, hero.max_hp), if hero.hp < hero.max_hp}
- [Go into the cave](#cave){once}
- [Pick the old lock](#lock){once}
- [Visit the trader](#trader)
- [Head home](#home)
::

# rest

The fire warms you. You are at {hero.hp} of {hero.max_hp} now.
::set{bond.{companion} = 1}
::jump{to=camp}

# cave

A bat swoops out of the dark and bites you.
::set{dmg = random(2, 5)}
::set{hero.hp = max(hero.hp - dmg, 1)}
It takes {dmg} hit points. You have {hero.hp} left.

::if{hero.hp <= hero.max_hp * 2 / 3}
You are hurt and should rest.
::endif

Behind the bat's nest you find a little purse.
::set{hero.gold += 5}
::jump{to=camp}

# lock

A rusty lock guards a small chest. You feel for the pins and roll the dice for luck.
::set{roll = random(1, 6)}
::set{gap = abs(roll - 4)}
You rolled {roll}, which is {gap} away from the sweet spot.

::if{roll >= 5}
The lock clicks open. Inside lie ten coins.
::set{hero.gold += 10}
::elif{roll >= 3}
The pick slips and the lock stays shut.
::else
The pick snaps and nicks your hand. You lose a hit point.
::set{hero.hp = max(hero.hp - 1, 1)}
::endif
::jump{to=camp}

# trader

A trader waves you over.
::set{cost = base * 2 - discount}
::set{items = count(inventory)}

"The lantern is {cost} gold, the rope {base}." You carry {items} things.

::choice
- [Buy the lantern](#bought){hero.gold -= cost, inventory += "lantern", if hero.gold >= cost and not inventory has "lantern"}
- [Buy the rope](#bought){hero.gold -= base, inventory += "rope", if hero.gold >= base and not inventory has "rope"}
- [Back to camp](#camp)
::

# bought

"A fine choice," says the trader.
::jump{to=trader}

# home

::if{bond.{companion} > 0}
{companion} stays close after the night at the fire.
::endif

::if{count(inventory) >= 2}
With a lantern and a rope you are ready for the deep caves, and you have {hero.gold} gold to spare.
::elif{count(inventory) == 1}
You are half ready for the deep caves. You have {hero.gold} gold.
::else
You go home with {hero.gold} gold and nothing to show for it.
::endif
::end
