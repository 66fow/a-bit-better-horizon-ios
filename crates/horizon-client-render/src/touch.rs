//! Touch controls for phones and tablets (Horizon's own), after Minecraft: Pocket Edition's: a D-pad (or an
//! analog stick) on the left, jump on the right, perspective, chat and pause at the top, and the hotbar and
//! an inventory button along the bottom. A finger anywhere else turns the view; a short tap uses (or attacks
//! the entity under the crosshair), and holding still mines.
//!
//! This module is the layout and the fingers' state: positions in GUI pixels in, [`Out`]s for the game to act
//! on out. game/touch.rs feeds it winit's touches and acts on what comes back.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use winit::{event::MouseButton, keyboard::KeyCode};

use crate::keybinds;

/// A button of the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Control {
    Forward,
    Back,
    Left,
    Right,
    /// The D-pad's centre: sneaking, toggled.
    Sneak,
    Jump,
    Perspective,
    Chat,
    Pause,
    /// HUD on or off. Hidden with the HUD it hides, but still where it was: a remembered tap brings the
    /// HUD back.
    HideHud,
    Inventory,
    Hotbar(u8),
    /// A button the player added, by its id in [`TouchSettings::buttons`].
    Custom(u16),
    /// The analog stick that replaces the D-pad: a finger on it moves at its own angle and speed.
    Stick,
}

impl Control {
    /// The D-pad's directions, which a finger can slide between.
    fn moves(self) -> bool {
        matches!(
            self,
            Control::Forward | Control::Back | Control::Left | Control::Right
        )
    }
}

/// How the left thumb moves the player.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Movement {
    /// Four directions slid between: the D-pad.
    #[default]
    DPad,
    /// An analog stick: any direction, any speed up to full.
    Stick,
}

/// `[x, y, width, height]` in GUI pixels.
pub type Rect = [f32; 4];

/// The D-pad's and jump's button size on a `h` GUI-pixel-high screen: about a seventh of the height, so a
/// phone (GUI scale 4, 292 high) gets 40 GUI pixels, some 53 points, over Apple's 44-point minimum.
pub fn button_size(h: f32) -> f32 {
    (h * 0.14).clamp(26.0, 48.0)
}
/// The gap kept from the screen's edges.
const MARGIN: f32 = 14.0;
/// How far above the hotbar its slots still take a tap: a thumb lands high.
const HOTBAR_REACH: f32 = 8.0;

/// Where every button is on a `w` x `h` GUI-pixel screen.
pub fn layout(w: f32, h: f32) -> Vec<(Control, Rect)> {
    let s = button_size(h);
    let small = (s * 0.65).round();
    // The D-pad's centre cell, high enough that its bottom row clears the screen's edge.
    let (cx, cy) = (MARGIN + s, h - MARGIN - 2.0 * s - 8.0);
    let mut v = vec![
        (Control::Forward, [cx, cy - s, s, s]),
        (Control::Left, [cx - s, cy, s, s]),
        (Control::Sneak, [cx, cy, s, s]),
        (Control::Right, [cx + s, cy, s, s]),
        (Control::Back, [cx, cy + s, s, s]),
        (Control::Jump, [w - MARGIN - s, cy, s, s]),
    ];
    let top = [
        Control::Perspective,
        Control::Chat,
        Control::Pause,
        Control::HideHud,
    ];
    let x0 = (w - 4.0 * small - 12.0) / 2.0;
    for (i, control) in top.into_iter().enumerate() {
        v.push((control, [x0 + i as f32 * (small + 4.0), 2.0, small, small]));
    }
    // Gui.extractItemHotbar's slots: the bar is 182 wide, centred, 22 high at the bottom.
    let (hx, hy) = ((w / 2.0 - 91.0).floor(), h - 22.0);
    for slot in 0..9u8 {
        v.push((
            Control::Hotbar(slot),
            [
                hx + 1.0 + 20.0 * f32::from(slot),
                hy - HOTBAR_REACH,
                20.0,
                22.0 + HOTBAR_REACH,
            ],
        ));
    }
    v.push((
        Control::Inventory,
        [hx + 184.0, hy - HOTBAR_REACH, 30.0, 22.0 + HOTBAR_REACH],
    ));
    v
}

/// The close button over a screen that has none of its own (an inventory, chat, a sign): the top right.
pub fn close_rect(w: f32, h: f32) -> Rect {
    let s = (button_size(h) * 0.75).round();
    [w - s - 4.0, 4.0, s, s]
}

/// The keyboard button, left of the close button: over a focused text field, it asks for the system's
/// keyboard again.
pub fn keyboard_rect(w: f32, h: f32) -> Rect {
    let c = close_rect(w, h);
    [c[0] - c[2] - 4.0, c[1], c[2], c[3]]
}

/// The point is inside the rectangle.
pub fn hit_rect(r: Rect, at: (f32, f32)) -> bool {
    contains(r, at)
}

fn contains(r: Rect, (x, y): (f32, f32)) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

/// The button under a point.
pub fn hit(layout: &[(Control, Rect)], at: (f32, f32)) -> Option<Control> {
    layout
        .iter()
        .find(|(_, r)| contains(*r, at))
        .map(|(c, _)| *c)
}

/// Horizon's touch overlay settings (`horizon-touch.txt`).
pub const FILE: &str = "horizon-touch.txt";

/// A button moved by the player: its top-left corner as fractions of the screen, so a layout survives a
/// rotation or a different phone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
}

/// A button the player added: what it sends and how it looks.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomButton {
    /// What the button shows; empty until the player names it ("NEW").
    pub name: String,
    /// The keys it sends, in order; empty slots are skipped. Pressed in order, released in reverse.
    pub keys: [Option<String>; MAX_KEYS],
    /// Its top-left corner as fractions of the screen, as [`Placement`].
    pub placement: Placement,
    /// Its width and height, `MIN_SIZE`..=`MAX_SIZE`, multiplied by the overlay's scale.
    pub width: f32,
    pub height: f32,
    /// Its own background opacity, `MIN_OPACITY`..=1.0, multiplied by the overlay's.
    pub opacity: f32,
    /// One tap holds the keys down, the next lets go.
    pub toggle: bool,
    /// A drag turns the view; a tap sends the keys.
    pub swipe: bool,
    /// A solid background color in place of vanilla's button sprite, and the stroke around it.
    pub background: Option<u32>,
    pub stroke: Option<u32>,
}

/// How many keys one button sends.
pub const MAX_KEYS: usize = 4;
/// The most characters a button's name takes.
pub const MAX_NAME: usize = 32;

impl Default for CustomButton {
    fn default() -> Self {
        Self {
            name: String::new(),
            keys: [None, None, None, None],
            placement: Placement { x: 0.5, y: 0.5 },
            width: 1.0,
            height: 1.0,
            opacity: 1.0,
            toggle: false,
            swipe: false,
            background: None,
            stroke: None,
        }
    }
}

/// A key or mouse button a player's button sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomKey {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl CustomKey {
    /// The `key.keyboard.*` or `key.mouse.*` name as a key, if it is one.
    pub fn of(name: &str) -> Option<Self> {
        if let Some(button) = keybinds::mouse_button(name) {
            return Some(Self::Mouse(button));
        }
        keybinds::key_code(name).map(Self::Key)
    }
}

