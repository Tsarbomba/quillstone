//! The compiled form of a Quillstone script.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fmt;
use std::path::{Component, Path};

use crate::dynpath::is_device_name;

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

/// The most characters `::input` accepts by default, and the most it may be told to accept.
pub const INPUT_MAX_DEFAULT: usize = 24;
pub const INPUT_MAX_LIMIT: usize = 64;

/// Most items a list variable holds: `+=` stops adding there, and a save with a longer list is refused
/// (real games hold dozens; the limit keeps a damaged or hostile file from costing memory or time).
pub const MAX_LIST_ITEMS: usize = 10_000;

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
    /// Whole-number arithmetic: `a + b`, `a * 2`, `a / b`, `a % b`.
    Bin(Box<Expr>, BinOp, Box<Expr>),
    /// `-x`.
    Neg(Box<Expr>),
    /// `min(a, b, ...)`, `max(a, b, ...)`, `abs(x)`.
    Func(Func, Vec<Expr>),
    /// `count(list)`: how many items the list variable holds.
    Count(String),
    /// `random(lo, hi)`: a whole number from `lo` to `hi`, both included. Only the
    /// parser's assignment sites let it through (never a condition), so rebuilding a
    /// screen can't re-roll it.
    Random(Box<Expr>, Box<Expr>),
}

/// The whole-number operators, loosest first: `+ -`, then `* / %`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    /// Truncates toward zero; dividing by 0 gives 0.
    Div,
    /// The remainder, with the sign of the left side; modulo 0 gives 0.
    Rem,
}

impl BinOp {
    pub fn symbol(self) -> char {
        match self {
            BinOp::Add => '+',
            BinOp::Sub => '-',
            BinOp::Mul => '*',
            BinOp::Div => '/',
            BinOp::Rem => '%',
        }
    }

    /// Saturating at the ends of the integer range; never panics.
    pub fn apply(self, a: i64, b: i64) -> i64 {
        match self {
            BinOp::Add => a.saturating_add(b),
            BinOp::Sub => a.saturating_sub(b),
            BinOp::Mul => a.saturating_mul(b),
            BinOp::Div if b == 0 => 0,
            BinOp::Div => a.saturating_div(b),
            // `i64::MIN % -1` overflows in Rust, and is 0 in mathematics
            BinOp::Rem => a.checked_rem(b).unwrap_or(0),
        }
    }
}

/// The functions that take any number of values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Min,
    Max,
    Abs,
}

/// The functions of the language, for error messages.
pub const FUNCTIONS: &str = "min(a, b, ...), max(a, b, ...), abs(x), count(list), random(lo, hi)";

/// The generator behind `random(lo, hi)`: splitmix64, one step per call. The seed
/// lives in the story state, so rollback and saves replay the same rolls.
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The seed of a game whose host gave none (tests, the command line, old saves).
pub const DEFAULT_SEED: u64 = 0x5EED_C0DE_5EED_C0DE;

/// A whole number from `lo` to `hi` (both included; swapped if `lo > hi`), advancing `state`.
pub fn roll(state: &mut u64, lo: i64, hi: i64) -> i64 {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let span = (hi as i128 - lo as i128 + 1) as u128;
    let r = splitmix64(state) as u128 % span;
    (lo as i128 + r as i128) as i64
}

fn int(v: &Value) -> i64 {
    v.as_int().unwrap_or(0)
}

/// A condition is just an expression that is tested for truthiness.
pub type Cond = Expr;

impl Expr {
    /// The value, with `random()` giving its lower bound (the parser keeps it out of
    /// everything that is evaluated this way).
    pub fn eval(&self, vars: &Vars) -> Value {
        self.eval_with(vars, &mut |lo, _| lo)
    }

