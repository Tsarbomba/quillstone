//! The compiled form of a Quillstone script.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fmt;
use std::path::{Component, Path};

/// A script value. Booleans compare as 0/1; unset variables read as `Int(0)`.
///
/// `List` is a set of non-empty texts (no duplicates) that remembers the order
/// items were added in. Lists exist only where the front matter declares them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Str(String),
    List(Vec<String>),
}

impl Value {
    /// `0`, `false` and `""` are false; everything else is true.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Int(n) => *n != 0,
            Value::Bool(b) => *b,
            Value::Str(s) => !s.is_empty(),
            Value::List(items) => !items.is_empty(),
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Bool(b) => Some(*b as i64),
            Value::Str(_) | Value::List(_) => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Str(s) => f.write_str(s),
            Value::List(items) => f.write_str(&items.join(", ")),
        }
    }
}

/// Script variables.
pub type Vars = BTreeMap<String, Value>;

/// Hidden variables `seen:<label>` count how often the story passed a label
/// (see `seen(label)` in conditions). Real variable names can't contain `:`.
pub const SEEN: &str = "seen:";

/// Hidden variables `once:<label>:<text>` remember that a `{once}` option was picked.
pub const ONCE: &str = "once:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Cmp {
    fn holds(self, ord: Ordering) -> bool {
        match self {
            Cmp::Eq => ord == Ordering::Equal,
            Cmp::Ne => ord != Ordering::Equal,
            Cmp::Lt => ord == Ordering::Less,
            Cmp::Le => ord != Ordering::Greater,
            Cmp::Gt => ord == Ordering::Greater,
            Cmp::Ge => ord != Ordering::Less,
        }
    }
}

/// An expression: `trust > 1 and not met`, `name == "Mika"`, `gold + 0`.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(Value),
    Var(String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cmp(Box<Expr>, Cmp, Box<Expr>),
    /// `inventory has "key"`: the list variable holds that text.
    Has(String, Box<Expr>),
}

/// A condition is just an expression that is tested for truthiness.
pub type Cond = Expr;

impl Expr {
    pub fn eval(&self, vars: &Vars) -> Value {
        match self {
            Expr::Lit(v) => v.clone(),
            Expr::Var(name) => vars.get(name).cloned().unwrap_or(Value::Int(0)),
            Expr::Not(e) => Value::Bool(!e.eval(vars).truthy()),
            Expr::And(a, b) => Value::Bool(a.eval(vars).truthy() && b.eval(vars).truthy()),
            Expr::Or(a, b) => Value::Bool(a.eval(vars).truthy() || b.eval(vars).truthy()),
            Expr::Cmp(a, op, b) => Value::Bool(compare(&a.eval(vars), *op, &b.eval(vars))),
            Expr::Has(name, item) => Value::Bool(match (vars.get(name), item.eval(vars)) {
                (Some(Value::List(items)), Value::Str(s)) => items.contains(&s),
                _ => false,
            }),
        }
    }

    pub fn test(&self, vars: &Vars) -> bool {
        self.eval(vars).truthy()
    }

    /// Every variable the expression reads, in order (with repeats).
    pub fn vars_read<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Expr::Lit(_) => {}
            Expr::Var(name) => out.push(name),
            Expr::Not(e) => e.vars_read(out),
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Cmp(a, _, b) => {
                a.vars_read(out);
                b.vars_read(out);
            }
            Expr::Has(name, item) => {
                out.push(name);
                item.vars_read(out);
            }
        }
    }

    /// Every `list has item` test inside the expression, as `(list variable, item expression)`.
    pub fn has_tests<'a>(&'a self, out: &mut Vec<(&'a str, &'a Expr)>) {
        match self {
            Expr::Lit(_) | Expr::Var(_) => {}
            Expr::Not(e) => e.has_tests(out),
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Cmp(a, _, b) => {
                a.has_tests(out);
                b.has_tests(out);
            }
            Expr::Has(name, item) => {
                out.push((name, item));
                item.has_tests(out);
            }
        }
    }
}

