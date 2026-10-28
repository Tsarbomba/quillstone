---
title: Two Languages
languages: en=English, sv=Svenska
vars: { gold: 12, name: "Alex", bag: [lamp] }
inventory: bag
---

<!-- Translating a game: the script stays in its own language (English, the first one in
     `languages:`). `quillstone lang export lang.quill.md sv` writes lang/sv.txt, a plain file with
     every text of the game; a translator fills in the `=` lines without touching this script.
     Players pick the language in Settings. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}
::item{id=lamp name="Brass lamp" text="Burns for hours."}

::bg{src=assets/rooftop.png}
::hud{id=gold text="Gold: {gold}" at=0.02,0.03 size=34 back=#00000099}

# start

::show{mika neutral at=center}

**Mika:** Hello, {name}! You carry {gold} gold and a lamp. {w=0.5}Look: **it still glows**.

::input{var=name prompt="What should I call you?" default="Alex" max=12}

**Mika:** Nice to meet you, {name}.

**Mika:** Yes.

Yes.

::choice
- [Open the door](#door)
- [Stay on the roof](#roof){gold -= 2}
::

# door

::show{mika happy at=center}

**Mika:** The door opens onto the stairs. Here, take {gold} gold with you.

::end

# roof

**Mika:** We watch the lights come on. That cost 2 gold, but it was worth it.