    /// The value; `draw(lo, hi)` supplies each `random(lo, hi)` (with `lo <= hi`).
    /// A text, a list or an unset variable in arithmetic counts as 0.
    pub fn eval_with(&self, vars: &Vars, draw: &mut dyn FnMut(i64, i64) -> i64) -> Value {
        match self {
            Expr::Lit(v) => v.clone(),
            Expr::Var(name) => vars.get(name).cloned().unwrap_or(Value::Int(0)),
            Expr::Not(e) => Value::Bool(!e.eval_with(vars, draw).truthy()),
            Expr::And(a, b) => Value::Bool(a.eval_with(vars, draw).truthy() && b.eval_with(vars, draw).truthy()),
            Expr::Or(a, b) => Value::Bool(a.eval_with(vars, draw).truthy() || b.eval_with(vars, draw).truthy()),
            Expr::Cmp(a, op, b) => {
                let a = a.eval_with(vars, draw);
                Value::Bool(compare(&a, *op, &b.eval_with(vars, draw)))
            }
            Expr::Has(name, item) => Value::Bool(match (vars.get(name), item.eval_with(vars, draw)) {
                (Some(Value::List(items)), Value::Str(s)) => items.contains(&s),
                _ => false,
            }),
            Expr::Bin(a, op, b) => {
                let a = int(&a.eval_with(vars, draw));
                Value::Int(op.apply(a, int(&b.eval_with(vars, draw))))
            }
            Expr::Neg(e) => Value::Int(int(&e.eval_with(vars, draw)).saturating_neg()),
            Expr::Func(f, args) => {
                let mut nums = args.iter().map(|a| int(&a.eval_with(vars, draw))).collect::<Vec<_>>().into_iter();
                let first = nums.next().unwrap_or(0);
                Value::Int(match f {
                    Func::Min => nums.fold(first, i64::min),
                    Func::Max => nums.fold(first, i64::max),
                    Func::Abs => first.saturating_abs(),
                })
            }
            Expr::Count(name) => Value::Int(match vars.get(name) {
                Some(Value::List(items)) => items.len() as i64,
                _ => 0,
            }),
            Expr::Random(lo, hi) => {
                let lo = int(&lo.eval_with(vars, draw));
                let hi = int(&hi.eval_with(vars, draw));
                Value::Int(if lo <= hi { draw(lo, hi) } else { draw(hi, lo) })
            }
        }
    }

    pub fn test(&self, vars: &Vars) -> bool {
        self.eval(vars).truthy()
    }