/// Lists are sets: the same items in any order are equal.
fn same_items(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

fn compare(a: &Value, op: Cmp, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => op.holds(x.cmp(y)),
        // sets are equal or not; they have no order
        (Value::List(x), Value::List(y)) => match op {
            Cmp::Eq => same_items(x, y),
            Cmp::Ne => !same_items(x, y),
            _ => false,
        },
        _ => match (a.as_int(), b.as_int()) {
            (Some(x), Some(y)) => op.holds(x.cmp(&y)),
            // text vs number: never equal, never ordered
            _ => op == Cmp::Ne,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
}

/// A variable change like `trust+=1` or `name="Alex"`.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub var: String,
    pub op: AssignOp,
    pub value: Expr,
}

impl Effect {
    pub fn apply(&self, vars: &mut Vars) {
        let rhs = self.value.eval(vars);
        // A list changes only by `+=` / `-=` of text; anything else leaves it alone.
        if let Some(Value::List(items)) = vars.get_mut(&self.var) {
            match (self.op, rhs) {
                (AssignOp::Add, Value::Str(s)) if !s.is_empty() && !items.contains(&s) => items.push(s),
                (AssignOp::Sub, Value::Str(s)) => items.retain(|i| *i != s),
                _ => {}
            }
            return;
        }
        match self.op {
            AssignOp::Set => {
                // lists only come from the front matter, never from an assignment
                if !matches!(rhs, Value::List(_)) {
                    vars.insert(self.var.clone(), rhs);
                }
            }
            AssignOp::Add => {
                let new = match (vars.get(&self.var), &rhs) {
                    (_, Value::List(_)) => None,
                    // appending text builds a string (an unset variable counts as empty)
                    (Some(Value::Str(s)), r) => Some(Value::Str(format!("{s}{r}"))),
                    (None, Value::Str(s)) => Some(Value::Str(s.clone())),
                    (cur, r) => {
                        let a = cur.map_or(Some(0), Value::as_int);
                        match (a, r.as_int()) {
                            (Some(a), Some(b)) => Some(Value::Int(a.saturating_add(b))),
                            _ => None,
                        }
                    }
                };
                if let Some(v) = new {
                    vars.insert(self.var.clone(), v);
                }
            }
            AssignOp::Sub => {
                let a = vars.get(&self.var).map_or(Some(0), Value::as_int);
                if let (Some(a), Some(b)) = (a, rhs.as_int()) {
                    vars.insert(self.var.clone(), Value::Int(a.saturating_sub(b)));
                }
            }
        }
    }
}

/// A character declared with `::character`.
#[derive(Debug, Clone, PartialEq)]
pub struct Character {
    /// Lowercase id used in `::show{mika ...}`.
    pub id: String,
    /// Display name shown on the name plate.
    pub name: String,
    /// Name plate colour, 0..1 RGB.
    pub color: Option<[f32; 3]>,
    /// Sprite path template; `{pose}` is replaced by the pose name.
    pub sprites: Option<String>,
}

/// An item declared with `::item`. Its `id` is the text a list variable holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    /// Shown in the inventory and shop; the id when not given.
    pub name: String,
    /// A picture (png, jpg or webp) for the grid.
    pub icon: Option<String>,
    /// What a shop asks, unless its line says otherwise.
    pub price: Option<i64>,
    /// The description shown when the item is hovered or selected.
    pub text: String,
    /// Source line (1-based) of the declaration.
    pub line: usize,
}

/// One line of a `::shop` block.
#[derive(Debug, Clone, PartialEq)]
pub struct ShopEntry {
    /// The id of a declared item.
    pub item: String,
    /// The item's price, or the line's own `price=`.
    pub price: i64,
    /// The item is only offered while this is true.
    pub cond: Option<Cond>,
}

/// A fully compiled script: a flat list of ops with all jumps resolved to indices.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub title: Option<String>,
    /// The theme file named in the front matter (`theme: theme.txt`), relative to the script.
    pub theme: Option<String>,
    /// Initial variable values from the front matter.
    pub vars: Vars,
    pub characters: BTreeMap<String, Character>,
    /// Items declared with `::item`, by id.
    pub items: BTreeMap<String, Item>,
    /// The list variable the inventory screen shows (front matter `inventory:`).
    pub inventory: Option<String>,
    pub ops: Vec<Op>,
    /// Source line (1-based) of each op; parallel to `ops`. Used for error messages.
    pub lines: Vec<usize>,
    /// Label name (slugified heading) -> index of the first op after it.
    pub labels: BTreeMap<String, usize>,
    /// Labels written with parameters (`# heal(amount, who)`): label -> parameter names.
    /// Only `::call` reaches them, and it must pass exactly these.
    pub params: BTreeMap<String, Vec<String>>,
    /// Labels whose visits are counted because a `seen(label)` reads them:
    /// op index -> the `seen:` variables to count up when that op runs.
    pub counted: BTreeMap<usize, Vec<String>>,
    /// Variables kept across playthroughs (front matter `persistent:`). Their
    /// starting values are in `vars` like any other.
    pub persistent: BTreeSet<String>,
    /// Pictures for the extras gallery, unlocked when the story shows them.
    pub gallery: Vec<String>,
    /// Tracks for the extras music room, unlocked when the story plays them.
    pub music_room: Vec<String>,
    /// Auto-voice: the folder of voice files named after their lines (front
    /// matter `voice:`), relative to the script. See `voice_name` in the runtime.
    pub voice_folder: Option<String>,
}

