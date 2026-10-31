//! Picture names built from variables, in the game shell: the draw list uses the resolved
//! picture, an unsafe value draws nothing and tells the player once, and a typed name works.

use super::*;
use quillstone_render::Size;
use quillstone_script::{parse, MISSING_PICTURE};

const STORY: &str = "\
---
vars: { place: \"rooftop\", mood: \"happy\", hero.name: \"Alex\" }
---
::character{id=mika sprites=art/mika_{pose}.png}
# start
::bg{src=\"bg/{place}.png\"}
::show{mika {mood} x=right}
![](items/{place}.png)
One.
::choice
- [Night](#night){place=\"night\", mood=\"sad\"}
- [Away](#away){place=\"../x\"}
- [Slash](#away){mood=\"a/b\"}
::
# night
::bg{src=\"bg/{place}.png\"}
::show{mika {mood}}
Night.
::end
# away
::bg{src=\"bg/{place}.png\"}
::show{mika {mood}}
Away.
::bg{src=\"bg/{place}.png\"}
::show{mika {mood}}
Again.
::end
";

fn new_game<'p>(p: &'p Program) -> Game<'p> {
    let mut g = Game::new(p, Settings { text_speed: 0.0, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g
}

fn key(g: &mut Game, k: Key) {
    g.handle(Event::Key(k));
}

/// Every picture file exists and is 500x1000, except the placeholder for a missing one.
fn size(src: &str) -> Option<Size> {
    (src != MISSING_PICTURE).then(|| Size::new(500.0, 1000.0))
}

fn pics(g: &Game) -> Vec<String> {
    g.draw_list(&size)
        .iter()
        .filter_map(|d| match d {
            Draw::Image { src, .. } => Some(src.clone()),
            _ => None,
        })
        .collect()
}

fn notice(g: &Game) -> Option<String> {
    g.notice.as_ref().map(|(m, _)| m.clone())
}

fn pick(g: &mut Game, text: &str) {
    let i = g.view().choices.iter().position(|c| c.contains(text)).unwrap_or_else(|| panic!("no choice {text}"));
    g.pick(i);
}

#[test]
fn the_draw_list_uses_the_resolved_pictures() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    let shown = pics(&g);
    for want in ["bg/rooftop.png", "art/mika_happy.png", "items/rooftop.png"] {
        assert!(shown.iter().any(|s| s == want), "{want} in {shown:?}");
    }
    assert!(shown.iter().all(|s| !s.contains('{')), "{shown:?}");
    key(&mut g, Key::Space);
    pick(&mut g, "Night");
    let shown = pics(&g);
    assert!(shown.iter().any(|s| s == "bg/night.png") && shown.iter().any(|s| s == "art/mika_sad.png"), "{shown:?}");
    assert!(notice(&g).is_none());
    // Rolling back brings the earlier pictures back.
    g.handle(Event::Wheel(1.0));
    let shown = pics(&g);
    assert!(shown.iter().any(|s| s == "bg/rooftop.png") && shown.iter().any(|s| s == "art/mika_happy.png"), "{shown:?}");
}

#[test]
fn looking_ahead_loads_nothing_for_a_name_that_is_not_known_yet() {
    let p = parse(STORY).unwrap();
    let g = new_game(&p);
    assert!(g.upcoming_images().iter().all(|s| !s.contains('{')), "{:?}", g.upcoming_images());
    assert!(g.upcoming_images().is_empty(), "{:?}", g.upcoming_images());
}

#[test]
fn an_unsafe_value_draws_nothing_and_tells_the_player_once() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    key(&mut g, Key::Space);
    pick(&mut g, "Away");
    // The background is the placeholder (no file), the sprite is a normal pose.
    let shown = pics(&g);
    assert!(shown.iter().all(|s| !s.contains("..") && !s.contains("{")), "{shown:?}");
    assert!(!shown.iter().any(|s| s.starts_with("bg/")), "nothing from bg/: {shown:?}");
    assert!(!shown.iter().any(|s| s == MISSING_PICTURE), "the placeholder has no file, so the draw list holds nothing for it");
    assert!(shown.iter().any(|s| s == "art/mika_happy.png"));
    let first = notice(&g).expect("the player is told");
    assert!(first.contains("{place}") && !first.contains("../x"), "{first}");
    // The same problem again (next lines) does not show a second notice.
    g.tick(60.0, false);
    assert!(notice(&g).is_none(), "the notice went away");
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "Again.");
    g.tick(0.0, false);
    assert!(notice(&g).is_none(), "once: {:?}", notice(&g));
}

#[test]
fn an_unsafe_pose_draws_no_sprite() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    key(&mut g, Key::Space);
    pick(&mut g, "Slash");
    let shown = pics(&g);
    assert!(!shown.iter().any(|s| s.contains("mika")), "{shown:?}");
    assert!(notice(&g).is_some_and(|n| n.contains("{mood}")));
}

fn line(g: &Game) -> String {
    g.view().dialogue.as_ref().map(|d| d.text.clone()).unwrap_or_default()
}

const ASK: &str = "---\nvars: { hero.name: \"Alex\" }\n---\n::input{var=hero.name prompt=\"Name?\" default=\"Alex\" max=64}\n::bg{src=\"faces/{hero.name}.png\"}\nHi {hero.name}.\n";

#[test]
fn a_typed_name_flows_into_a_picture_name_safely() {
    let p = parse(ASK).unwrap();
    let mut g = new_game(&p);
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Input);
    g.handle(Event::Text("Mika".into()));
    key(&mut g, Key::Enter);
    assert!(pics(&g).iter().any(|s| s == "faces/Mika.png"), "{:?}", pics(&g));
    assert!(notice(&g).is_none());
    for hostile in ["../../x", "a/b", "C:x", "a\\b", ".."] {
        let mut g = new_game(&p);
        key(&mut g, Key::Space);
        g.handle(Event::Text(hostile.into()));
        key(&mut g, Key::Enter);
        let shown = pics(&g);
        assert!(shown.iter().all(|s| !s.contains("faces/")), "{hostile}: {shown:?}");
        assert!(notice(&g).is_some(), "{hostile}");
    }
}

#[test]
fn the_places_demo_shows_both_backgrounds_and_both_poses() {
    let p = parse(include_str!("../../../../examples/places.quill.md")).unwrap();
    let mut g = new_game(&p);
    let shown = pics(&g);
    assert!(shown.iter().any(|s| s == "assets/places/rooftop.png") && shown.iter().any(|s| s == "assets/mika_neutral.png"), "{shown:?}");
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    pick(&mut g, "night sky, in a good mood");
    let shown = pics(&g);
    assert!(shown.iter().any(|s| s == "assets/places/night.png") && shown.iter().any(|s| s == "assets/mika_happy.png"), "{shown:?}");
    assert!(notice(&g).is_none());
}
