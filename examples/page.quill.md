---
title: Pages
---

<!-- Full-screen pages: `::page` makes the lines after it stack up on a page instead of replacing each
     other in the textbox, until `::page{off}`. See "Full-screen pages" in SCRIPT.md. -->

::character{id=mika name=Mika color=#ffd98c}

# start

**Mika:** I found a letter in the old desk. I will read it out.

::page

Dear Mika,

The lamp on the roof still works. I light it every evening, in case you come back this way.

The winter was long, but the snow is gone now and the stairs are safe again.

**Mika:** She always wrote like this: one thought to a line.

A page holds as many lines as fit; the one after that starts a fresh page.

::choice
- [Read it again](#start)
- [Put it away](#end)
::

# end

::page{off}

**Mika:** Back in the textbox. The letter goes in my pocket.
::end