    /// Every variable the expression reads, in order (with repeats).
    pub fn vars_read<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Expr::Lit(_) => {}
            Expr::Var(name) | Expr::Count(name) => out.push(name),
            Expr::Not(e) | Expr::Neg(e) => e.vars_read(out),
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Cmp(a, _, b) | Expr::Bin(a, _, b) | Expr::Random(a, b) => {
                a.vars_read(out);
                b.vars_read(out);
            }
            Expr::Has(name, item) => {
                out.push(name);
                item.vars_read(out);
            }
            Expr::Func(_, args) => args.iter().for_each(|a| a.vars_read(out)),
        }
    }

    /// The expressions directly inside this one.
    pub fn children(&self) -> Vec<&Expr> {
        match self {
            Expr::Lit(_) | Expr::Var(_) | Expr::Count(_) => Vec::new(),
            Expr::Not(e) | Expr::Neg(e) | Expr::Has(_, e) => vec![e],
            Expr::And(a, b) | Expr::Or(a, b) | Expr::Cmp(a, _, b) | Expr::Bin(a, _, b) | Expr::Random(a, b) => vec![a, b],
            Expr::Func(_, args) => args.iter().collect(),
        }
    }

    /// Whether a `random(lo, hi)` is anywhere inside.
    pub fn has_random(&self) -> bool {
        matches!(self, Expr::Random(..)) || self.children().into_iter().any(Expr::has_random)
    }

    /// Every `list has item` test inside the expression, as `(list variable, item expression)`.
    pub fn has_tests<'a>(&'a self, out: &mut Vec<(&'a str, &'a Expr)>) {
        if let Expr::Has(name, item) = self {
            out.push((name, item));
        }
        for c in self.children() {
            c.has_tests(out);
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
        self.apply_with(vars, &mut |lo, _| lo);
    }

    /// [`Effect::apply`], with `draw(lo, hi)` supplying each `random(lo, hi)`.
    pub fn apply_with(&self, vars: &mut Vars, draw: &mut dyn FnMut(i64, i64) -> i64) {
        let rhs = self.value.eval_with(vars, draw);
        // A list changes only by `+=` / `-=` of text; anything else leaves it alone.
        if let Some(Value::List(items)) = vars.get_mut(&self.var) {
            match (self.op, rhs) {
                (AssignOp::Add, Value::Str(s)) if !s.is_empty() && items.len() < MAX_LIST_ITEMS && !items.contains(&s) => {
                    items.push(s);
                }
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
    /// Layer ids of a layered sprite, back to front (empty: no layers).
    pub layers: Vec<String>,
    /// Layer picture template: `{layer}` is a layer id, `{value}` the value shown on it.
    pub layer_sprites: Option<String>,
}

/// The most layers one character may have.
pub const MAX_LAYERS: usize = 8;

/// The layer value that hides a layer (`::show{mika eyes=none}`).
pub const LAYER_NONE: &str = "none";

impl Character {
    /// The picture of layer `layer` showing `value` (a pattern when `value` has `{variable}` parts).
    pub fn layer_path(&self, layer: &str, value: &str) -> Option<String> {
        let template = self.layer_sprites.as_deref()?;
        Some(template.replace("{layer}", layer).replace("{value}", value))
    }
}

/// One picture of a frame animation and how long it stays up.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimFrame {
    pub file: String,
    pub seconds: f32,
}

/// A frame animation declared with `::anim`. Shown like a picture (`anim=id`); the
/// runtime and the view refer to it as the `src` text `anim:<id>` (see [`anim_id`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Anim {
    pub id: String,
    pub frames: Vec<AnimFrame>,
    /// Starts over after the last frame (otherwise the last frame stays).
    pub looped: bool,
    /// Source line (1-based) of the declaration.
    pub line: usize,
}

/// The shortest and longest time one frame may be shown, in seconds.
pub const ANIM_FRAME_SECONDS: std::ops::RangeInclusive<f32> = 0.02..=60.0;

impl Anim {
    /// How long one run through the frames takes.
    pub fn total(&self) -> f32 {
        self.frames.iter().map(|f| f.seconds).sum()
    }

    /// Whether the picture ever changes (more than one frame).
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// Whether, `t` seconds after it started, the picture can still change.
    pub fn running(&self, t: f32) -> bool {
        self.is_animated() && (self.looped || t < self.total())
    }

    /// The index of the frame shown `t` seconds after the animation started. A
    /// frame's last instant belongs to the next frame; a looping animation wraps,
    /// a one-shot one stays on its last frame.
    pub fn frame_index(&self, t: f32) -> usize {
        let total = self.total();
        if self.frames.len() < 2 || !t.is_finite() || t <= 0.0 || total <= 0.0 {
            return 0;
        }
        let t = if self.looped {
            t % total
        } else if t >= total {
            return self.frames.len() - 1;
        } else {
            t
        };
        let mut end = 0.0;
        for (i, f) in self.frames.iter().enumerate() {
            end += f.seconds;
            if t < end {
                return i;
            }
        }
        self.frames.len() - 1 // rounding at the very end of a lap
    }

    /// The picture file shown `t` seconds after the animation started.
    pub fn frame_at(&self, t: f32) -> &str {
        self.frames.get(self.frame_index(t)).map_or("", |f| f.file.as_str())
    }
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
    /// An int variable holding how many the player has (`count=`): the item is then
    /// shown while it is above 0, whatever the inventory list holds.
    pub count: Option<String>,
    /// The label the inventory's Use button calls (`use=#label`).
    pub use_label: Option<String>,
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

/// One entry of the Extras "Scenes" tab (front matter `replays:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    /// A label without parameters (its slug, as `::jump` writes it).
    pub label: String,
    /// What the menu shows once it is unlocked.
    pub title: String,
}

/// Most scenes a game may list for replay.
pub const MAX_REPLAYS: usize = 64;

/// A fully compiled script: a flat list of ops with all jumps resolved to indices.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    pub title: Option<String>,
    /// The `id:` front matter key: the name of the player's save folder (see `title`, the fallback).
    pub id: Option<String>,
    /// The theme file named in the front matter (`theme: theme.txt`), relative to the script.
    pub theme: Option<String>,
    /// Initial variable values from the front matter.
    pub vars: Vars,
    pub characters: BTreeMap<String, Character>,
    /// Frame animations declared with `::anim`, by id.
    pub anims: BTreeMap<String, Anim>,
    /// Items declared with `::item`, by id.
    pub items: BTreeMap<String, Item>,
    /// The list variable the inventory screen shows (front matter `inventory:`).
    pub inventory: Option<String>,
    /// The list variable the journal shows (front matter `journal:`): the same screen as the
    /// inventory, fed by a second list of `::item` entries (quests), without Use, prices or counts.
    pub journal: Option<String>,
    /// Front matter `journal_done:`: a list whose entries the journal shows after the open ones, greyed
    /// and marked Done. Only with a `journal:`.
    pub journal_done: Option<String>,
    /// Front matter `sprite_size: pixels`: sprites are drawn at their picture's pixel size times this
    /// (`art_scale:`, default 1). `None`: every sprite is the same fixed height.
    pub art_scale: Option<f32>,
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
    /// Gallery entry -> the picture its unlocked tile shows instead of the entry itself (`a.webm | t.png`).
    pub gallery_thumbs: BTreeMap<String, String>,
    /// Tracks for the extras music room, unlocked when the story plays them.
    pub music_room: Vec<String>,
    /// Scenes the player can replay from Extras (front matter `replays:`), in
    /// declaration order; each unlocks the first time the story reaches its label.
    pub replays: Vec<Replay>,
    /// Op index -> the `replays` labels that start there (the unlock points).
    pub replay_at: BTreeMap<usize, Vec<String>>,
    /// Auto-voice: the folder of voice files named after their lines (front
    /// matter `voice:`), relative to the script. See `voice_name` in the runtime.
    pub voice_folder: Option<String>,
    /// Files pulled in with `::include`, in the order they were read. The main script's
    /// lines are numbered as written; each included file's lines continue after it (see
    /// [`SourceSpan`]), so `lines` and every other line number stay plain numbers.
    /// Use [`Program::locate`] to turn one back into a file and a line.
    pub sources: Vec<SourceSpan>,
    /// The languages the game is offered in (front matter `languages:`). The first is the one
    /// the script is written in; the others are translated from `lang/<code>.txt`. Empty: one language.
    pub languages: Vec<Language>,
}

