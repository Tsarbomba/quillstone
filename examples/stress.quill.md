---
title: Quillstone Stress Test
---

<!-- Not a game: one menu of edge cases to look at by eye. Every test returns here. -->

::character{id=a name=Tester color=#9fd3ff}

# hub

::bg{src=assets/night.png}
**Tester:** Pick a test.

::choice
- [A very long line](#long)
- [8 choices](#eight)
- [12 choices](#twelve)
- [60 short lines (skip, auto, backlog)](#lines)
- [Long line with a pause and colours](#markup)
- [Finish](#end)
::

# long
**Tester:** The rain kept falling on the old town long after the last tram had gone, and nobody in the square seemed to mind, because everyone there had somewhere warmer to be. The rain kept falling on the old town long after the last tram had gone, and nobody in the square seemed to mind, because everyone there had somewhere warmer to be. The rain kept falling on the old town long after the last tram had gone, and nobody in the square seemed to mind, because everyone there had somewhere warmer to be.
::jump{to=hub}

# eight
**Tester:** Eight options should all fit above the textbox.
::choice
- [Option 1](#hub)
- [Option 2](#hub)
- [Option 3](#hub)
- [Option 4](#hub)
- [Option 5](#hub)
- [Option 6](#hub)
- [Option 7](#hub)
- [Option 8](#hub)
::

# twelve
**Tester:** Twelve options shrink more, and the arrow keys and mouse still reach them all.
::choice
- [Option 1](#hub)
- [Option 2](#hub)
- [Option 3](#hub)
- [Option 4](#hub)
- [Option 5](#hub)
- [Option 6](#hub)
- [Option 7](#hub)
- [Option 8](#hub)
- [Option 9](#hub)
- [Option 10](#hub)
- [Option 11](#hub)
- [Option 12](#hub)
::

# lines
Line 1 of a long run of text for skip, auto and the backlog.

Line 2 of a long run of text for skip, auto and the backlog.

Line 3 of a long run of text for skip, auto and the backlog.

Line 4 of a long run of text for skip, auto and the backlog.

Line 5 of a long run of text for skip, auto and the backlog.

Line 6 of a long run of text for skip, auto and the backlog.

Line 7 of a long run of text for skip, auto and the backlog.

Line 8 of a long run of text for skip, auto and the backlog.

Line 9 of a long run of text for skip, auto and the backlog.

Line 10 of a long run of text for skip, auto and the backlog.

Line 11 of a long run of text for skip, auto and the backlog.

Line 12 of a long run of text for skip, auto and the backlog.

Line 13 of a long run of text for skip, auto and the backlog.

Line 14 of a long run of text for skip, auto and the backlog.

Line 15 of a long run of text for skip, auto and the backlog.

Line 16 of a long run of text for skip, auto and the backlog.

Line 17 of a long run of text for skip, auto and the backlog.

Line 18 of a long run of text for skip, auto and the backlog.

Line 19 of a long run of text for skip, auto and the backlog.

Line 20 of a long run of text for skip, auto and the backlog.

Line 21 of a long run of text for skip, auto and the backlog.

Line 22 of a long run of text for skip, auto and the backlog.

Line 23 of a long run of text for skip, auto and the backlog.

Line 24 of a long run of text for skip, auto and the backlog.

Line 25 of a long run of text for skip, auto and the backlog.

Line 26 of a long run of text for skip, auto and the backlog.

Line 27 of a long run of text for skip, auto and the backlog.

Line 28 of a long run of text for skip, auto and the backlog.

Line 29 of a long run of text for skip, auto and the backlog.

Line 30 of a long run of text for skip, auto and the backlog.

Line 31 of a long run of text for skip, auto and the backlog.

Line 32 of a long run of text for skip, auto and the backlog.

Line 33 of a long run of text for skip, auto and the backlog.

Line 34 of a long run of text for skip, auto and the backlog.

Line 35 of a long run of text for skip, auto and the backlog.

Line 36 of a long run of text for skip, auto and the backlog.

Line 37 of a long run of text for skip, auto and the backlog.

Line 38 of a long run of text for skip, auto and the backlog.

Line 39 of a long run of text for skip, auto and the backlog.

Line 40 of a long run of text for skip, auto and the backlog.

Line 41 of a long run of text for skip, auto and the backlog.

Line 42 of a long run of text for skip, auto and the backlog.

Line 43 of a long run of text for skip, auto and the backlog.

Line 44 of a long run of text for skip, auto and the backlog.

Line 45 of a long run of text for skip, auto and the backlog.

Line 46 of a long run of text for skip, auto and the backlog.

Line 47 of a long run of text for skip, auto and the backlog.

Line 48 of a long run of text for skip, auto and the backlog.

Line 49 of a long run of text for skip, auto and the backlog.

Line 50 of a long run of text for skip, auto and the backlog.

Line 51 of a long run of text for skip, auto and the backlog.

Line 52 of a long run of text for skip, auto and the backlog.

Line 53 of a long run of text for skip, auto and the backlog.

Line 54 of a long run of text for skip, auto and the backlog.

Line 55 of a long run of text for skip, auto and the backlog.

Line 56 of a long run of text for skip, auto and the backlog.

Line 57 of a long run of text for skip, auto and the backlog.

Line 58 of a long run of text for skip, auto and the backlog.

Line 59 of a long run of text for skip, auto and the backlog.

Line 60 of a long run of text for skip, auto and the backlog.
::jump{to=hub}

# markup
**Tester:** A *long* {color=#ff9f9f}coloured part{/color}{w=1} after a pause, then more words so the line wraps: The rain kept falling on the old town long after the last tram had gone, and nobody in the square seemed to mind, because everyone there had somewhere warmer to be. The rain kept falling on the old town long after the last tram had gone, and nobody in the square seemed to mind, because everyone there had somewhere warmer to be. The rain kept falling on the old town long after the last tram had gone, and nobody in the square seemed to mind, because everyone there had somewhere warmer to be.
::jump{to=hub}

# end
**Tester:** Done.