impl Program {
    /// Files named in the front matter (gallery pictures and music room tracks).
    pub fn extra_assets(&self) -> impl Iterator<Item = (AssetKind, &str)> {
        let gallery = self.gallery.iter().map(|p| (kind_from_path(p), p.as_str()));
        gallery.chain(self.music_room.iter().map(|p| (AssetKind::Audio, p.as_str())))
    }

    /// The indices of the ops within `steps` ops of `pc`, nearest first, following
    /// jumps, calls and every choice.
    pub fn ops_ahead(&self, pc: usize, steps: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([pc]);
        while let Some(at) = queue.pop_front() {
            if seen.len() >= steps || !seen.insert(at) {
                continue;
            }
            let Some(op) = self.ops.get(at) else { continue };
            out.push(at);
            match op {
                Op::Jump { target, cond: None } => queue.push_back(*target),
                Op::Jump { target, .. } => queue.extend([*target, at + 1]),
                Op::Call { target, .. } => queue.extend([*target, at + 1]),
                Op::Choice { options } => queue.extend(options.iter().map(|o| o.target)),
                Op::Return | Op::End => {}
                _ => queue.push_back(at + 1),
            }
        }
        out
    }

    /// The images the story may show within `steps` ops of `pc`, nearest first,
    /// following jumps, calls and every choice (for loading them before they're needed).
    pub fn images_ahead(&self, pc: usize, steps: usize) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for at in self.ops_ahead(pc, steps) {
            for (kind, src) in self.ops[at].all_assets() {
                if kind == AssetKind::Image && !out.contains(&src) {
                    out.push(src);
                }
            }
        }
        out
    }

    /// Whether the story may open the inventory, and which item icons the shops
    /// it may open sell, within `steps` ops of `pc` (see [`Program::ops_ahead`]).
    pub fn bag_icons_ahead(&self, pc: usize, steps: usize) -> (bool, Vec<&str>) {
        let (mut inventory, mut icons) = (false, Vec::new());
        for at in self.ops_ahead(pc, steps) {
            match &self.ops[at] {
                Op::Inventory => inventory = true,
                Op::Shop { stock, .. } => {
                    for icon in stock.iter().filter_map(|s| self.items.get(&s.item)?.icon.as_deref()) {
                        if !icons.contains(&icon) {
                            icons.push(icon);
                        }
                    }
                }
                _ => {}
            }
        }
        (inventory, icons)
    }

    /// The icon of every declared item, as `(source line, path)`, in the order the
    /// items are declared in the script.
    pub fn item_icons(&self) -> impl Iterator<Item = (usize, &str)> {
        let mut icons: Vec<(usize, &str)> = self.items.values().filter_map(|i| Some((i.line, i.icon.as_deref()?))).collect();
        icons.sort_by_key(|(line, _)| *line);
        icons.into_iter()
    }

    /// Like [`Program::assets`] but with the source line of each file's own mention:
    /// a hotspot image reports its region's line, not the `::hotspots` line.
    /// `(source line, kind, path)`, in script order.
    pub fn assets_with_lines(&self) -> impl Iterator<Item = (usize, AssetKind, &str)> {
        self.ops.iter().enumerate().flat_map(|(i, op)| {
            let op_line = self.lines.get(i).copied().unwrap_or(0);
            let found: Vec<(usize, AssetKind, &str)> = match op {
                Op::Choice { options } => options
                    .iter()
                    .filter_map(|o| Some((o.line, AssetKind::Image, o.area.as_ref()?.image.as_deref()?)))
                    .collect(),
                _ => op.all_assets().into_iter().map(|(kind, path)| (op_line, kind, path)).collect(),
            };
            found
        })
    }

    /// Literal item texts that name no `::item`, where the inventory list is
    /// changed with `+=` or tested with `has`: `(op index, text)`. Empty when the
    /// game has no inventory or declares no items.
    pub fn unknown_items(&self) -> Vec<(usize, String)> {
        let (Some(list), false) = (&self.inventory, self.items.is_empty()) else { return Vec::new() };
        let mut out = Vec::new();
        for (at, op) in self.ops.iter().enumerate() {
            let effects: Vec<&Effect> = match op {
                Op::Set(e) => vec![e],
                Op::Choice { options } => options.iter().flat_map(|o| &o.effects).collect(),
                _ => Vec::new(),
            };
            let mut texts: Vec<&str> = effects
                .into_iter()
                .filter(|e| e.var == *list && e.op != AssignOp::Set)
                .filter_map(|e| match &e.value {
                    Expr::Lit(Value::Str(s)) => Some(s.as_str()),
                    _ => None,
                })
                .collect();
            let mut tests = Vec::new();
            for e in op.exprs() {
                e.has_tests(&mut tests);
            }
            for (name, item) in tests {
                if let (true, Expr::Lit(Value::Str(s))) = (name == list, item) {
                    texts.push(s);
                }
            }
            for t in texts {
                if !self.items.contains_key(t) && !out.iter().any(|(a, x)| *a == at && x == t) {
                    out.push((at, t.to_string()));
                }
            }
        }
        out
    }

    /// Every asset the script references, as `(op index, kind, path)`.
    pub fn assets(&self) -> impl Iterator<Item = (usize, AssetKind, &str)> {
        self.ops
            .iter()
            .enumerate()
            .flat_map(|(i, op)| op.all_assets().into_iter().map(move |(kind, path)| (i, kind, path)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    #[default]
    Contain,
    Cover,
    Stretch,
}

impl Fit {
    pub fn parse(s: &str) -> Option<Fit> {
        match s {
            "contain" => Some(Fit::Contain),
            "cover" => Some(Fit::Cover),
            "stretch" => Some(Fit::Stretch),
            _ => None,
        }
    }
}

/// How an animation speeds up and slows down (`ease=` in the script).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ease {
    Linear,
    /// Starts slow, ends fast.
    In,
    /// Starts fast, ends slow.
    Out,
    /// Slow at both ends: the natural default for movement.
    #[default]
    InOut,
}

impl Ease {
    pub fn parse(s: &str) -> Option<Ease> {
        match s {
            "linear" => Some(Ease::Linear),
            "in" => Some(Ease::In),
            "out" => Some(Ease::Out),
            "inout" | "in-out" => Some(Ease::InOut),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Ease::Linear => "linear",
            Ease::In => "in",
            Ease::Out => "out",
            Ease::InOut => "inout",
        }
    }

    /// Progress `t` (0..1, clamped) through the curve (cubic easing).
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::In => t * t * t,
            Ease::Out => 1.0 - (1.0 - t).powi(3),
            Ease::InOut if t < 0.5 => 4.0 * t * t * t,
            Ease::InOut => 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0,
        }
    }
}

