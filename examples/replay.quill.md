---
title: Replay Demo
vars: { trust: 0 }
persistent: { roof_visits: 0 }
replays: { rooftop: "The rooftop", night-walk: "Night walk" }
---

<!-- `replays:` lists labels the player can watch again from Extras > Scenes.
     A scene unlocks the first time the story reaches its label. A replay starts
     fresh at that label, never saves anything, and ends at `::end_replay`
     (or when the story ends). In the real game `::end_replay` does nothing. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

# Start

::bg{src=assets/night.png}
Two scenes in this story can be watched again later from Extras. Pick one now.

::choice
- [Go up to the rooftop](#rooftop)
- [Take a night walk](#night-walk)
::

# Rooftop

::bg{src=assets/rooftop.png}
::show{mika neutral at=right}
::set{roof_visits += 1}
**Mika:** You came up here, too. Nobody else does.

::choice
- [Tell her it is peaceful](#rooftop-kind){trust+=1}
- [Say you were just passing](#rooftop-cool){trust-=1}
::

# Rooftop kind

::show{mika happy}
**Mika:** Then stay a while.
::jump{to=rooftop-end}

# Rooftop cool

**Mika:** Suit yourself.
::jump{to=rooftop-end}

# Rooftop end

::end_replay
::hide{mika}
::jump{to=finale}

# Night walk

::bg{src=assets/night.png}
The streets are empty. Your footsteps are the only sound.
A single window is still lit.
::end_replay
::jump{to=finale}

# Finale

**Mika:** Whichever way you went, it was a good night.
Your trust is {trust}. It starts again at zero in a replay.