/// A mapping's name as the button and the options page show it: "LMB"/"RMB" for a mouse button,
/// else the key's display name.
pub fn key_label(name: &str) -> String {
    match keybinds::mouse_button(name) {
        Some(MouseButton::Left) => "LMB".to_owned(),
        Some(MouseButton::Right) => "RMB".to_owned(),
        _ => keybinds::display_name(name).plain(),
    }
}

/// How see-through the button backgrounds may become.
pub const MIN_OPACITY: f32 = 0.2;
/// How small and how large the buttons may grow.
pub const MIN_SCALE: f32 = 0.75;
pub const MAX_SCALE: f32 = 1.5;
/// How small and how large a player's button may be drawn, on either side.
pub const MIN_SIZE: f32 = 0.5;
pub const MAX_SIZE: f32 = 3.0;

/// The colors a button's background and stroke can take, with the names the options page shows. Packed as
/// `hud::rgba` packs them: alpha in the top byte, then blue, green, red.
pub const COLORS: [(&str, u32); 9] = [
    ("White", 0xFFF0F0F0),
    ("Gray", 0xFF6E6E6E),
    ("Black", 0xFF1E1E1E),
    ("Red", 0xFF2C34D8),
    ("Orange", 0xFF1C84E8),
    ("Yellow", 0xFF28C8E6),
    ("Green", 0xFF46AA3C),
    ("Blue", 0xFFDC7832),
    ("Purple", 0xFFD25096),
];

/// The next color after `current`: the palette follows "Default".
pub fn cycle_color(current: Option<u32>) -> Option<u32> {
    let index = current.and_then(|c| COLORS.iter().position(|(_, v)| *v == c));
    match index {
        None => Some(COLORS[0].1),
        Some(i) => COLORS.get(i + 1).map(|(_, v)| *v),
    }
}

/// The color's name, "Default" for none.
pub fn color_name(color: Option<u32>) -> &'static str {
    match color {
        None => "Default",
        Some(c) => COLORS
            .iter()
            .find(|(_, v)| *v == c)
            .map_or("Custom", |(name, _)| *name),
    }
}

/// The mouse buttons a custom button can send, by the names [`keybinds::mouse_button`] reads.
pub const MOUSE_KEYS: [&str; 2] = ["key.mouse.left", "key.mouse.right"];

/// The controls the Touch Controls page lists, in layout order. The pause and hide-HUD buttons are not
/// listed: the pause button is a phone's way back, and the hide-HUD button is how the HUD comes back.
pub const CONTROLS: [Control; 10] = [
    Control::Forward,
    Control::Back,
    Control::Left,
    Control::Right,
    Control::Sneak,
    Control::Jump,
    Control::Perspective,
    Control::Chat,
    Control::Pause,
    Control::Inventory,
];

/// The control's name in `horizon-touch.txt`; the hotbar stays where the HUD puts it.
pub fn control_name(control: Control) -> Option<&'static str> {
    Some(match control {
        Control::Forward => "forward",
        Control::Back => "back",
        Control::Left => "left",
        Control::Right => "right",
        Control::Sneak => "sneak",
        Control::Jump => "jump",
        Control::Perspective => "perspective",
        Control::Chat => "chat",
        Control::Pause => "pause",
        Control::HideHud => "hide_hud",
        Control::Inventory => "inventory",
        Control::Stick => "stick",
        Control::Hotbar(_) | Control::Custom(_) => return None,
    })
}

pub fn control_from_name(name: &str) -> Option<Control> {
    Some(match name {
        "forward" => Control::Forward,
        "back" => Control::Back,
        "left" => Control::Left,
        "right" => Control::Right,
        "sneak" => Control::Sneak,
        "jump" => Control::Jump,
        "perspective" => Control::Perspective,
        "chat" => Control::Chat,
        "pause" => Control::Pause,
        "hide_hud" => Control::HideHud,
        "inventory" => Control::Inventory,
        "stick" => Control::Stick,
        _ => return None,
    })
}

/// The lang key naming a button on the settings page.
pub fn control_caption(control: Control) -> &'static str {
    match control {
        Control::Forward => "horizon.options.touch.forward",
        Control::Back => "horizon.options.touch.back",
        Control::Left => "horizon.options.touch.left",
        Control::Right => "horizon.options.touch.right",
        Control::Sneak => "horizon.options.touch.sneak",
        Control::Jump => "horizon.options.touch.jump",
        Control::Perspective => "horizon.options.touch.perspective",
        Control::Chat => "horizon.options.touch.chat",
        Control::Pause => "horizon.options.touch.pause",
        Control::HideHud => "horizon.options.touch.hide_hud",
        Control::Inventory => "horizon.options.touch.inventory",
        Control::Hotbar(_) => "horizon.options.touch.hotbar",
        Control::Custom(_) => "horizon.options.touch.custom",
        Control::Stick => "horizon.options.touch.stick",
    }
}

/// The touch overlay as the player arranged it (`horizon-touch.txt`).
#[derive(Clone, Debug, PartialEq)]
pub struct TouchSettings {
    /// How opaque the button backgrounds are; 1 draws them as Horizon always did.
    pub opacity: f32,
    /// How large the buttons are, `MIN_SCALE`..=`MAX_SCALE`.
    pub scale: f32,
    /// Buttons the player moved, by name: where their top-left corner sits.
    pub placements: BTreeMap<Control, Placement>,
    /// Buttons the player turned off. The pause button is never hidden: a phone needs a way back.
    pub hidden: BTreeSet<Control>,
    /// The player's own buttons, by id.
    pub buttons: BTreeMap<u16, CustomButton>,
    /// The D-pad, or the analog stick in its place.
    pub movement: Movement,
    /// The stick's own size, `MIN_SCALE`..=`MAX_SCALE`, multiplied by the overlay's scale.
    pub stick_scale: f32,
    /// The stick's own opacity, `MIN_OPACITY`..=1.0, multiplied by the overlay's.
    pub stick_opacity: f32,
}

impl Default for TouchSettings {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            scale: 1.0,
            placements: BTreeMap::new(),
            hidden: BTreeSet::new(),
            buttons: BTreeMap::new(),
            movement: Movement::DPad,
            stick_scale: 1.0,
            stick_opacity: 1.0,
        }
    }
}

fn clamp_unit(value: &str, default: f32, lo: f32, hi: f32) -> f32 {
    value
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .map_or(default, |v| v.clamp(lo, hi))
}

/// A mapping as a button keeps it: a key or mouse button name, or `None` for an empty slot.
fn parse_key(value: &str) -> Option<String> {
    let value = value.trim();
    CustomKey::of(value).map(|_| value.to_owned())
}

/// `-` for no color, else eight hex digits.
fn parse_color(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        return None;
    }
    u32::from_str_radix(value, 16).ok()
}

fn write_color(color: Option<u32>) -> String {
    color.map_or_else(|| "-".to_owned(), |c| format!("{c:08X}"))
}