/// Which way a `::shake` moves the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    X,
    Y,
    #[default]
    Both,
}

impl Axis {
    pub fn parse(s: &str) -> Option<Axis> {
        match s {
            "x" => Some(Axis::X),
            "y" => Some(Axis::Y),
            "both" => Some(Axis::Both),
            _ => None,
        }
    }
}

/// The direction a wipe's edge travels (`wipe=` on `::bg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wipe {
    Left,
    Right,
    Up,
    Down,
}

impl Wipe {
    pub fn parse(s: &str) -> Option<Wipe> {
        match s {
            "left" => Some(Wipe::Left),
            "right" => Some(Wipe::Right),
            "up" => Some(Wipe::Up),
            "down" => Some(Wipe::Down),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Image,
    Video,
    Audio,
}

/// A relative path that stays inside the folder it's relative to.
/// `\` is refused on every system: on Windows it is a separator, so
/// `assets\..\..\x.png` would climb out there even though it is one name elsewhere.
/// `:` is refused too, since Windows reads `C:x.png` as a drive; `//host/share` is already a root.
pub fn is_inside(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Guess what kind of asset a path is from its extension.
pub fn kind_from_path(path: &str) -> AssetKind {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "webm" | "mp4" | "m4v" | "mkv" | "mov" | "ogv" | "avi" => AssetKind::Video,
        "ogg" | "oga" | "mp3" | "wav" | "flac" | "opus" | "m4a" | "aac" => AssetKind::Audio,
        _ => AssetKind::Image,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceOption {
    pub text: String,
    /// Op index to continue at when picked.
    pub target: usize,
    pub effects: Vec<Effect>,
    /// The option is only offered while this is true.
    pub cond: Option<Cond>,
    /// Set for the entries of a `::hotspots` block: where on the screen to click.
    pub area: Option<Area>,
    /// Source line (1-based) of the option.
    pub line: usize,
}

/// A clickable region of the screen (`::hotspots`).
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    /// `x, y, w, h` as fractions (0..1) of the screen.
    pub rect: [f32; 4],
    /// Drawn stretched into the region; invisible until hovered without one.
    pub image: Option<String>,
    /// No tint, outline or tooltip, and skipped by keyboard focus; still clickable.
    pub hidden: bool,
}

/// One instruction of the compiled script.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Dialogue (`speaker` is `Some`) or narration (`None`). Waits for the player.
    Say { speaker: Option<String>, color: Option<[f32; 3]>, text: String },
    /// A still image shown inline (`![alt](path)`) together with the next waiting line.
    Image { src: String, alt: String },
    /// Background. `src` may be a video (the backend plays it looped). With a
    /// `wipe`, the new picture is revealed from one side over `fade` seconds.
    Bg { src: String, fade: f32, wipe: Option<Wipe> },
    /// Show (or change the pose of) a sprite. `at` is 0..1 across the screen;
    /// `None` keeps the current position, or centres a new sprite. `y` is where
    /// the sprite's bottom edge is, as a fraction of the screen height (-0.5 to
    /// 1.5; new sprites stand on the bottom edge, 1.0). `z` orders sprites (-100
    /// to 100, default 0; higher is in front). Both keep the current value when `None`.
    /// `flip` mirrors the picture left to right; `xscale` and `yscale` (0.05 to 10)
    /// stretch it along one axis, on top of `scale`. All three keep the sprite's
    /// current value when `None` (and are off / 1 for a new sprite).
    Show {
        id: String,
        src: String,
        at: Option<f32>,
        y: Option<f32>,
        z: Option<i32>,
        scale: Option<f32>,
        flip: Option<bool>,
        xscale: Option<f32>,
        yscale: Option<f32>,
        fade: f32,
    },
    /// Hide one sprite, or all of them when `id` is `None`.
    Hide { id: Option<String>, fade: f32 },
    /// Slide and/or resize a sprite. `None` leaves that property as it is. `flip`
    /// switches at the start of the move; the rest animate.
    Move {
        id: String,
        at: Option<f32>,
        y: Option<f32>,
        scale: Option<f32>,
        flip: Option<bool>,
        xscale: Option<f32>,
        yscale: Option<f32>,
        time: f32,
        ease: Ease,
    },
    /// Shake the stage (background and sprites) for `time` seconds, by up to
    /// `strength` pixels (at 1920x1080) along `axis`. Doesn't wait.
    Shake { time: f32, strength: f32, axis: Axis },
    /// Pan and zoom the stage: `zoom` (1 = whole stage) centred on the point
    /// `x`, `y` (0..1 across and down the stage). `None` keeps the current value.
    Camera { zoom: Option<f32>, x: Option<f32>, y: Option<f32>, time: f32, ease: Ease },
    /// Background music. With an `intro`, that file plays once and `src` then loops.
    Music { src: String, intro: Option<String>, fade: f32, looped: bool, volume: f32 },
    StopMusic { fade: f32 },
    /// One-shot sound effect.
    Sfx { src: String, volume: f32 },
    /// A looping sound effect (`::sfx{src=... loop}`); it belongs to the scene.
    /// Starting one that is already looping only changes its volume.
    SfxLoop { src: String, fade: f32, volume: f32 },
    /// `::sfx{stop=file}` (one file) or `::sfx{stop}` (`None`: everything on the
    /// sound-effect channel): stops its loop and any one-shot of it still playing.
    StopSfx { src: Option<String>, fade: f32 },
    /// Hide or show the whole dialogue box (textbox, name plate, line, quick menu),
    /// fading over `fade` seconds (0: at once). A line or choice shows it again by itself.
    Textbox { hidden: bool, fade: f32 },
    /// A full-screen flash of `color` that fades out over `time` seconds. Doesn't wait.
    Flash { time: f32, color: [f32; 3] },
    /// Fade the stage to `color` over `time` seconds and keep it covered. Waits
    /// until the fade is done (like a `::wait`).
    FadeOut { time: f32, color: [f32; 3] },
    /// Fade the cover away over `time` seconds. Doesn't wait.
    FadeIn { time: f32 },
    /// Voice clip for the next line.
    Voice { src: String },
    /// Pause for `seconds` (the player can skip it). Waits like a line does.
    Wait { seconds: f32 },
    /// Fullscreen video clip. Waits until it ends or the player skips it.
    Video { src: String, fit: Fit, looped: bool, skippable: bool },
    /// Waits for the player to pick an option (or, when the options have an
    /// `area`, to click a region of the screen).
    Choice { options: Vec<ChoiceOption> },
    /// The inventory screen; waits until the player closes it.
    Inventory,
    /// A shop: items for sale, paid with the int variable `pay`, bought into the
    /// list `into`. Waits until the player leaves.
    Shop { pay: String, into: String, stock: Vec<ShopEntry> },
    Jump { target: usize, cond: Option<Cond> },
    /// Jump to a label and come back at the next op after `::return`. `args` are
    /// the label's parameters: each is evaluated (all of them before any is
    /// stored) and assigned to the ordinary variable of that name before the jump.
    Call { target: usize, args: Vec<(String, Expr)> },
    Return,
    Set(Effect),
    /// End of the script. The parser always appends one.
    End,
}

impl Op {
    /// Every expression in the op (conditions, and the values of assignments).
    pub fn exprs(&self) -> Vec<&Expr> {
        match self {
            Op::Jump { cond: Some(c), .. } => vec![c],
            Op::Set(e) => vec![&e.value],
            Op::Call { args, .. } => args.iter().map(|(_, e)| e).collect(),
            Op::Shop { stock, .. } => stock.iter().filter_map(|s| s.cond.as_ref()).collect(),
            Op::Choice { options } => options
                .iter()
                .flat_map(|o| o.cond.iter().chain(o.effects.iter().map(|e| &e.value)))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Every asset file the op needs: [`Op::asset`], or a hotspot block's images.
    pub fn all_assets(&self) -> Vec<(AssetKind, &str)> {
        match self {
            Op::Choice { options } => options
                .iter()
                .filter_map(|o| o.area.as_ref()?.image.as_deref())
                .map(|i| (AssetKind::Image, i))
                .collect(),
            Op::Music { src, intro, .. } => {
                let intro = intro.iter().map(|i| (AssetKind::Audio, i.as_str()));
                std::iter::once((AssetKind::Audio, src.as_str())).chain(intro).collect()
            }
            _ => self.asset().into_iter().collect(),
        }
    }

    /// The asset file this op needs, if any.
    pub fn asset(&self) -> Option<(AssetKind, &str)> {
        match self {
            Op::Image { src, .. } => Some((AssetKind::Image, src)),
            Op::Bg { src, .. } => Some((kind_from_path(src), src)),
            Op::Show { src, .. } => Some((AssetKind::Image, src)),
            Op::Music { src, .. } | Op::Sfx { src, .. } | Op::SfxLoop { src, .. } | Op::Voice { src } => {
                Some((AssetKind::Audio, src))
            }
            Op::Video { src, .. } => Some((AssetKind::Video, src)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::is_inside;

    #[test]
    fn only_paths_inside_the_game_folder_are_inside() {
        assert!(is_inside("assets/bg.png") && is_inside("./theme.txt") && is_inside("story.quill.md"));
        assert!(!is_inside("../shared/bg.png") && !is_inside("assets/../../x.png"));
        assert!(!is_inside("/home/me/bg.png") && !is_inside(""));
        assert!(!is_inside("assets\\..\\..\\x.png") && !is_inside("assets\\bg.png"), "backslashes, on any system");
        assert!(!is_inside("C:\\art\\bg.png") && !is_inside("C:/x.png") && !is_inside("C:x.png"), "drives, on any system");
        assert!(!is_inside("//host/share/x.png") && !is_inside("\\\\host\\share\\x.png"), "UNC, on any system");
    }

    #[test]
    fn images_ahead_follow_every_branch_nearest_first() {
        let p = crate::parse(
            "::bg{src=a.png}\nHi.\n::choice\n- [Left](#left)\n- [Right](#right)\n::\n# left\n::bg{src=l.png}\nL.\n::end\n# right\n::show{x src=r.png}\nR.\n::bg{src=far.png}\n",
        )
        .unwrap();
        assert_eq!(p.images_ahead(0, 100), ["a.png", "l.png", "r.png", "far.png"]);
        assert!(!p.images_ahead(0, 6).contains(&"far.png"), "only so far ahead");
        assert_eq!(p.images_ahead(p.labels["left"], 100), ["l.png"], "the past isn't loaded");
    }

    #[test]
    fn easing_curves_start_at_0_end_at_1_and_keep_their_shape() {
        for e in [Ease::Linear, Ease::In, Ease::Out, Ease::InOut] {
            assert_eq!(e.apply(0.0), 0.0);
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6);
            assert_eq!(e.apply(-1.0), 0.0);
            assert_eq!(Ease::parse(e.name()), Some(e));
        }
        assert!(Ease::In.apply(0.5) < 0.5 && Ease::Out.apply(0.5) > 0.5);
        assert!((Ease::InOut.apply(0.5) - 0.5).abs() < 1e-6);
        assert!(Ease::InOut.apply(0.1) < 0.1);
    }

    use super::*;

    fn vars(pairs: &[(&str, Value)]) -> Vars {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    fn lit(n: i64) -> Box<Expr> {
        Box::new(Expr::Lit(Value::Int(n)))
    }

    fn var(n: &str) -> Box<Expr> {
        Box::new(Expr::Var(n.into()))
    }

    #[test]
    fn truthiness() {
        assert!(Value::Int(2).truthy() && !Value::Int(0).truthy());
        assert!(Value::Bool(true).truthy() && !Value::Bool(false).truthy());
        assert!(Value::Str("x".into()).truthy() && !Value::Str(String::new()).truthy());
    }

    #[test]
    fn unset_variables_read_as_zero() {
        assert_eq!(Expr::Var("nope".into()).eval(&Vars::new()), Value::Int(0));
        assert!(!Expr::Var("nope".into()).test(&Vars::new()));
    }

    #[test]
    fn comparisons_between_numbers_bools_and_text() {
        let v = vars(&[("n", Value::Int(3)), ("b", Value::Bool(true)), ("s", Value::Str("hi".into()))]);
        assert!(Expr::Cmp(var("n"), Cmp::Gt, lit(2)).test(&v));
        assert!(Expr::Cmp(var("b"), Cmp::Eq, lit(1)).test(&v)); // true == 1
        assert!(Expr::Cmp(var("s"), Cmp::Eq, Box::new(Expr::Lit(Value::Str("hi".into())))).test(&v));
        assert!(Expr::Cmp(var("s"), Cmp::Lt, Box::new(Expr::Lit(Value::Str("z".into())))).test(&v));
        // text vs number: only != holds
        assert!(!Expr::Cmp(var("s"), Cmp::Eq, lit(1)).test(&v));
        assert!(Expr::Cmp(var("s"), Cmp::Ne, lit(1)).test(&v));
        assert!(!Expr::Cmp(var("s"), Cmp::Gt, lit(1)).test(&v));
    }

    #[test]
    fn logic_operators_short_circuit_to_booleans() {
        let v = vars(&[("a", Value::Int(1)), ("b", Value::Int(0))]);
        assert!(Expr::Or(var("a"), var("b")).test(&v));
        assert!(!Expr::And(var("a"), var("b")).test(&v));
        assert!(Expr::Not(var("b")).test(&v));
        assert_eq!(Expr::And(var("a"), var("a")).eval(&v), Value::Bool(true));
    }

    fn effect(var: &str, op: AssignOp, value: Value) -> Effect {
        Effect { var: var.into(), op, value: Expr::Lit(value) }
    }

    #[test]
    fn numeric_effects() {
        let mut v = Vars::new();
        effect("x", AssignOp::Add, Value::Int(2)).apply(&mut v);
        effect("x", AssignOp::Add, Value::Int(3)).apply(&mut v);
        effect("x", AssignOp::Sub, Value::Int(1)).apply(&mut v);
        assert_eq!(v["x"], Value::Int(4));
        effect("x", AssignOp::Set, Value::Bool(true)).apply(&mut v);
        assert_eq!(v["x"], Value::Bool(true));
    }

    #[test]
    fn text_effects_append_and_ignore_nonsense() {
        let mut v = Vars::new();
        effect("s", AssignOp::Add, Value::Str("ab".into())).apply(&mut v);
        effect("s", AssignOp::Add, Value::Str("cd".into())).apply(&mut v);
        effect("s", AssignOp::Add, Value::Int(1)).apply(&mut v);
        assert_eq!(v["s"], Value::Str("abcd1".into()));
        // subtracting from text does nothing
        effect("s", AssignOp::Sub, Value::Int(1)).apply(&mut v);
        assert_eq!(v["s"], Value::Str("abcd1".into()));
    }

    #[test]
    fn effects_can_read_other_variables() {
        let mut v = vars(&[("price", Value::Int(5)), ("gold", Value::Int(20))]);
        Effect { var: "gold".into(), op: AssignOp::Sub, value: Expr::Var("price".into()) }.apply(&mut v);
        assert_eq!(v["gold"], Value::Int(15));
    }

    #[test]
    fn asset_kinds_follow_the_extension() {
        let bg_video = Op::Bg { src: "loop.webm".into(), fade: 0.0, wipe: None };
        assert_eq!(bg_video.asset(), Some((AssetKind::Video, "loop.webm")));
        let show = Op::Show { id: "a".into(), src: "a.png".into(), at: None, y: None, z: None, scale: None, flip: None, xscale: None, yscale: None, fade: 0.0 };
        assert_eq!(show.asset(), Some((AssetKind::Image, "a.png")));
        assert_eq!(Op::Voice { src: "v.ogg".into() }.asset(), Some((AssetKind::Audio, "v.ogg")));
        assert_eq!(Op::Return.asset(), None);
    }

    // ---- list variables ----

    fn list(items: &[&str]) -> Value {
        Value::List(items.iter().map(|s| s.to_string()).collect())
    }

    fn text(s: &str) -> Expr {
        Expr::Lit(Value::Str(s.into()))
    }

    fn change(v: &mut Vars, var: &str, op: AssignOp, value: Expr) {
        Effect { var: var.into(), op, value }.apply(v);
    }

    #[test]
    fn lists_are_sets_that_keep_their_order() {
        let mut v = vars(&[("inv", list(&[]))]);
        for item in ["map", "torch", "map", "key", "torch"] {
            change(&mut v, "inv", AssignOp::Add, text(item));
        }
        assert_eq!(v["inv"], list(&["map", "torch", "key"]));
        change(&mut v, "inv", AssignOp::Sub, text("torch"));
        change(&mut v, "inv", AssignOp::Sub, text("absent"));
        assert_eq!(v["inv"], list(&["map", "key"]));
        change(&mut v, "inv", AssignOp::Add, text("torch"));
        assert_eq!(v["inv"], list(&["map", "key", "torch"]), "re-added at the end");
        assert_eq!(v["inv"].to_string(), "map, key, torch");
        assert_eq!(list(&[]).to_string(), "");
        assert!(v["inv"].truthy() && !list(&[]).truthy());
    }

    #[test]
    fn a_list_ignores_everything_but_adding_and_removing_text() {
        let mut v = vars(&[("inv", list(&["a"])), ("n", Value::Int(2)), ("t", Value::Str("b".into()))]);
        change(&mut v, "inv", AssignOp::Set, text("x"));
        change(&mut v, "inv", AssignOp::Set, Expr::Lit(Value::Int(0)));
        change(&mut v, "inv", AssignOp::Add, Expr::Lit(Value::Int(1)));
        change(&mut v, "inv", AssignOp::Add, Expr::Var("n".into()));
        change(&mut v, "inv", AssignOp::Sub, Expr::Lit(Value::Int(1)));
        change(&mut v, "inv", AssignOp::Add, text(""));
        assert_eq!(v["inv"], list(&["a"]));
        change(&mut v, "inv", AssignOp::Add, Expr::Var("t".into()));
        assert_eq!(v["inv"], list(&["a", "b"]), "a text variable works");
    }

    #[test]
    fn lists_never_leak_into_other_variables() {
        let mut v = vars(&[("inv", list(&["a"])), ("n", Value::Int(2)), ("t", Value::Str("x".into()))]);
        change(&mut v, "copy", AssignOp::Set, Expr::Var("inv".into()));
        change(&mut v, "n", AssignOp::Add, Expr::Var("inv".into()));
        change(&mut v, "t", AssignOp::Add, Expr::Var("inv".into()));
        assert!(!v.contains_key("copy"));
        assert_eq!((&v["n"], &v["t"]), (&Value::Int(2), &Value::Str("x".into())));
    }

    #[test]
    fn lists_compare_as_sets() {
        let v = vars(&[("a", list(&["x", "y"])), ("b", list(&["y", "x"])), ("c", list(&["x"])), ("n", Value::Int(0))]);
        let cmp = |l: &str, op, r: &str| Expr::Cmp(var(l), op, var(r)).test(&v);
        assert!(cmp("a", Cmp::Eq, "b") && !cmp("a", Cmp::Ne, "b"));
        assert!(!cmp("a", Cmp::Eq, "c") && cmp("a", Cmp::Ne, "c"));
        assert!(!cmp("a", Cmp::Lt, "c") && !cmp("a", Cmp::Ge, "c") && !cmp("a", Cmp::Le, "b"), "no ordering");
        assert!(!cmp("a", Cmp::Eq, "n") && cmp("a", Cmp::Ne, "n"));
    }
}
