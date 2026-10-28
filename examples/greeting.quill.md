---
title: Greeting Demo
vars: { hero.name: "Alex" }
---

<!-- `::input` asks the player to type a text and stores it in a text variable.
     `default` is used when the field is left empty, `max` limits the length.
     Whatever is typed shows exactly as typed, even if it has braces or stars. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

# start

::bg{src=assets/rooftop.png}
::show{mika neutral at=right}

**Mika:** Hi! You must be the new student.

::input{var=hero.name prompt="What is your name?" default="Alex" max=16}

::show{mika happy}
**Mika:** Nice to meet you, {hero.name}!
**Mika:** Welcome to the rooftop club, {hero.name}. We don't have many rules.

::choice
- [Ask what the club does](#club)
- [Say goodbye](#bye)
::

# club

**Mika:** We watch the sky, mostly. You will fit in, {hero.name}.
::jump{to=bye}

# bye

**Mika:** See you tomorrow, **{hero.name}**!