/// A name keeps its characters but not the file's own separators.
fn escape_name(name: &str) -> String {
    name.replace('%', "%25").replace(':', "%3A")
}
fn unescape_name(name: &str) -> String {
    name.replace("%3A", ":").replace("%25", "%")
}

/// `button:`'s value: the id and the button, in either the old five-field format or the current one.
fn parse_button(value: &str) -> Option<(u16, CustomButton)> {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 5 && parts.len() != 14 {
        return None;
    }
    let id = parts[0].trim().parse::<u16>().ok()?;
    let (x, y) = parts[1].split_once(',')?;
    let default = Placement { x: 0.5, y: 0.5 };
    let placement = Placement {
        x: clamp_unit(x.trim(), default.x, 0.0, 1.0),
        y: clamp_unit(y.trim(), default.y, 0.0, 1.0),
    };
    if parts.len() == 5 {
        // `<scale>:<opacity>:<key>`, a square button.
        let scale = clamp_unit(parts[2], 1.0, MIN_SCALE, MAX_SCALE);
        return Some((
            id,
            CustomButton {
                placement,
                width: scale,
                height: scale,
                opacity: clamp_unit(parts[3], 1.0, MIN_OPACITY, 1.0),
                keys: [parse_key(parts[4]), None, None, None],
                ..CustomButton::default()
            },
        ));
    }
    Some((
        id,
        CustomButton {
            name: unescape_name(parts[5].trim())
                .chars()
                .take(MAX_NAME)
                .collect(),
            keys: std::array::from_fn(|i| parse_key(parts[10 + i])),
            placement,
            width: clamp_unit(parts[2], 1.0, MIN_SIZE, MAX_SIZE),
            height: clamp_unit(parts[3], 1.0, MIN_SIZE, MAX_SIZE),
            opacity: clamp_unit(parts[4], 1.0, MIN_OPACITY, 1.0),
            toggle: parts[6].trim() == "1",
            swipe: parts[7].trim() == "1",
            background: parse_color(parts[8]),
            stroke: parse_color(parts[9]),
        },
    ))
}

