---
title: Chapter Select Demo
id: chapter-select-demo
chapters: { start: "The beginning" | assets/rooftop.png, night-walk: "A walk at night" | assets/night.png, finale: "Finale" {open} }
---

<!-- `chapters:` adds a Chapters button to the main menu. Each entry is `label: "Title"`, with an
     optional thumbnail after a `|`. A chapter unlocks when the story first reaches its label (and
     stays unlocked in later games); `{open}` makes it available from the very first launch.
     Picking an open tile starts a NEW game at that label. -->

# start

**Mika:** Welcome. Back at the main menu there is a Chapters button, because this script lists its chapters in the front matter.

**Mika:** This first chapter is unlocked by playing it. The last one, the finale, is marked open, so it was there from the start.

::choice
- [Take a walk at night](#night-walk)
- [Skip to the finale](#finale)
::

# night-walk

::bg{src=assets/night.png fade=1}
**Mika:** Reaching this label just unlocked "A walk at night". Next time, start it straight from the Chapters screen.

# finale

**Mika:** And that is the finale. Chapters start a new game, so everything begins with the variables from the front matter.
