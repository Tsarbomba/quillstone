---
title: Shop Demo
vars: { gold: 30, inventory: [] }
---

<!-- A list variable is a set of items: `inventory += "key"` adds one (twice does
     nothing), `inventory -= "key"` removes it, `inventory has "key"` asks, and
     `{inventory}` shows the items in the order they were added.
     `# price_line(item, cost)` is a label with parameters: `::call{to=price_line
     item="lamp" cost=10}` sets `item` and `cost` (ordinary variables) and comes
     back at the `::return`. -->

# start

You push open the door of a cramped little shop. The keeper nods without looking up.

Two things are priced on the shelf behind the counter.
::call{to=price_line item="brass key" cost=15}
::call{to=price_line item="lamp" cost=10}

# shop

You have {gold} gold.

::if{inventory}
Your bag holds: {inventory}.
::else
Your bag is empty.
::endif

::choice
- [Buy the brass key (15 gold)](#bought){gold -= 15, inventory += "key", if gold >= 15 and not inventory has "key"}
- [Buy the lamp (10 gold)](#bought){gold -= 10, inventory += "lamp", if gold >= 10 and not inventory has "lamp"}
- [Buy the old map (10 gold)](#bought){gold -= 10, inventory += "map", if gold >= 10 and not inventory has "map"}
- [Sell the map back (5 gold)](#sold){gold += 5, inventory -= "map", if inventory has "map"}
- [Leave the shop](#leave)
::

# bought

"A fine choice," says the keeper.
::jump{to=shop}

# sold

The keeper counts out five coins and tucks the map under the counter.
::jump{to=shop}

# leave

::if{inventory has "key"}
Outside, the key turns the lock of the old gate with a click. Whatever is behind it is yours now.
::else
Outside, the old gate is locked tight. Perhaps the shop sold something for it.
::endif

::if{inventory has "lamp" and inventory has "map"}
With the lamp lit and the map in your hand, you will not get lost in the tunnels.
::endif
::end

# price_line(item, cost)

The {item} costs {cost} gold.
::return