impl TouchSettings {
    pub fn load(dir: &Path) -> Self {
        match fs::read_to_string(dir.join(FILE)) {
            Ok(text) => Self::parse(&text),
            Err(_) => Self::default(),
        }
    }
    /// Written once something differs from the defaults, so players who never touch the overlay get no new
    /// file.
    pub fn save(&self, dir: &Path) {
        let path = dir.join(FILE);
        if *self != Self::default() || path.exists() {
            let _ = fs::write(path, self.serialize());
        }
    }
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "opacity" => s.opacity = clamp_unit(value, s.opacity, MIN_OPACITY, 1.0),
                "scale" => s.scale = clamp_unit(value, s.scale, MIN_SCALE, MAX_SCALE),
                "movement" => {
                    if value == "stick" {
                        s.movement = Movement::Stick;
                    }
                }
                // `stick:<scale>:<opacity>`, as `serialize` writes it.
                "stick" => {
                    if let Some((scale, opacity)) = value.split_once(':') {
                        s.stick_scale =
                            clamp_unit(scale.trim(), s.stick_scale, MIN_SCALE, MAX_SCALE);
                        s.stick_opacity =
                            clamp_unit(opacity.trim(), s.stick_opacity, MIN_OPACITY, 1.0);
                    }
                }
                "hidden" => {
                    if let Some(control) = control_from_name(value) {
                        s.set_hidden(control, true);
                    }
                }
                // `place:<name>:x,y`, as `serialize` writes it.
                "place" => {
                    if let Some((name, xy)) = value.split_once(':')
                        && let Some(control) = control_from_name(name.trim())
                        && let Some((x, y)) = xy.split_once(',')
                    {
                        let default = Placement { x: 0.0, y: 0.0 };
                        let placement = Placement {
                            x: clamp_unit(x.trim(), default.x, 0.0, 1.0),
                            y: clamp_unit(y.trim(), default.y, 0.0, 1.0),
                        };
                        s.placements.insert(control, placement);
                    }
                }
                // `button:<id>:x,y:width:height:opacity:name:toggle:swipe:bg:stroke:k0:k1:k2:k3`, as
                // `serialize` writes it; an older five-field line
                // (`button:<id>:x,y:scale:opacity:key`) still reads.
                "button" => {
                    if let Some((id, button)) = parse_button(value) {
                        s.buttons.insert(id, button);
                    }
                }
                _ => {}
            }
        }
        s
    }
    pub fn serialize(&self) -> String {
        let mut text = format!("opacity:{}\nscale:{}\n", self.opacity, self.scale);
        if self.movement == Movement::Stick {
            text.push_str("movement:stick\n");
        }
        if self.stick_scale != 1.0 || self.stick_opacity != 1.0 {
            text.push_str(&format!(
                "stick:{}:{}\n",
                self.stick_scale, self.stick_opacity
            ));
        }
        for (control, placement) in &self.placements {
            if let Some(name) = control_name(*control) {
                text.push_str(&format!("place:{name}:{},{}\n", placement.x, placement.y));
            }
        }
        for control in &self.hidden {
            if let Some(name) = control_name(*control) {
                text.push_str(&format!("hidden:{name}\n"));
            }
        }
        for (id, button) in &self.buttons {
            let key = |slot: usize| button.keys[slot].as_deref().unwrap_or("");
            text.push_str(&format!(
                "button:{id}:{},{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}\n",
                button.placement.x,
                button.placement.y,
                button.width,
                button.height,
                button.opacity,
                escape_name(&button.name),
                u8::from(button.toggle),
                u8::from(button.swipe),
                write_color(button.background),
                write_color(button.stroke),
                key(0),
                key(1),
                key(2),
                key(3),
            ));
        }
        text
    }
    /// Default positions, resized and moved; the hotbar follows the HUD and is never hidden.
    pub fn layout(&self, w: f32, h: f32) -> Vec<(Control, Rect)> {
        let scale = self.scale.clamp(MIN_SCALE, MAX_SCALE);
        let mut v = layout(w, h);
        if self.movement == Movement::Stick {
            // The stick takes the D-pad's 3×3 box, and sneak moves to the left of jump.
            v.retain(|(control, _)| {
                !matches!(
                    control,
                    Control::Forward | Control::Back | Control::Left | Control::Right
                )
            });
            let s = button_size(h);
            let (cx, cy) = (MARGIN + s, h - MARGIN - 2.0 * s - 8.0);
            v.push((Control::Stick, [cx - s, cy - s, 3.0 * s, 3.0 * s]));
            if let Some((_, sneak)) = v.iter_mut().find(|(c, _)| *c == Control::Sneak) {
                *sneak = [w - MARGIN - 2.0 * s - 8.0, cy, s, s];
            }
        }
        v.retain(|(control, _)| self.visible(*control));
        for (control, r) in &mut v {
            if matches!(control, Control::Hotbar(_)) {
                continue;
            }
            let own = if *control == Control::Stick {
                self.stick_scale.clamp(MIN_SCALE, MAX_SCALE)
            } else {
                1.0
            };
            let (bw, bh) = (r[2] * scale * own, r[3] * scale * own);
            let (mut x, mut y) = (r[0] + (r[2] - bw) / 2.0, r[1] + (r[3] - bh) / 2.0);
            if let Some(placement) = self.placements.get(control) {
                x = placement.x * w;
                y = placement.y * h;
            }
            *r = [
                x.clamp(0.0, (w - bw).max(0.0)),
                y.clamp(0.0, (h - bh).max(0.0)),
                bw,
                bh,
            ];
        }
        // The player's buttons, after the built-in ones: each its own size.
        let base = button_size(h);
        for (id, button) in &self.buttons {
            let side = |v: f32| {
                (base * scale * v.clamp(MIN_SIZE, MAX_SIZE))
                    .round()
                    .max(1.0)
            };
            let (bw, bh) = (side(button.width), side(button.height));
            let x = button.placement.x * w;
            let y = button.placement.y * h;
            v.push((
                Control::Custom(*id),
                [
                    x.clamp(0.0, (w - bw).max(0.0)),
                    y.clamp(0.0, (h - bh).max(0.0)),
                    bw,
                    bh,
                ],
            ));
        }
        v
    }
    pub fn visible(&self, control: Control) -> bool {
        matches!(
            control,
            Control::Hotbar(_)
                | Control::Pause
                | Control::HideHud
                | Control::Custom(_)
                | Control::Stick
        ) || !self.hidden.contains(&control)
    }
    pub fn set_hidden(&mut self, control: Control, hidden: bool) {
        if matches!(
            control,
            Control::Hotbar(_)
                | Control::Pause
                | Control::HideHud
                | Control::Custom(_)
                | Control::Stick
        ) {
            return;
        }
        if hidden {
            self.hidden.insert(control);
        } else {
            self.hidden.remove(&control);
        }
    }
    /// Where a button's top-left corner goes, as fractions of the screen.
    pub fn set_placement(&mut self, control: Control, x: f32, y: f32) {
        let placement = Placement {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
        };
        match control {
            Control::Hotbar(_) => {}
            Control::Custom(id) => {
                if let Some(button) = self.buttons.get_mut(&id) {
                    button.placement = placement;
                }
            }
            _ => {
                self.placements.insert(control, placement);
            }
        }
    }
    /// A new button at the middle of the screen, showing "NEW" until it is named.
    pub fn add_button(&mut self, w: f32, h: f32) -> u16 {
        let id = self
            .buttons
            .keys()
            .next_back()
            .copied()
            .map_or(0, |id| id.saturating_add(1));
        let size = (button_size(h) * self.scale.clamp(MIN_SCALE, MAX_SCALE)).round();
        self.buttons.insert(
            id,
            CustomButton {
                placement: Placement {
                    x: ((w - size) / 2.0).max(0.0) / w.max(1.0),
                    y: ((h - size) / 2.0).max(0.0) / h.max(1.0),
                },
                ..CustomButton::default()
            },
        );
        id
    }
    pub fn remove_button(&mut self, id: u16) {
        self.buttons.remove(&id);
    }
    pub fn button(&self, id: u16) -> Option<&CustomButton> {
        self.buttons.get(&id)
    }
    /// The name the player gave it; the button shows "NEW" while it is empty.
    pub fn set_button_name(&mut self, id: u16, name: String) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.name = name.chars().filter(|c| *c != '\n').take(MAX_NAME).collect();
        }
    }
    pub fn set_button_key(&mut self, id: u16, slot: usize, key: Option<String>) {
        if let Some(button) = self.buttons.get_mut(&id)
            && slot < MAX_KEYS
        {
            button.keys[slot] = key;
        }
    }
    pub fn clear_button_keys(&mut self, id: u16) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.keys = std::array::from_fn(|_| None);
        }
    }
    pub fn set_button_size(&mut self, id: u16, width: f32, height: f32) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.width = width.clamp(MIN_SIZE, MAX_SIZE);
            button.height = height.clamp(MIN_SIZE, MAX_SIZE);
        }
    }
    pub fn set_button_opacity(&mut self, id: u16, opacity: f32) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.opacity = opacity.clamp(MIN_OPACITY, 1.0);
        }
    }
    pub fn set_button_toggle(&mut self, id: u16, toggle: bool) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.toggle = toggle;
        }
    }
    pub fn set_button_swipe(&mut self, id: u16, swipe: bool) {
        if let Some(button) = self.buttons.get_mut(&id) {
            button.swipe = swipe;
        }
    }
    /// The background (`stroke` false) or stroke color, `None` for vanilla's look.
    pub fn set_button_color(&mut self, id: u16, stroke: bool, color: Option<u32>) {
        if let Some(button) = self.buttons.get_mut(&id) {
            if stroke {
                button.stroke = color;
            } else {
                button.background = color;
            }
        }
    }
    /// The mapping in `slot`, if one is set.
    pub fn button_key_name(&self, id: u16, slot: usize) -> Option<&str> {
        self.button(id)?.keys.get(slot)?.as_deref()
    }
    /// Every mapping the button sends, in order.
    pub fn button_mappings(&self, id: u16) -> Vec<CustomKey> {
        let Some(button) = self.button(id) else {
            return Vec::new();
        };
        button
            .keys
            .iter()
            .flatten()
            .filter_map(|name| CustomKey::of(name))
            .collect()
    }
    /// The buttons whose drags turn the view instead.
    pub fn swipes(&self) -> BTreeSet<Control> {
        self.buttons
            .iter()
            .filter(|(_, button)| button.swipe)
            .map(|(id, _)| Control::Custom(*id))
            .collect()
    }
    /// What the button shows: the name the player gave it, or "NEW".
    pub fn button_label(&self, id: u16) -> String {
        match self.button(id) {
            Some(button) if !button.name.is_empty() => button.name.clone(),
            _ => "NEW".to_owned(),
        }
    }
    /// The default positions and size; hidden buttons stay hidden and opacity stays as it is.
    pub fn reset_layout(&mut self) {
        self.scale = 1.0;
        self.placements.clear();
    }
}

/// What the game does with the fingers' changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    Press(Control),
    Release(Control),
    /// Turn the view by this much, in GUI pixels dragged.
    Look(f32, f32),
    /// A short tap away from the buttons: use, or attack what the crosshair is on.
    Tap,
    /// A finger held still away from the buttons: mining starts, and stops when it lifts.
    MineStart,
    MineEnd,
    /// The analog stick's position: `[left, forward]`, each -1..=1.
    Move(f32, f32),
}

/// How far a finger moves (GUI pixels) before a touch is a drag and not a tap.
const SLOP: f32 = 8.0;
/// How long a finger stays still (seconds) before it mines.
const HOLD: f64 = 0.35;
/// How far from the stick's centre (as a share of its radius) a finger moves nothing.
const STICK_DEADZONE: f32 = 0.15;

