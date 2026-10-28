---
title: Weather
---

<!-- `::particles` puts cheap ambient weather over the picture: snow, rain, or any small
     picture of your own. Settings: `amount=` (1 to 200, default 60), `speed=` (0.2 to 5),
     `wind=` (-2 to 2, negative blows left) and `fade=` (seconds to ease in, default 1).
     `layer=back` puts them behind the sprites. `::particles{none}` stops them. -->

# start

::bg{src=assets/night.png}

Snow, the default: 60 small flakes drifting down.

::particles{snow}

A heavier fall, pushed by a wind from the left.

::particles{snow amount=140 speed=1.4 wind=0.8 fade=2}

The snow turns to rain, which falls faster and slants with the wind.

::particles{rain amount=120 wind=0.6}

A gale.

::particles{rain amount=200 speed=1.3 wind=-1.8}

The rain stops.

::particles{none fade=2}

Any picture can fall: petals of a small file (32 px or so is plenty), a few at a time.

::particles{src=assets/petal.png amount=24 speed=0.9 wind=0.3}

They stay behind the text box, the menus and the HUD.

::particles{none}

The end.