/// A language the game is offered in (see [`Program::languages`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language {
    /// A safe file-name fragment: the translation file is `lang/<code>.txt`.
    pub code: String,
    /// What the Settings screen shows, in that language itself.
    pub name: String,
}

/// Most languages a game may list.
pub const MAX_LANGUAGES: usize = 16;

/// Where an included file's lines sit in a [`Program`]'s line numbering: its line `n` is
/// numbered `base + n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    /// The path as written in `::include`, relative to the game folder.
    pub file: String,
    pub base: usize,
    /// How many lines the file has.
    pub len: usize,
}

impl Program {
    /// The file and line a program line number stands for: `(None, line)` for the main
    /// script, `(Some(path), line)` for an included file.
    pub fn locate(&self, line: usize) -> (Option<&str>, usize) {
        match self.sources.iter().find(|s| line > s.base && line <= s.base + s.len) {
            Some(s) => (Some(s.file.as_str()), line - s.base),
            None => (None, line),
        }
    }

    /// `path:line` for a program line number, where `main` is how the caller names the
    /// main script and included files keep their game-folder paths.
    pub fn place(&self, main: &str, line: usize) -> String {
        match self.locate(line) {
            (Some(file), n) => format!("{file}:{n}"),
            (None, n) => format!("{main}:{n}"),
        }
    }

    /// The included files (game-folder paths), for a hot reload to watch and a package to ship.
    pub fn included_files(&self) -> impl Iterator<Item = &str> {
        self.sources.iter().map(|s| s.file.as_str())
    }