/// The stick's `[left, forward]` for a finger at `at`: zero in the middle, full at the ring and beyond.
fn stick_vector(r: Rect, at: (f32, f32)) -> (f32, f32) {
    let radius = (r[2].min(r[3]) / 2.0).max(1.0);
    let dx = (at.0 - (r[0] + r[2] / 2.0)) / radius;
    let dy = (at.1 - (r[1] + r[3] / 2.0)) / radius;
    let len = dx.hypot(dy);
    if len <= STICK_DEADZONE {
        return (0.0, 0.0);
    }
    let mag = ((len - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)).min(1.0);
    // Screen right is strafing right, screen down is walking back.
    (-dx / len * mag, -dy / len * mag)
}

fn stick_rect(layout: &[(Control, Rect)]) -> Option<Rect> {
    layout
        .iter()
        .find(|(c, _)| *c == Control::Stick)
        .map(|(_, r)| *r)
}

#[derive(Clone, Debug)]
enum Kind {
    Button(Control),
    World {
        dragged: bool,
        mining: bool,
    },
    /// A player's swipeable button: a drag turns the view, a tap sends its keys.
    Swipe {
        control: Control,
        dragged: bool,
    },
    /// The analog stick, wherever its rect is now (the layout may change under the finger).
    Stick,
}

#[derive(Clone, Debug)]
struct Finger {
    id: u64,
    kind: Kind,
    start: (f32, f32),
    last: (f32, f32),
    since: f64,
}

/// The fingers on the screen while playing.
#[derive(Clone, Debug, Default)]
pub struct Touches {
    fingers: Vec<Finger>,
}

impl Touches {
    /// A finger went down at `at`, `now` seconds into some clock. `swipes` are the buttons whose drags
    /// turn the view instead of sending their keys.
    pub fn start(
        &mut self,
        id: u64,
        at: (f32, f32),
        now: f64,
        layout: &[(Control, Rect)],
        swipes: &BTreeSet<Control>,
    ) -> Vec<Out> {
        let mut out = self.end(id);
        let kind = match hit(layout, at) {
            Some(Control::Stick) => {
                if let Some(r) = stick_rect(layout) {
                    let (left, forward) = stick_vector(r, at);
                    out.push(Out::Move(left, forward));
                }
                Kind::Stick
            }
            Some(control) if swipes.contains(&control) => Kind::Swipe {
                control,
                dragged: false,
            },
            Some(control) => {
                out.push(Out::Press(control));
                Kind::Button(control)
            }
            None => Kind::World {
                dragged: false,
                mining: false,
            },
        };
        self.fingers.push(Finger {
            id,
            kind,
            start: at,
            last: at,
            since: now,
        });
        out
    }

