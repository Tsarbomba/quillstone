---
title: Chapters Demo
vars: { trust: 0 }
---

<!-- A story split over several files. `::include` reads another script file at this spot, as if
     its lines were written here: labels, characters and everything else it declares are shared.
     Paths are relative to this file's folder. Front matter (the block above) belongs here only. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

::include{chapters/ch1.quill.md}
::include{src=chapters/ch2.quill.md}

# ending

::if{trust > 0}
**Mika:** I'm glad you stayed.
::else
**Mika:** Maybe next time.
::endif
::end
