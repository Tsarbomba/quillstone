---
title: Splash Demo
persistent: { launches: 0 }
---

<!-- A label called `splash` runs at every launch, before the main menu. Here it shows two logos and a
     content warning. It ends at its `::return` (or where the next label starts), and then the menu appears.
     Esc skips it. It is never saved or rolled back, and a new game starts at the top of the story,
     not in it. What it `::set`s on a persistent variable is kept. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

# Start

::bg{src=assets/night.png}
::show{mika neutral x=50%}
**Mika:** Before the main menu you saw a splash screen: two logos and a warning. It runs at every launch, and it has run {launches} times for you so far.

**Mika:** Quit and open the game again to see it, or press Esc during it to skip to the menu.

::end

# Splash

::set{launches += 1}
::bg{color=#000000}
::show{id=logo-a src=assets/lamp_glow.png x=center y=center fade=1}
::wait{time=2.5}
::hide{logo-a fade=0.5}
::wait{time=0.7}
::show{id=logo-b src=assets/scarf.png x=center y=center fade=1}
::wait{time=2.5}
::hide{logo-b fade=0.5}
::wait{time=0.7}
This story is about a quiet night. Nothing in it is frightening.
::return
