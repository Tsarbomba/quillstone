---
title: Phone Demo
theme: phone_theme.txt
vars: { going: false }
---

<!-- `::message` puts a message in a phone-style chat with a character; `::reply` offers answers in it.
     The Messages screen opens from a HUD button with `open=messages` (there is no key for it). -->

::character{id=mika name=Mika color=#ffd98c sprites=assets/mika_{pose}.png portrait=assets/mika_happy.png}

# start

::bg{src=assets/rooftop.png}
::show{mika neutral x=80%}

**Mika:** I'll text you the details. Here, this button is your phone.

<!-- The button opens the Messages screen: contacts, then the chat. -->
::hud{id=phone text="Phone" open=messages x=0.9 y=0.05 back=#00000099 pad=10}

**Mika:** Check it when you can.

<!-- A message from Mika: a notice shows ("quiet" would hide it), and the thread counts it as unread. -->
::message{from=mika text="Hi! It's Mika. The roof at eight?"}

**Mika:** There. Take a look.

**Mika:** You answered me earlier, remember? Your own messages show on the right.
<!-- `to=` is a message from the player: it never shows a notice. -->
::message{to=mika text="Sure, I'll bring tea."}

<!-- A picture, or a video with a thumbnail, shows in its bubble and opens enlarged when picked. -->
::message{from=mika image=assets/night.png text="This is how it looks at night."}
::message{from=mika video=assets/flashback.webm thumb=assets/thumbs/flashback.png text="And last summer!" quiet}

**Mika:** You can answer later, or right now.

<!-- A reply the story does not wait for: the options sit in Mika's thread until you pick one. -->
::reply{to=mika later}
- [Sounds great, I'll be there](#later_yes){going=true}
- [Can't make it](#later_no)
::

**Mika:** Take your time. Open your phone whenever you like, then go on.
**Mika:** Ready for the real question?

<!-- A reply the story waits for: the Messages screen opens on the chat with the options at the bottom. -->
::message{from=mika text="So, are you coming tonight?"}
::reply{to=mika}
- [Yes, see you at eight](#yes){going=true}
- [Not tonight](#no)
::

# yes
**Mika:** Wonderful. Bring a jacket.
::jump{to=end}

# no
**Mika:** Oh well. Next time.
::jump{to=end}

# later_yes
**Mika:** (Your answer reached her.)
::message{from=mika text="Yay!"}
::return

# later_no
**Mika:** (Your answer reached her.)
::return

# end
::if{going}
**Mika:** See you on the roof.
::else
**Mika:** I'll keep a seat free.
::endif
::end
