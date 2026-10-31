---
title: Gallery Demo
theme: theme.txt
gallery: [assets/rooftop.png, assets/flashback.webm | assets/thumbs/flashback.png, assets/night.png]
---

<!-- A gallery can mix pictures and video clips. A picture unlocks when the story shows it;
     a clip unlocks the first time the story plays it as a looping `::bg` (or a `::video`).
     A clip can name a picture for its tile after a `|` (here assets/thumbs/flashback.png).
     Play through once, then open Extras from the main menu. -->

# start

::bg{src=assets/rooftop.png}
A picture for the gallery: the rooftop.

::bg{src=assets/flashback.webm}
A looping clip for the gallery: the same rooftop, remembered.

::bg{src=assets/night.png fade=1}
And the night sky to end on. Open Extras at the main menu to see all three.