    /// Files named in the front matter (gallery pictures and thumbnails, music room tracks).
    pub fn extra_assets(&self) -> impl Iterator<Item = (AssetKind, &str)> {
        let gallery = self.gallery.iter().map(|p| (kind_from_path(p), p.as_str()));
        let thumbs = self.gallery_thumbs.values().map(|p| (AssetKind::Image, p.as_str()));
        gallery.chain(thumbs).chain(self.music_room.iter().map(|p| (AssetKind::Audio, p.as_str())))
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
                Op::Choice { options, .. } => queue.extend(options.iter().map(|o| o.target)),
                Op::Return | Op::End => {}
                _ => queue.push_back(at + 1),
            }
        }
        out
    }

    /// The images the story may show within `steps` ops of `pc`, nearest first,
    /// following jumps, calls and every choice (for loading them before they're needed).
    /// Picture names with `{variable}` parts are left out: they load when shown.
    pub fn images_ahead(&self, pc: usize, steps: usize) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for at in self.ops_ahead(pc, steps) {
            let op = &self.ops[at];
            let anims: Vec<&Anim> = match op {
                Op::Bg { src, .. } => anim_id(src).and_then(|id| self.anims.get(id)).into_iter().collect(),
                Op::Show { src, layers, .. } => std::iter::once(src.as_str())
                    .chain(layers.iter().map(|(_, l)| l.as_str()))
                    .filter_map(|s| anim_id(s).and_then(|id| self.anims.get(id)))
                    .collect(),
                _ => Vec::new(),
            };
            let frames = anims.into_iter().flat_map(|a| a.frames.iter().map(|f| (AssetKind::Image, f.file.as_str())));
            for (kind, src) in op.all_assets().into_iter().chain(frames) {
                // A name with `{variable}` parts is built when it is shown: nothing to load ahead.
                if kind == AssetKind::Image && !crate::is_pattern(src) && !out.contains(&src) {
                    out.push(src);
                }
            }
        }
        out
    }

    /// Whether the story may open the inventory or the journal, and which item icons the shops
    /// it may open sell, within `steps` ops of `pc` (see [`Program::ops_ahead`]).
    pub fn bag_icons_ahead(&self, pc: usize, steps: usize) -> (bool, bool, Vec<&str>) {
        let (mut inventory, mut journal, mut icons) = (false, false, Vec::new());
        for at in self.ops_ahead(pc, steps) {
            match &self.ops[at] {
                Op::Inventory => inventory = true,
                Op::Journal => journal = true,
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
        (inventory, journal, icons)
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
                Op::Choice { options, .. } => options
                    .iter()
                    .filter_map(|o| Some((o.line, AssetKind::Image, o.area.as_ref()?.image.as_deref()?)))
                    .collect(),
                _ => op.all_assets().into_iter().map(|(kind, path)| (op_line, kind, path)).collect(),
            };
            found
        })
    }

    /// The most variable names the story can make beyond the declared ones: every `::set` and choice
    /// effect target (`{once}` flags included), `::input` variable, `::call` parameter and counted
    /// `seen:` label. A save may not hold more (plus slack) before the reader refuses it.
    pub fn creatable_vars(&self) -> usize {
        let from_ops: usize = self
            .ops
            .iter()
            .map(|op| match op {
                Op::Set(_) | Op::Input { .. } => 1,
                Op::Choice { options, .. } => options.iter().map(|o| o.effects.len()).sum(),
                Op::Call { args, .. } => args.len(),
                _ => 0,
            })
            .sum();
        from_ops + self.counted.values().map(Vec::len).sum::<usize>()
    }

    /// Literal item texts that name no `::item`, where the inventory or journal lists are
    /// changed with `+=` or tested with `has`: `(op index, text)`. Empty when the
    /// game has no such list or declares no items.
    pub fn unknown_items(&self) -> Vec<(usize, String)> {
        let lists: Vec<&String> = [&self.inventory, &self.journal, &self.journal_done].into_iter().flatten().collect();
        if self.items.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (at, op) in self.ops.iter().enumerate() {
            let effects: Vec<&Effect> = match op {
                Op::Set(e) => vec![e],
                Op::Choice { options, .. } => options.iter().flat_map(|o| &o.effects).collect(),
                _ => Vec::new(),
            };
            let mut texts: Vec<&str> = effects
                .into_iter()
                .filter(|e| lists.contains(&&e.var) && e.op != AssignOp::Set)
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
                if let (true, Expr::Lit(Value::Str(s))) = (lists.iter().any(|l| l.as_str() == name), item) {
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
        let ops = self
            .ops
            .iter()
            .enumerate()
            .flat_map(|(i, op)| op.all_assets().into_iter().map(move |(kind, path)| (i, kind, path)));
        // The pictures of every declared animation (op index 0: they belong to no op).
        ops.chain(self.anim_frames().map(|(_, _, file)| (0, AssetKind::Image, file)))
    }

    /// The picture files of every `::anim` frame, as `(declaration line, animation id, file)`.
    pub fn anim_frames(&self) -> impl Iterator<Item = (usize, &str, &str)> {
        self.anims.values().flat_map(|a| a.frames.iter().map(move |f| (a.line, a.id.as_str(), f.file.as_str())))
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

/// Most particles `::particles` draws at once.
pub const MAX_PARTICLES: u32 = 200;
/// How many it draws when `amount=` is left out.
pub const DEFAULT_PARTICLES: u32 = 60;
/// Allowed `speed=` and `wind=` of `::particles`.
pub const PARTICLE_SPEED: std::ops::RangeInclusive<f32> = 0.2..=5.0;
pub const PARTICLE_WIND: std::ops::RangeInclusive<f32> = -2.0..=2.0;

/// Allowed `alpha=` of `::tint` (the story must stay visible), and its default.
pub const TINT_ALPHA: std::ops::RangeInclusive<f32> = 0.0..=0.9;
pub const DEFAULT_TINT_ALPHA: f32 = 0.4;

/// The colour wash over the stage (`::tint`): an RGB colour (0..1) at `alpha` opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint {
    pub color: [f32; 3],
    pub alpha: f32,
}

/// What falls in `::particles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParticleKind {
    Snow,
    Rain,
    /// A picture file (inside the game folder), falling and drifting like snow.
    Image(String),
}

/// The settings of the ambient particles (`::particles`). Where each particle is
/// is never stored: the view works it out from the time.
#[derive(Debug, Clone, PartialEq)]
pub struct Particles {
    pub kind: ParticleKind,
    /// How many are drawn, 1 to [`MAX_PARTICLES`].
    pub amount: u32,
    /// Fall speed multiplier (see [`PARTICLE_SPEED`]).
    pub speed: f32,
    /// Sideways push, negative to the left (see [`PARTICLE_WIND`]).
    pub wind: f32,
    /// Drawn between the background and the sprites instead of over them.
    pub back: bool,
}

impl Particles {
    pub fn new(kind: ParticleKind) -> Particles {
        Particles { kind, amount: DEFAULT_PARTICLES, speed: 1.0, wind: 0.0, back: false }
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

/// Prefix of a picture (background or sprite) that is a frame animation: `anim:<id>`.
/// (`:` can't be in a game-folder path, so it never collides with a file.)
pub const ANIM_PREFIX: &str = "anim:";

/// The `src` that names the animation `id`.
pub fn anim_src(id: &str) -> String {
    format!("{ANIM_PREFIX}{id}")
}

/// The animation a picture `src` names, or `None` for a file or a colour.
pub fn anim_id(src: &str) -> Option<&str> {
    src.strip_prefix(ANIM_PREFIX).filter(|id| crate::is_plain_ident(id))
}

/// Prefix of a background that is a solid colour instead of a file: `color:#rrggbb`.
/// (`:` can't be in a game-folder path, see [`is_inside`], so it never collides with a file.)
pub const BG_COLOR_PREFIX: &str = "color:";

/// The `src` of a solid-colour background (quantised to 8 bits per channel).
pub fn bg_color_src(c: [f32; 3]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{BG_COLOR_PREFIX}#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

/// The colour of a solid-colour background `src`, or `None` for a file.
pub fn bg_color(src: &str) -> Option<[f32; 3]> {
    let hex = src.strip_prefix(BG_COLOR_PREFIX)?.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map(|v| v as f32 / 255.0).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// How far outside the screen `x=` / `y=` of a sprite may go (a fraction of the screen, so -200% to 300%). Past
/// 0..1 the sprite moves by whole screens, so `x=-100%` and `x=200%` are fully off the screen for any sprite no
/// bigger than it; not enough to lose a sprite for good.
pub const POSITION: std::ops::RangeInclusive<f32> = -2.0..=3.0;

/// An expression kept with the text it was written as (saves store the text).
#[derive(Debug, Clone, PartialEq)]
pub struct HudExpr {
    pub src: String,
    pub expr: Expr,
}

impl HudExpr {
    /// Parse `src` as an expression (`random` is refused, as in every condition).
    pub fn parse(src: &str) -> Result<HudExpr, String> {
        let src = src.trim();
        Ok(HudExpr { src: src.to_string(), expr: crate::parse_expr(src)? })
    }
}

/// What a HUD element is (see [`HudElem`]).
#[derive(Debug, Clone, PartialEq)]
pub enum HudKind {
    /// Tagged text with `{name}` values filled in when it is drawn. `size` and `color`
    /// default to the theme's; `back` is a panel behind the text, `pad` its padding.
    Text { text: String, size: Option<f32>, color: Option<[f32; 4]>, back: Option<[f32; 4]>, pad: f32 },
    /// A bar filled `value / max` of the way, left to right. `border` is a colour and a width.
    Bar { value: HudExpr, max: HudExpr, size: [f32; 2], color: Option<[f32; 4]>, back: [f32; 4], border: Option<([f32; 4], f32)> },
    /// A picture inside the game folder, drawn at its own size times `scale`.
    Image { src: String, scale: f32 },
}

/// One always-on overlay element (`::hud`). `id` is its key: a new element with the
/// same id replaces it in place. `x` and `y` place it as `::show` does (0..1: the point that far across the element
/// is that far across the screen).
#[derive(Debug, Clone, PartialEq)]
pub struct HudElem {
    pub id: String,
    pub x: f32,
    pub y: f32,
    /// Drawn only while this is true (checked every time it is drawn).
    pub cond: Option<HudExpr>,
    pub kind: HudKind,
}

impl HudElem {
    /// Every expression the element reads when it is drawn.
    pub fn exprs(&self) -> Vec<&Expr> {
        let mut out: Vec<&Expr> = self.cond.iter().map(|c| &c.expr).collect();
        if let HudKind::Bar { value, max, .. } = &self.kind {
            out.push(&value.expr);
            out.push(&max.expr);
        }
        out
    }

    /// The picture file, for an image element.
    pub fn image(&self) -> Option<&str> {
        match &self.kind {
            HudKind::Image { src, .. } => Some(src),
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
/// So are the names Windows keeps for devices (`con`, `nul`, `com1`...): opening one never reads a file.
pub fn is_inside(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && p.components().all(|c| match c {
            Component::Normal(n) => !is_device_name(&n.to_string_lossy()),
            c => matches!(c, Component::CurDir),
        })
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

/// Shortest and longest `::choice{time=..}`, in seconds.
pub const MIN_CHOICE_TIME: f32 = 0.5;
pub const MAX_CHOICE_TIME: f32 = 600.0;

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
    /// Picked when a timed choice runs out (`{default}`); only in a `::choice{time=..}`.
    pub default: bool,
    /// `{show}` / `{show="reason"}`: while `cond` is false the option is drawn greyed instead of hidden.
    pub lock: Option<Lock>,
    /// Source line (1-based) of the option.
    pub line: usize,
}

/// How a locked option shows: `cond` of the option is what unlocks it, so it can't hide it too.
#[derive(Debug, Clone, PartialEq)]
pub struct Lock {
    /// Shown beside the option while it is locked; empty when the author gave none. Tagged text like the option's.
    pub reason: String,
    /// What still hides the option (a `{once}` option already picked); `None` = always shown.
    pub shown_if: Option<Cond>,
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
    /// Show (or change the pose of) a sprite. `x` and `y` place it (see [`POSITION`]): the point that
    /// fraction across (down) the sprite is that fraction across (down) the screen, so 0..1 keeps it on
    /// screen; beyond that it slides off by whole screens. `None` keeps the current position; a new sprite is centred on the bottom edge (0.5, 1.0).
    /// `z` orders sprites (-100 to 100, default 0; higher is in front). All three keep the current value when `None`.
    /// `flip` mirrors the picture left to right; `xscale` and `yscale` (0.05 to 10)
    /// stretch it along one axis, on top of `scale`. All three keep the sprite's
    /// current value when `None` (and are off / 1 for a new sprite).
    /// A layered sprite (`::show{mika body=casual eyes=open}`) has an empty `src` and
    /// the layers the command sets: (layer id, picture) in the character's layer order,
    /// where the picture is a file or pattern, `anim:<id>`, or empty to hide the layer.
    /// Layers the command leaves out keep their value.
    Show {
        id: String,
        src: String,
        layers: Vec<(String, String)>,
        x: Option<f32>,
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
        x: Option<f32>,
        y: Option<f32>,
        scale: Option<f32>,
        flip: Option<bool>,
        xscale: Option<f32>,
        yscale: Option<f32>,
        time: f32,
        ease: Ease,
        /// `loop`: after the move, go back and forth between the start and the end
        /// values until something else moves, shows or hides the sprite. `time` is one leg.
        looped: bool,
    },
    /// Shake the stage (background and sprites) for `time` seconds, by up to
    /// `strength` pixels (at 1920x1080) along `axis`. Doesn't wait.
    Shake { time: f32, strength: f32, axis: Axis },
    /// Pan and zoom the stage: `zoom` (1 = whole stage), the view placed by `x`, `y` (0..1) like a sprite's
    /// `x=` / `y=` (0 = left/top edge of the stage in view). `None` keeps the current value.
    Camera { zoom: Option<f32>, x: Option<f32>, y: Option<f32>, time: f32, ease: Ease },
    /// Start, change (`Some`) or stop (`None`) the ambient particles, easing over `fade`
    /// seconds. Doesn't wait.
    Particles { set: Option<Particles>, fade: f32 },
    /// Set (`Some`) or remove (`None`) the colour wash over the stage, easing over `fade`
    /// seconds from what it is now. Doesn't wait.
    Tint { set: Option<Tint>, fade: f32 },
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
    /// A small message in a corner of the screen for `time` seconds (`::notify`). Doesn't wait;
    /// `{var}` values are filled in when it runs.
    Notify { text: String, time: f32 },
    /// Unlock a gallery or music room entry without showing or playing it (`::unlock{src=...}`).
    Unlock { src: String },
    /// Fade the stage to `color` over `time` seconds and keep it covered. Waits
    /// until the fade is done (like a `::wait`).
    FadeOut { time: f32, color: [f32; 3] },
    /// Fade the cover away over `time` seconds. Doesn't wait.
    FadeIn { time: f32 },
    /// Create or replace a HUD element (`::hud`), fading in over `fade` seconds.
    Hud { elem: Box<HudElem>, fade: f32 },
    /// Remove one HUD element (`::hud{hide=id}`); an unknown id does nothing.
    HudHide { id: String, fade: f32 },
    /// Remove every HUD element (`::hud{clear}`).
    HudClear { fade: f32 },
    /// Voice clip for the next line.
    Voice { src: String },
    /// Pause for `seconds` (the player can skip it). Waits like a line does.
    Wait { seconds: f32 },
    /// Fullscreen video clip. Waits until it ends or the player skips it.
    Video { src: String, fit: Fit, looped: bool, skippable: bool },
    /// Waits for the player to pick an option (or, when the options have an
    /// `area`, to click a region of the screen). With a `time` (seconds, `MIN_CHOICE_TIME` to
    /// `MAX_CHOICE_TIME`) the frontend picks the `default` option when it runs out; the
    /// runtime itself never counts time.
    Choice { options: Vec<ChoiceOption>, time: Option<f32> },
    /// The inventory screen; waits until the player closes it.
    Inventory,
    /// The journal screen (`::journal`); waits until the player closes it.
    Journal,
    /// A shop: items for sale, paid with the int variable `pay`, bought into the
    /// list `into`. Waits until the player leaves.
    Shop { pay: String, into: String, stock: Vec<ShopEntry> },
    /// Ask the player to type a text into the text variable `var`; waits for it.
    /// `prompt` is tagged text with `{name}` interpolation (like narration). `max`
    /// counts characters. With a `default`, confirming an empty field stores it.
    Input { var: String, prompt: String, default: Option<String>, max: usize },
    Jump { target: usize, cond: Option<Cond> },
    /// Jump to a label and come back at the next op after `::return`. `args` are
    /// the label's parameters: each is evaluated (all of them before any is
    /// stored) and assigned to the ordinary variable of that name before the jump.
    Call { target: usize, args: Vec<(String, Expr)> },
    Return,
    Set(Effect),
    /// `::end_replay`: ends a scene replay (see `Program::replays`); in normal play
    /// it does nothing.
    EndReplay,
    /// End of the script. The parser always appends one.
    End,
}

impl Op {
    /// Every expression in the op (conditions, and the values of assignments).
    pub fn exprs(&self) -> Vec<&Expr> {
        match self {
            Op::Jump { cond: Some(c), .. } => vec![c],
            Op::Set(e) => vec![&e.value],
            Op::Hud { elem, .. } => elem.exprs(),
            Op::Call { args, .. } => args.iter().map(|(_, e)| e).collect(),
            Op::Shop { stock, .. } => stock.iter().filter_map(|s| s.cond.as_ref()).collect(),
            Op::Choice { options, .. } => options
                .iter()
                .flat_map(|o| o.cond.iter().chain(o.effects.iter().map(|e| &e.value)))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Every asset file the op needs: [`Op::asset`], or a hotspot block's images.
    pub fn all_assets(&self) -> Vec<(AssetKind, &str)> {
        match self {
            Op::Choice { options, .. } => options
                .iter()
                .filter_map(|o| o.area.as_ref()?.image.as_deref())
                .map(|i| (AssetKind::Image, i))
                .collect(),
            Op::Music { src, intro, .. } => {
                let intro = intro.iter().map(|i| (AssetKind::Audio, i.as_str()));
                std::iter::once((AssetKind::Audio, src.as_str())).chain(intro).collect()
            }
            // Every layer file the command names (the ones it leaves out come from other commands).
            Op::Show { layers, .. } if !layers.is_empty() => layers
                .iter()
                .filter(|(_, l)| !l.is_empty() && anim_id(l).is_none())
                .map(|(_, l)| (AssetKind::Image, l.as_str()))
                .collect(),
            _ => self.asset().into_iter().collect(),
        }
    }

    /// The asset file this op needs, if any.
    pub fn asset(&self) -> Option<(AssetKind, &str)> {
        match self {
            Op::Image { src, .. } => Some((AssetKind::Image, src)),
            Op::Bg { src, .. } if bg_color(src).is_some() || anim_id(src).is_some() => None,
            Op::Bg { src, .. } => Some((kind_from_path(src), src)),
            Op::Hud { elem, .. } => elem.image().map(|i| (AssetKind::Image, i)),
            Op::Particles { set: Some(Particles { kind: ParticleKind::Image(src), .. }), .. } => Some((AssetKind::Image, src)),
            Op::Show { src, .. } if src.is_empty() || anim_id(src).is_some() => None,
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
    fn windows_device_names_are_not_inside() {
        assert!(!is_inside("COM1") && !is_inside("assets/nul.png") && !is_inside("aux/bg.png"));
        assert!(is_inside("assets/console.png") && is_inside("com10.png"), "only the exact stems");
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
        let show = Op::Show { id: "a".into(), src: "a.png".into(), layers: vec![], x: None, y: None, z: None, scale: None, flip: None, xscale: None, yscale: None, fade: 0.0 };
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
    fn adding_to_a_list_stops_at_the_item_limit() {
        let mut v = Vars::new();
        v.insert("bag".into(), Value::List((0..MAX_LIST_ITEMS).map(|i| i.to_string()).collect()));
        effect("bag", AssignOp::Add, Value::Str("one more".into())).apply(&mut v);
        assert_eq!(v["bag"], Value::List((0..MAX_LIST_ITEMS).map(|i| i.to_string()).collect()));
        effect("bag", AssignOp::Sub, Value::Str("0".into())).apply(&mut v);
        effect("bag", AssignOp::Add, Value::Str("one more".into())).apply(&mut v);
        assert!(matches!(&v["bag"], Value::List(l) if l.len() == MAX_LIST_ITEMS && l.last().unwrap() == "one more"));
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