    /// A finger moved: the D-pad follows it from direction to direction, and a finger on the world turns
    /// the view.
    pub fn moved(&mut self, id: u64, at: (f32, f32), layout: &[(Control, Rect)]) -> Vec<Out> {
        let Some(f) = self.fingers.iter_mut().find(|f| f.id == id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match &mut f.kind {
            Kind::Stick => {
                if let Some(r) = stick_rect(layout) {
                    let (left, forward) = stick_vector(r, at);
                    out.push(Out::Move(left, forward));
                }
            }
            Kind::Button(control) if control.moves() => {
                if let Some(next) = hit(layout, at).filter(|c| c.moves() && c != control) {
                    out.push(Out::Release(*control));
                    out.push(Out::Press(next));
                    *control = next;
                }
            }
            Kind::Button(_) => {}
            Kind::World { dragged, .. } | Kind::Swipe { dragged, .. } => {
                let (dx, dy) = (at.0 - f.start.0, at.1 - f.start.1);
                if !*dragged && dx.hypot(dy) > SLOP {
                    *dragged = true;
                }
                if *dragged {
                    out.push(Out::Look(at.0 - f.last.0, at.1 - f.last.1));
                }
            }
        }
        f.last = at;
        out
    }

    /// A finger lifted.
    pub fn end(&mut self, id: u64) -> Vec<Out> {
        let Some(i) = self.fingers.iter().position(|f| f.id == id) else {
            return Vec::new();
        };
        match self.fingers.remove(i).kind {
            Kind::Stick => vec![Out::Move(0.0, 0.0)],
            Kind::Button(control) => vec![Out::Release(control)],
            Kind::Swipe {
                control,
                dragged: false,
            } => vec![Out::Press(control), Out::Release(control)],
            Kind::Swipe { .. } => Vec::new(),
            Kind::World { mining: true, .. } => vec![Out::MineEnd],
            Kind::World { dragged: false, .. } => vec![Out::Tap],
            Kind::World { .. } => Vec::new(),
        }
    }

    /// UIKit cancelled a finger: release its holds without interpreting it as a tap.
    pub fn cancel(&mut self, id: u64) -> Vec<Out> {
        let Some(i) = self.fingers.iter().position(|f| f.id == id) else {
            return Vec::new();
        };
        match self.fingers.remove(i).kind {
            Kind::Stick => vec![Out::Move(0.0, 0.0)],
            Kind::Button(control) => vec![Out::Release(control)],
            Kind::World { mining: true, .. } => vec![Out::MineEnd],
            Kind::World { .. } | Kind::Swipe { .. } => Vec::new(),
        }
    }

    /// Once a frame: a finger on the world that has stayed still long enough starts mining.
    pub fn frame(&mut self, now: f64) -> Vec<Out> {
        let mut out = Vec::new();
        for f in &mut self.fingers {
            if let Kind::World {
                dragged: false,
                mining,
            } = &mut f.kind
                && !*mining
                && now - f.since >= HOLD
            {
                *mining = true;
                out.push(Out::MineStart);
            }
        }
        out
    }

    /// Every finger lifted at once (a screen opened over the game).
    pub fn release_all(&mut self) -> Vec<Out> {
        let ids: Vec<u64> = self.fingers.iter().map(|f| f.id).collect();
        ids.into_iter().flat_map(|id| self.cancel(id)).collect()
    }

    /// A finger holds this button down (drawn pressed).
    pub fn held(&self, control: Control) -> bool {
        self.fingers.iter().any(|f| match f.kind {
            Kind::Button(c) | Kind::Swipe { control: c, .. } => c == control,
            Kind::Stick => control == Control::Stick,
            Kind::World { .. } => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(layout: &[(Control, Rect)], control: Control) -> (f32, f32) {
        let r = layout.iter().find(|(c, _)| *c == control).unwrap().1;
        (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
    }

    const NO_SWIPES: &BTreeSet<Control> = &BTreeSet::new();

    #[test]
    fn buttons_do_not_overlap_and_fit_the_screen() {
        for (w, h) in [(633.0, 292.0), (427.0, 240.0), (480.0, 270.0)] {
            let l = layout(w, h);
            for (i, (a, ra)) in l.iter().enumerate() {
                assert!(
                    ra[0] >= 0.0 && ra[1] >= 0.0 && ra[0] + ra[2] <= w && ra[1] + ra[3] <= h,
                    "{a:?} off screen"
                );
                for (b, rb) in &l[i + 1..] {
                    let apart = ra[0] + ra[2] <= rb[0]
                        || rb[0] + rb[2] <= ra[0]
                        || ra[1] + ra[3] <= rb[1]
                        || rb[1] + rb[3] <= ra[1];
                    assert!(apart, "{a:?} overlaps {b:?} at {w}x{h}");
                }
            }
        }
    }

    #[test]
    fn the_dpad_follows_a_sliding_finger() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        assert_eq!(
            t.start(1, at(&l, Control::Forward), 0.0, &l, NO_SWIPES),
            [Out::Press(Control::Forward)]
        );
        assert!(t.held(Control::Forward));
        assert_eq!(
            t.moved(1, at(&l, Control::Right), &l),
            [Out::Release(Control::Forward), Out::Press(Control::Right)]
        );
        // Off the pad, the last direction stays held, as in Pocket Edition.
        assert_eq!(t.moved(1, (300.0, 100.0), &l), []);
        assert_eq!(t.end(1), [Out::Release(Control::Right)]);
        assert!(!t.held(Control::Right));
    }

    #[test]
    fn a_quick_still_touch_taps_and_a_held_one_mines() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        assert_eq!(t.start(1, (300.0, 100.0), 0.0, &l, NO_SWIPES), []);
        assert_eq!(t.frame(0.1), []);
        assert_eq!(t.end(1), [Out::Tap]);

        t.start(2, (300.0, 100.0), 1.0, &l, NO_SWIPES);
        assert_eq!(t.frame(1.3), []);
        assert_eq!(t.frame(1.4), [Out::MineStart]);
        assert_eq!(t.frame(1.5), []);
        assert_eq!(t.end(2), [Out::MineEnd]);
    }

    #[test]
    fn a_dragging_finger_turns_the_view_and_does_not_tap() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        t.start(1, (300.0, 100.0), 0.0, &l, NO_SWIPES);
        // Within the slop (a thumb's wobble): no look yet, and still a tap candidate.
        assert_eq!(t.moved(1, (305.0, 100.0), &l), []);
        assert_eq!(t.moved(1, (313.0, 104.0), &l), [Out::Look(8.0, 4.0)]);
        assert_eq!(t.frame(5.0), []);
        assert_eq!(t.end(1), []);
    }

    #[test]
    fn two_fingers_move_and_look_at_once() {
        let l = layout(633.0, 292.0);
        let mut t = Touches::default();
        t.start(1, at(&l, Control::Forward), 0.0, &l, NO_SWIPES);
        t.start(2, (400.0, 100.0), 0.0, &l, NO_SWIPES);
        assert_eq!(t.moved(2, (420.0, 100.0), &l), [Out::Look(20.0, 0.0)]);
        assert!(t.held(Control::Forward));
        let mut all = t.release_all();
        all.sort_by_key(|o| format!("{o:?}"));
        assert_eq!(all, [Out::Release(Control::Forward)]);
    }

    fn placed(l: &[(Control, Rect)], control: Control) -> Rect {
        l.iter().find(|(c, _)| *c == control).unwrap().1
    }

    #[test]
    fn touch_settings_survive_a_round_trip() {
        let mut s = TouchSettings::default();
        s.opacity = 0.5;
        s.scale = 1.25;
        s.set_placement(Control::Jump, 0.25, 0.6);
        s.set_hidden(Control::Chat, true);
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
    }

    #[test]
    fn the_hotbar_pause_and_hide_hud_stay_on_and_in_place() {
        let mut s = TouchSettings::default();
        s.set_hidden(Control::Pause, true);
        s.set_hidden(Control::HideHud, true);
        s.set_hidden(Control::Hotbar(3), true);
        assert!(s.visible(Control::Pause));
        assert!(s.visible(Control::HideHud));
        assert!(s.visible(Control::Hotbar(3)));
        let l = s.layout(633.0, 292.0);
        assert!(l.iter().any(|(c, _)| *c == Control::Pause));
        assert!(l.iter().any(|(c, _)| *c == Control::HideHud));
        assert!(l.iter().any(|(c, _)| *c == Control::Hotbar(3)));
        // A file naming it is read but still leaves it on.
        let old = TouchSettings::parse("hidden:hide_hud\n");
        assert!(old.visible(Control::HideHud));
        assert_eq!(control_from_name("hide_hud"), Some(Control::HideHud));
    }

    #[test]
    fn the_hide_hud_button_sits_in_the_top_row_after_pause() {
        let l = layout(633.0, 292.0);
        let (pause, hud) = (placed(&l, Control::Pause), placed(&l, Control::HideHud));
        assert!(hud[0] > pause[0]);
        assert!((hud[1] - pause[1]).abs() < 0.01);
        assert!((hud[2] - pause[2]).abs() < 0.01);
    }

    #[test]
    fn a_swipe_button_turns_the_view_and_a_tap_sends_its_keys() {
        let mut s = TouchSettings::default();
        let id = s.add_button(633.0, 292.0);
        s.set_button_swipe(id, true);
        assert_eq!(s.swipes(), BTreeSet::from([Control::Custom(id)]));
        let l = s.layout(633.0, 292.0);
        let r = placed(&l, Control::Custom(id));
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let swipes = s.swipes();
        let mut t = Touches::default();
        // A finger that drags turns the view, and sends no key when it lifts.
        assert_eq!(t.start(1, centre, 0.0, &l, &swipes), []);
        assert!(t.held(Control::Custom(id)));
        assert_eq!(
            t.moved(1, (centre.0 + 20.0, centre.1), &l),
            [Out::Look(20.0, 0.0)]
        );
        assert_eq!(t.end(1), []);
        // A tap presses and releases its keys at once.
        t.start(2, centre, 1.0, &l, &swipes);
        assert_eq!(
            t.end(2),
            [
                Out::Press(Control::Custom(id)),
                Out::Release(Control::Custom(id))
            ]
        );
        // A cancelled tap sends nothing.
        t.start(3, centre, 2.0, &l, &swipes);
        assert_eq!(t.cancel(3), []);
        // A button that is not swipeable holds its key while the finger is on it.
        t.start(4, centre, 3.0, &l, NO_SWIPES);
        assert_eq!(t.end(4), [Out::Release(Control::Custom(id))]);
    }

    #[test]
    fn colors_cycle_through_the_palette_and_back_to_default() {
        let mut color = None;
        for (name, value) in COLORS {
            color = cycle_color(color);
            assert_eq!(color, Some(value));
            assert_eq!(color_name(color), name);
        }
        assert_eq!(cycle_color(color), None);
        assert_eq!(color_name(None), "Default");
    }

    #[test]
    fn a_hidden_button_leaves_the_layout() {
        let mut s = TouchSettings::default();
        s.set_hidden(Control::Chat, true);
        let l = s.layout(633.0, 292.0);
        assert!(!l.iter().any(|(c, _)| *c == Control::Chat));
        assert_eq!(l.len(), layout(633.0, 292.0).len() - 1);
    }

    #[test]
    fn a_placement_moves_a_button_and_keeps_it_on_screen() {
        let mut s = TouchSettings::default();
        s.set_placement(Control::Jump, 0.1, 0.2);
        let r = placed(&s.layout(600.0, 300.0), Control::Jump);
        assert!((r[0] - 60.0).abs() < 0.01 && (r[1] - 60.0).abs() < 0.01);
        // Far over the edge: the button is pulled back onto the screen.
        s.set_placement(Control::Jump, 5.0, 5.0);
        let r = placed(&s.layout(600.0, 300.0), Control::Jump);
        assert!(r[0] >= 0.0 && r[1] >= 0.0);
        assert!(r[0] + r[2] <= 600.0 && r[1] + r[3] <= 300.0);
    }

    #[test]
    fn scale_resizes_buttons_but_not_the_hotbar() {
        let mut s = TouchSettings::default();
        s.scale = 1.5;
        let base = layout(600.0, 300.0);
        let l = s.layout(600.0, 300.0);
        let (b, j) = (placed(&base, Control::Jump), placed(&l, Control::Jump));
        assert!((j[2] - b[2] * 1.5).abs() < 0.01);
        assert_eq!(
            placed(&l, Control::Hotbar(0)),
            placed(&base, Control::Hotbar(0))
        );
    }

    #[test]
    fn custom_buttons_survive_a_round_trip() {
        let mut s = TouchSettings::default();
        let id = s.add_button(633.0, 292.0);
        s.set_button_name(id, "Fly".into());
        s.set_button_key(id, 0, Some("key.keyboard.f3".into()));
        s.set_button_key(id, 2, Some("key.mouse.left".into()));
        s.set_button_size(id, 1.25, 0.75);
        s.set_button_opacity(id, 0.5);
        s.set_button_toggle(id, true);
        s.set_button_swipe(id, true);
        s.set_button_color(id, false, Some(COLORS[3].1));
        s.set_button_color(id, true, Some(COLORS[6].1));
        s.set_placement(Control::Custom(id), 0.1, 0.7);
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        // The keys press in order; empty slots are skipped.
        assert_eq!(
            s.button_mappings(id),
            [
                CustomKey::Key(winit::keyboard::KeyCode::F3),
                CustomKey::Mouse(winit::event::MouseButton::Left)
            ]
        );
        // A name keeps even the file's own separators.
        s.set_button_name(id, "a:b%c".into());
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        assert_eq!(s.button_label(id), "a:b%c");
    }

    #[test]
    fn old_button_lines_still_load_and_new_ones_are_written() {
        let s = TouchSettings::parse("button:7:0.25,0.5:1.5:0.5:key.keyboard.g\n");
        let b = s.button(7).unwrap();
        assert_eq!((b.width, b.height), (1.5, 1.5));
        assert_eq!(b.opacity, 0.5);
        assert_eq!(b.keys[0].as_deref(), Some("key.keyboard.g"));
        assert!(b.name.is_empty() && !b.toggle && !b.swipe);
        assert_eq!(b.background, None);
        // Saved back, the button keeps its shape in the current format.
        let again = TouchSettings::parse(&s.serialize());
        assert_eq!(again, s);
        assert!(s.serialize().contains("button:7:"));
        let line = s.serialize();
        let line = line.lines().find(|l| l.starts_with("button:")).unwrap();
        assert_eq!(line.split(':').count(), 15);
    }

    #[test]
    fn a_custom_button_can_send_a_mouse_click() {
        let mut s = TouchSettings::default();
        let id = s.add_button(600.0, 300.0);
        s.set_button_key(id, 0, Some("key.mouse.left".into()));
        assert_eq!(key_label("key.mouse.left"), "LMB");
        assert_eq!(
            s.button_mappings(id),
            [CustomKey::Mouse(winit::event::MouseButton::Left)]
        );
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        s.set_button_key(id, 0, Some("key.mouse.right".into()));
        assert_eq!(key_label("key.mouse.right"), "RMB");
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        // A name that is neither a key nor a mouse button is still refused.
        let bad = TouchSettings::parse("button:1:0.5,0.5:1:1:1::0:0:-:-:key.mouse.nope:::\n");
        assert!(bad.button_mappings(1).is_empty());
    }

    #[test]
    fn a_new_button_is_centred_and_labelled_new() {
        let mut s = TouchSettings::default();
        let id = s.add_button(600.0, 300.0);
        assert_eq!(s.button_label(id), "NEW");
        assert!(s.button_mappings(id).is_empty());
        let r = placed(&s.layout(600.0, 300.0), Control::Custom(id));
        let size = r[2];
        assert!((r[0] - (600.0 - size) / 2.0).abs() < 0.01);
        assert!((r[1] - (300.0 - size) / 2.0).abs() < 0.01);
        // Naming it replaces "NEW"; clearing the name brings it back.
        s.set_button_name(id, "Sprint".into());
        assert_eq!(s.button_label(id), "Sprint");
        s.set_button_name(id, String::new());
        assert_eq!(s.button_label(id), "NEW");
    }

    #[test]
    fn custom_buttons_get_unique_ids_and_can_be_removed() {
        let mut s = TouchSettings::default();
        let a = s.add_button(640.0, 360.0);
        let b = s.add_button(640.0, 360.0);
        assert_ne!(a, b);
        s.remove_button(a);
        assert!(s.button(a).is_none());
        assert!(
            s.layout(640.0, 360.0)
                .iter()
                .any(|(c, _)| *c == Control::Custom(b))
        );
    }

    #[test]
    fn resetting_the_layout_keeps_custom_buttons() {
        let mut s = TouchSettings::default();
        let id = s.add_button(600.0, 300.0);
        s.set_button_key(id, 0, Some("key.keyboard.f3".into()));
        s.reset_layout();
        assert_eq!(
            s.button_mappings(id),
            [CustomKey::Key(winit::keyboard::KeyCode::F3)]
        );
    }

    #[test]
    fn a_custom_button_uses_its_own_width_height_and_opacity() {
        let mut s = TouchSettings::default();
        let id = s.add_button(600.0, 300.0);
        let base = placed(&s.layout(600.0, 300.0), Control::Custom(id));
        s.set_button_size(id, 1.5, 0.5);
        s.set_button_opacity(id, 0.1);
        let bigger = placed(&s.layout(600.0, 300.0), Control::Custom(id));
        assert!((bigger[2] - base[2] * 1.5).abs() < 0.01);
        assert!((bigger[3] - base[3] * 0.5).abs() < 0.01);
        let button = s.button(id).unwrap();
        assert_eq!(button.opacity, MIN_OPACITY);
        // Far past either end, the sides stay in bounds.
        s.set_button_size(id, 9.0, 0.0);
        let button = s.button(id).unwrap();
        assert_eq!((button.width, button.height), (MAX_SIZE, MIN_SIZE));
    }

    #[test]
    fn bad_button_lines_are_skipped_or_clamped() {
        let s = TouchSettings::parse(
            "button:x:0,0:1:1:key.keyboard.f3\n\
             button:2:2,-1:9:nope:\n\
             button:3:0.5,0.5:1.25:0.5:key.keyboard.a\n\
             button:4:0,0:1:1:key.keyboard.nope\n\
             button:5:0.5,0.5:1:1:1::0:0:-:-::::\n",
        );
        assert!(s.button(1).is_none());
        let two = s.button(2).unwrap();
        assert_eq!(two.placement, Placement { x: 1.0, y: 0.0 });
        assert_eq!((two.width, two.height), (MAX_SCALE, MAX_SCALE));
        assert_eq!(two.opacity, 1.0);
        assert!(two.keys.iter().all(Option::is_none));
        let three = s.button(3).unwrap();
        assert_eq!(three.keys[0].as_deref(), Some("key.keyboard.a"));
        // An unknown key name leaves the slot empty.
        assert!(s.button(4).unwrap().keys.iter().all(Option::is_none));
        // A new-format line with every slot empty still parses.
        assert_eq!(s.button(5).unwrap().keys, [None, None, None, None]);
        // Too few or too many parts are not a button.
        assert!(TouchSettings::parse("button:6:0,0:1\n").button(6).is_none());
    }

    #[test]
    fn bad_values_fall_back_to_the_defaults() {
        let s = TouchSettings::parse("opacity:9\nscale:nope\nhidden:pause\nplace:jump:2,-1\n");
        assert_eq!(s.opacity, 1.0);
        assert_eq!(s.scale, 1.0);
        assert!(s.visible(Control::Pause));
        assert_eq!(
            s.placements.get(&Control::Jump),
            Some(&Placement { x: 1.0, y: 0.0 })
        );
    }

    fn stick_settings() -> TouchSettings {
        TouchSettings {
            movement: Movement::Stick,
            ..TouchSettings::default()
        }
    }

    #[test]
    fn stick_mode_replaces_the_dpad_and_moves_sneak_left_of_jump() {
        let s = stick_settings();
        let l = s.layout(633.0, 292.0);
        assert!(l.iter().all(|(c, _)| !c.moves()));
        let stick = placed(&l, Control::Stick);
        let s_size = button_size(292.0);
        assert!((stick[2] - 3.0 * s_size).abs() < 0.01);
        assert!((stick[3] - 3.0 * s_size).abs() < 0.01);
        let (sneak, jump) = (placed(&l, Control::Sneak), placed(&l, Control::Jump));
        assert!((sneak[0] + sneak[2] + 8.0 - jump[0]).abs() < 0.01);
        assert!((sneak[1] - jump[1]).abs() < 0.01);
        // The D-pad mode still has its four buttons and no stick.
        let d = TouchSettings::default().layout(633.0, 292.0);
        assert!(d.iter().any(|(c, _)| *c == Control::Forward));
        assert!(d.iter().all(|(c, _)| *c != Control::Stick));
    }

    #[test]
    fn stick_mode_buttons_do_not_overlap_and_fit_the_screen() {
        for (w, h) in [(633.0, 292.0), (427.0, 240.0), (480.0, 270.0)] {
            let l = stick_settings().layout(w, h);
            for (i, (a, ra)) in l.iter().enumerate() {
                assert!(
                    ra[0] >= 0.0 && ra[1] >= 0.0 && ra[0] + ra[2] <= w && ra[1] + ra[3] <= h,
                    "{a:?} off screen"
                );
                for (b, rb) in &l[i + 1..] {
                    let apart = ra[0] + ra[2] <= rb[0]
                        || rb[0] + rb[2] <= ra[0]
                        || ra[1] + ra[3] <= rb[1]
                        || rb[1] + rb[3] <= ra[1];
                    assert!(apart, "{a:?} overlaps {b:?} at {w}x{h}");
                }
            }
        }
    }

    #[test]
    fn the_stick_is_dead_in_the_middle_and_full_past_the_ring() {
        let s = stick_settings();
        let r = placed(&s.layout(633.0, 292.0), Control::Stick);
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let radius = r[2] / 2.0;
        assert_eq!(stick_vector(r, centre), (0.0, 0.0));
        assert_eq!(
            stick_vector(r, (centre.0 + 0.1 * radius, centre.1)),
            (0.0, 0.0)
        );
        // Right strafes right, up walks forward.
        let (left, forward) = stick_vector(r, (centre.0 + radius, centre.1));
        assert!((left + 1.0).abs() < 0.001 && forward.abs() < 0.001);
        let (left, forward) = stick_vector(r, (centre.0, centre.1 - radius));
        assert!(left.abs() < 0.001 && (forward - 1.0).abs() < 0.001);
        // Half way out: half speed once past the deadzone.
        let (_, forward) = stick_vector(r, (centre.0, centre.1 + radius / 2.0));
        assert!((forward + (0.5 - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)).abs() < 0.001);
        // Off the ring, the stick stays at full, and never past it.
        let (left, forward) = stick_vector(r, (centre.0 - 2.0 * radius, centre.1 + 2.0 * radius));
        assert!((left.hypot(forward) - 1.0).abs() < 0.001);
        assert!(left > 0.0 && forward < 0.0);
    }

    #[test]
    fn a_finger_on_the_stick_moves_and_a_lift_stops_it() {
        let s = stick_settings();
        let l = s.layout(633.0, 292.0);
        let r = placed(&l, Control::Stick);
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let radius = r[2] / 2.0;
        let mut t = Touches::default();
        assert_eq!(
            t.start(1, centre, 0.0, &l, NO_SWIPES),
            [Out::Move(0.0, 0.0)]
        );
        assert!(t.held(Control::Stick));
        assert_eq!(
            t.moved(1, (centre.0 - radius, centre.1), &l),
            [Out::Move(1.0, 0.0)]
        );
        assert_eq!(t.end(1), [Out::Move(0.0, 0.0)]);
        assert!(!t.held(Control::Stick));
    }

    #[test]
    fn the_stick_moves_while_another_finger_looks() {
        let s = stick_settings();
        let l = s.layout(633.0, 292.0);
        let r = placed(&l, Control::Stick);
        let centre = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        let mut t = Touches::default();
        t.start(1, centre, 0.0, &l, NO_SWIPES);
        t.start(2, (400.0, 100.0), 0.0, &l, NO_SWIPES);
        assert_eq!(t.moved(2, (420.0, 100.0), &l), [Out::Look(20.0, 0.0)]);
        assert!(t.held(Control::Stick));
        assert_eq!(t.release_all(), [Out::Move(0.0, 0.0)]);
    }

    #[test]
    fn stick_settings_survive_a_round_trip() {
        let mut s = stick_settings();
        s.stick_scale = 1.25;
        s.stick_opacity = 0.5;
        s.set_placement(Control::Stick, 0.1, 0.6);
        assert_eq!(TouchSettings::parse(&s.serialize()), s);
        // A file written before the stick existed has no such lines, and the default file gains none.
        let old = TouchSettings::parse("opacity:1\nscale:1\n");
        assert_eq!(old.movement, Movement::DPad);
        let default = TouchSettings::default().serialize();
        assert!(!default.contains("movement") && !default.contains("stick:"));
        // Resetting the layout keeps the mode and the stick's own size and opacity.
        s.reset_layout();
        assert_eq!(s.movement, Movement::Stick);
        assert_eq!((s.stick_scale, s.stick_opacity), (1.25, 0.5));
        assert!(!s.placements.contains_key(&Control::Stick));
    }

    #[test]
    fn bad_stick_values_fall_back_to_the_defaults() {
        let s = TouchSettings::parse("movement:nope\nstick:9:nope\n");
        assert_eq!(s.movement, Movement::DPad);
        assert_eq!(s.stick_scale, MAX_SCALE);
        assert_eq!(s.stick_opacity, 1.0);
    }
}
