---
title: Places
vars: { place: "rooftop", mood: "neutral" }
---

<!-- Picture names built from variables: `{place}` picks the background and `{mood}` picks
     Mika's pose. A pattern like `assets/places/{place}.png` stands for every file it can
     build, so `check` needs at least one match and `package` ships them all (that is why the
     two backgrounds have a folder of their own). See "Picture names from variables" in SCRIPT.md. -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png}

# start

::bg{src="assets/places/{place}.png"}
::show{mika {mood} at=right}
**Mika:** The picture behind me is chosen by the variable `place`, and my pose by `mood`.
**Mika:** Right now `place` is "{place}" and `mood` is "{mood}".

# pick

::choice
- [Go to the rooftop, in a good mood](#go){place="rooftop", mood="happy"}
- [Go to the night sky, calm](#go){place="night", mood="neutral"}
- [Go to the night sky, in a good mood](#go){place="night", mood="happy"}
- [That is enough](#end)
::

# go

::bg{src="assets/places/{place}.png" fade=0.8}
::show{mika {mood}}
**Mika:** Now `place` is "{place}" and `mood` is "{mood}".

::choice
- [Try another place](#pick)
- [That is enough](#end)
::

# end

**Mika:** A name that isn't a safe file-name part, such as one with a slash, draws nothing instead of reaching for another folder.
