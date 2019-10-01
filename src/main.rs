#[macro_use]
extern crate serde_derive;

use std::io::{Read, Write};
use std::fs::File;
use std::error::Error;

use std::cmp;
use rand::{Rng};
use rand::distributions::{WeightedIndex, Distribution};

use tcod::colors::{self, Color};
use tcod::console::*;
use tcod::map::{FovAlgorithm, Map as FovMap};
use tcod::input::{self, Event, Key, Mouse};

const SCREEN_WIDTH: i32 = 80;
const SCREEN_HEIGHT: i32 = 50;
const LIMIT_FPS: i32 = 20;

const MAP_WIDTH: i32 = 80;
const MAP_HEIGHT: i32 = 43;

const COLOR_DARK_WALL: Color = Color { r: 0, g: 0, b: 100 };
const COLOR_LIGHT_WALL: Color = Color {r: 130, g: 110, b: 50};
const COLOR_DARK_GROUND: Color = Color {r: 50, g: 50, b: 150};
const COLOR_LIGHT_GROUND: Color = Color {r: 200, g: 180, b: 50};

const ROOM_MAX_SIZE: i32 = 10;
const ROOM_MIN_SIZE: i32 = 6;
const MAX_ROOMS: i32 = 30;

const FOV_ALGO: FovAlgorithm = FovAlgorithm::Basic;
const FOV_LIGHT_WALLS: bool = true;
const TORCH_RADIUS: i32 = 5;

const PLAYER: usize = 0;

const BAR_WIDTH: i32 = 20;
const PANEL_HEIGHT: i32 = 7;
const PANEL_Y: i32 = SCREEN_HEIGHT - PANEL_HEIGHT;
const MSG_X: i32 = BAR_WIDTH + 2;
const MSG_WIDTH: i32 = SCREEN_WIDTH - BAR_WIDTH - 2;
const MSG_HEIGHT: usize = PANEL_HEIGHT as usize - 1;

const INVENTORY_WIDTH:i32 = 50;

const HEAL_AMOUNT:i32 = 40;
const ATTACK_BUFF:i32 = 2;
const PLAYER_MAX_ATTACK:i32 = 9;
const LIGHTNING_DAMAGE:i32 = 40;
const LIGHTNING_RANGE:i32 = 5;

const LEVEL_UP_BASE: i32 = 200;
const LEVEL_UP_FACTOR: i32 = 150;

const MONSTER_LEVEL_UP_BASE: u32 = 20;
const MONSTER_ATTACK_LEVEL_UP_BASE: u32 = 5;
const MONSTER_DEFENSE_LEVEL_UP_BASE: u32 = 5;
const MONSTER_LEVEL_UP_FACTOR: i32 = 2;

const LEVEL_SCREEN_WIDTH: i32 = 40;
const CHARACTER_SCREEN_WIDTH: i32 = 30;


#[derive(Debug, Serialize, Deserialize)]
struct Object {
    x: i32,
    y: i32,
    char: char,
    color: Color,
    name: String,
    blocks: bool,
    alive: bool,
    fighter: Option<Fighter>,
    ai: Option<Ai>,
    item:Option<Item>,
    level: i32,
    equipment: Option<Equipment>,
    always_visible: bool,
}

struct Tcod {
    root: Root,
    con: Offscreen,
    panel: Offscreen,
    fov: FovMap,
    mouse: Mouse
}

#[derive(Serialize, Deserialize)]
struct Game {
    map: Map,
    log: Messages,
    inventory: Vec<Object>,
    dungeon_level: u32,
}

struct Transition {
    level: u32,
    value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Equipment {
    slot: Slot,
    equipped: bool,
    max_hp_bonus: i32,
    power_bonus: i32,
    defense_bonus: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Slot {
    RightHand,
    LeftHand,
    Chest
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Slot::LeftHand => write!(f, "left hand"),
            Slot::RightHand => write!(f, "right hand"),
            Slot::Chest => write!(f, "chest"),
        }
    }
}

trait MessageLog {
    fn add<T: Into<String>>(&mut self, message: T, color: Color);
}

impl MessageLog for Vec<(String, Color)> {
    fn add<T: Into<String>>(&mut self, message: T, color: Color) {
        self.push((message.into(), color));
    }
}

impl Object {

    pub fn new(x: i32, y: i32, char: char, name: &str, color: Color, blocks: bool,) -> Self {
        Object{
            x,
            y,
            char,
            color,
            name: name.into(),
            blocks,
            alive:false,
            fighter: None,
            ai: None,
            item: None,
            level: 1,
            equipment: None,
            always_visible: false,
        }
    }

    pub fn draw(&self, con: &mut dyn Console){
        con.set_default_foreground(self.color);
        con.put_char(self.x, self.y, self.char, BackgroundFlag::None);
    }

    pub fn pos(&self) -> (i32, i32){
        (self.x, self.y)
    }

    pub fn set_pos(&mut self, x: i32, y: i32){
        self.x = x;
        self.y = y;
    }

    pub fn distance_to(&self, other: &Object) -> f32 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        ((dx.pow(2) + dy.pow(2)) as f32).sqrt()
    }

    pub fn take_damage(&mut self, damage: i32, game: &mut Game) -> Option<i32> {

        //borrowed
        if let Some(fighter) = self.fighter.as_mut() {
            if damage > 0 {
                fighter.hp -= damage;
            }
        }

        //Copy
        if let Some(fighter) = self.fighter {
            if fighter.hp <= 0 {
                self.alive = false;
                fighter.on_death.callback(self, game);
                return Some(fighter.xp);
            }
        }
        None
    }

    pub fn attack(&mut self, target: &mut Object, game: &mut Game) {

        let mut damage = self.power(game) - target.defense(game);

        if rand::random::<f32>() <0.1 {
            damage = -1;
        }

        if damage > 0 {
            game.log.add(format!("{} attacks {} for {} hit points.", self.name, target.name, damage), colors::WHITE);

            if let Some(xp) = target.take_damage(damage, game) {
                self.fighter.as_mut().unwrap().xp += xp;
            }

        } else if damage < 0 {
            game.log.add(format!("{} miss {}.", self.name, target.name), colors::ORANGE);
        } else {
            game.log.add(format!("{} attacks {} but it has no effect!",self.name, target.name), colors::WHITE);
        }
    }

    pub fn cast(&mut self, _tcod: &mut Tcod, cast_type: &str, amount: i32, game: &mut Game) {
        let max_hp = self.max_hp(game);
        match cast_type.as_ref() {

            "heal" =>

                if let Some(ref mut fighter) = self.fighter {
                    fighter.hp += amount;
                    if fighter.hp > max_hp {
                        fighter.hp = max_hp;
                    }
                }

//            "attack_buff" =>
//                if let Some(ref mut fighter) = self.fighter {
//                    self.power(game) += amount;
////                    if fighter.power >= PLAYER_MAX_ATTACK{
////                        fighter.power = PLAYER_MAX_ATTACK;
////                    }
//                }

            _ => ()

        }
    }

    pub fn equip (&mut self, log: &mut Vec<(String, Color)> ){

        if self.item.is_none(){
            log.add(
                format!("Can't equip {:?} because it's not an Item.", self),
                colors::RED,
            );
            return;
        }

        if let Some(ref mut equipment) = self.equipment {
            if !equipment.equipped {
                equipment.equipped = true;
                log.add(
                    format!("Equipped {} on {:?}.", self.name, equipment.slot),
                    colors::LIGHT_GREEN,
                );
            }
        } else {
            log.add(
                format!("Can't equip {:?} because it's not an Equipment.", self),
                colors::RED,
            );
        }
    }

    pub fn dequip(&mut self, log: &mut Vec<(String, Color)>) {
        if self.item.is_none() {
            log.add(
                format!("Can't dequip {:?} because it's not an Item.", self),
                colors::RED,
            );
            return;
        };
        if let Some(ref mut equipment) = self.equipment {
            if equipment.equipped {
                equipment.equipped = false;
                log.add(
                    format!("Dequipped {} from {:?}.", self.name, equipment.slot),
                    colors::LIGHT_YELLOW,
                );
            }
        } else {
            log.add(
                format!("Can't dequip {:?} because it's not an Equipment.", self),
                colors::RED,
            );
        }
    }

    pub fn power(&self, game: &Game) -> i32 {
        let base_power = self.fighter.map_or(0, |f| f.base_power);
        let bonus: i32 = self
            .get_all_equipped(game)
            .iter()
            .map(|e| e.power_bonus)
            .sum();
        base_power + bonus
    }

    pub fn defense(&self, game: &Game) -> i32 {
        let base_defense = self.fighter.map_or(0, |f| f.base_defense);
        let bonus: i32 = self
            .get_all_equipped(game)
            .iter()
            .map(|e| e.defense_bonus)
            .sum();
        base_defense + bonus
    }

    pub fn get_all_equipped(&self, game: &Game) -> Vec<Equipment> {
        if self.name == "player" {
            game.inventory
                .iter()
                .filter(|item| item.equipment.map_or(false, |e| e.equipped))
                .map(|item| item.equipment.unwrap())
                .collect()
        } else {
            vec![] //noequippement objects
        }
    }

    pub fn max_hp(&self, game: &Game) -> i32 {
        let base_max_hp = self.fighter.map_or(0, |f| f.base_max_hp);
        let bonus: i32 = self
            .get_all_equipped(game)
            .iter()
            .map(|e| e.max_hp_bonus)
            .sum();
        base_max_hp + bonus
    }

}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct Tile {
    blocked: bool,
    block_sight: bool,
    explored: bool,
}

impl Tile {
    pub fn empty() -> Self{
        Tile{blocked: false, block_sight: false, explored: false}
    }

    pub fn wall() -> Self{
        Tile{blocked: true, block_sight: true, explored: false}
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum Item {
    Heal,
    AttackBuff,
    Lightning,
    Sword,
    Chest,
    Targe,
}

enum UseResult {
    UsedUp,
    Cancelled,
    UseAndKept,
    UseAndTakeTurn
}

type Map = Vec<Vec<Tile>>;
type Messages = Vec<(String, Color)>;


fn move_by(id: usize, dx: i32, dy: i32, map: &Map, objects: &mut[Object]){

    let (x,y) = objects[id].pos();

    if !is_blocked(x + dx, y + dy, map, objects){
        objects[id].set_pos(x + dx, y + dy);
    }

}

fn from_dungeon_level(table: &[Transition], level: u32) -> u32 {
    table
        .iter()
        .rev()
        .find(|transition| level >= transition.level)
        .map_or(0, |transition| transition.value)
}

fn move_towards(id: usize, target_x: i32, target_y: i32, map: &Map, objects: &mut [Object]) {
    let dx = target_x - objects[id].x;
    let dy = target_y - objects[id].y;
    let distance = ((dx.pow(2) + dy.pow(2)) as f32).sqrt();

    let dx = (dx as f32 / distance).round() as i32;
    let dy = (dy as f32 / distance).round() as i32;

    move_by(id, dx, dy, map, objects);
}

fn mut_two<T>(first_index: usize, second_index: usize, items: &mut [T]) -> (&mut T, &mut T) {
    assert_ne!(first_index, second_index);

    let split_at_index = cmp::max(first_index, second_index);
    let (first_slice, second_slice) = items.split_at_mut(split_at_index);

    if first_index < second_index {
        (&mut first_slice[first_index], &mut second_slice[0])
    } else {
        (&mut second_slice[0], &mut first_slice[second_index])
    }
}

fn menu<T: AsRef<str>>(header: &str, options: &[T], width: i32, root: &mut Root) -> Option<usize>{
    assert!(
        options.len() <= 27,
        "Cannot have a menu with more than 26 options."
    );

    let header_height = if header.is_empty() {
        0
    } else {
        root.get_height_rect(0, 0, width, SCREEN_HEIGHT, header)
    };
    let height = options.len() as i32 + header_height;

    let mut window = Offscreen::new(width,  height);
    window.set_default_foreground(colors::WHITE);
    window.print_rect_ex(
        0,
        0,
        width,
        height,
        BackgroundFlag::None,
        TextAlignment::Left,
        header,
    );

    for(index, option_text) in options.iter().enumerate() {
        let menu_letter = (b'a' + index as u8) as char;
        let text = format!("[{}] - {}", menu_letter, option_text.as_ref());
        window.print_ex(
            0,
            header_height + index as i32,
            BackgroundFlag::None,
            TextAlignment::Left,
            text,
        );
    }

    let x = SCREEN_WIDTH / 2 - width / 2;
    let y = SCREEN_HEIGHT / 2 - height / 2;

    tcod::console::blit(&mut window, (0, 0), (width, height), root, (x, y), 1.0, 0.7);
    root.flush();
    let key = root.wait_for_keypress(true);

    if key.printable.is_alphabetic(){
        let index = key.printable.to_ascii_lowercase() as usize - 'a' as usize;
        if index < options.len() {
            Some(index)
        }else{
            None
        }
    }else{
        None
    }

}

fn inventory_menu(inventory: &[Object], header: &str, root: &mut Root) -> Option<usize> {
    let options = if inventory.len() == 0 {
        vec!["Inventory is empty.".into()]
    } else {
        inventory
            .iter()
            .map(|item| {
                match item.equipment {
                    Some(equipment) if equipment.equipped => {
                        format!("{} (on {})", item.name, equipment.slot)
                    }
                    _ => item.name.clone(),
                }
            })
            .collect()    };

    let inventory_index = menu(header, &options, INVENTORY_WIDTH, root);

    if inventory.len() > 0 {
        inventory_index
    } else {
        None
    }
}

fn cast_heal(tcod: &mut Tcod,_inventory_id: usize, objects: &mut [Object], game: &mut Game) -> UseResult{

    let player = &mut objects[PLAYER];

    if let Some(fighter) = player.fighter {
        if fighter.hp == player.max_hp(game) {
            game.log.add("You are already at full health.", colors::RED);
            return UseResult::Cancelled;
        }
        game.log
            .add("Your wounds start to feel better!", colors::LIGHT_VIOLET);
        objects[PLAYER].cast(tcod, "heal", HEAL_AMOUNT, game);
        return UseResult::UsedUp;
    }
    UseResult::Cancelled
}

fn cast_attack_buff(tcod: &mut Tcod, _inventory_id: usize, objects: &mut [Object], game: &mut Game) -> UseResult{

    if let Some(fighter) = objects[PLAYER].fighter {
        if fighter.base_power >= PLAYER_MAX_ATTACK {
            game.log.add("your attack lvl is too high for this item level", colors::RED);
            return UseResult::Cancelled;
        }
        objects[PLAYER].cast(tcod, "attack_buff", ATTACK_BUFF, game);
        game.log.add(format!("Permanently increase your attack by: {}", ATTACK_BUFF), colors::GREEN);

        return UseResult::UsedUp;
    }
    UseResult::Cancelled
}

fn cast_lightning(
    tcod: &mut Tcod,
    _inventory_id: usize,
    objects: &mut [Object],
    game: &mut Game
) -> UseResult {
    let monster_id = closest_monster(LIGHTNING_RANGE, objects, tcod);
    if let Some(monster_id) = monster_id {

        game.log.add(format!("A lightning bolt strikes the {} with a loud thunder! \
                 The damage is {} hit points.",
            objects[monster_id].name, LIGHTNING_DAMAGE), colors::LIGHT_BLUE,);

        if let Some(xp) = objects[monster_id].take_damage(LIGHTNING_DAMAGE, game) {
            objects[PLAYER].fighter.as_mut().unwrap().xp += xp;
        }

        UseResult::UseAndTakeTurn
    } else {
        game.log.add("No enemy is close enough to strike.", colors::RED);
        UseResult::Cancelled
    }
}

fn toggle_equipment(_tcod: &mut Tcod, inventory_id: usize, _objects: &mut [Object], game: &mut Game) -> UseResult {
    let equipment = match game.inventory[inventory_id].equipment {
        Some(equipment) => equipment,
        None => return UseResult::Cancelled,
    };

    if equipment.equipped == true {
        game.inventory[inventory_id].dequip(&mut game.log);
    }else{
        if let Some(current) = get_equipped_in_slot(equipment.slot, &game.inventory) {
            game.inventory[current].dequip(&mut game.log);
        }
        game.inventory[inventory_id].equip(&mut game.log);
    }

    UseResult::UseAndKept
}

fn get_equipped_in_slot (slot: Slot, inventory: &[Object]) -> Option<usize> {
    for (inventory_id, item) in inventory.iter().enumerate() {
        if item.equipment.as_ref().map_or(false,|e| e.equipped && e.slot == slot){
            return Some(inventory_id)
        }
    }
    None
}

fn level_up(objects: &mut [Object], game: &mut Game, tcod: &mut Tcod){
    let player = &mut objects[PLAYER];
    let level_up_xp = LEVEL_UP_BASE + player.level * LEVEL_UP_FACTOR;

    if player.fighter.as_ref().map_or(0, |f| f.xp) >= level_up_xp {
        player.level += 1;
        game.log.add(format!("You reached level {}!", player.level), colors::YELLOW);

        let fighter = player.fighter.as_mut().unwrap();
        let mut choice = None;
        while choice.is_none() {

            choice = menu(
                "Level up! Choose a stat to raise:\n",
                &[
                    format!("Constitution (+20 HP, from {})", fighter.base_max_hp),
                    format!("Strength (+1 attack, from {})", fighter.base_power),
                    format!("Agility (+1 defense, from {})", fighter.base_defense),
                ],
                LEVEL_SCREEN_WIDTH,
                &mut tcod.root,
            );
        }
        fighter.xp -= level_up_xp;
        match choice.unwrap() {
            0 => {
                fighter.base_max_hp += 20;
                fighter.hp += 20;
            }
            1 => {
                fighter.base_power += 1;
            }
            2 => {
                fighter.base_defense += 1;
            }
            _ => unreachable!(),
        }

    }
}

fn closest_monster(max_range: i32, objects: &mut [Object], tcod: &Tcod) -> Option<usize> {
    let mut closest_enemy = None;
    let mut closest_dist = (max_range + 1) as f32;

    for (id, object) in objects.iter().enumerate() {
        if (id != PLAYER)
            && object.fighter.is_some()
            && object.ai.is_some()
            && tcod.fov.is_in_fov(object.x, object.y)
        {
            let dist = objects[PLAYER].distance_to(object);
            if dist < closest_dist {
                closest_enemy = Some(id);
                closest_dist = dist;
            }
        }
    }
    closest_enemy
}

fn ai_take_turn(monster_id: usize, game: &mut Game, objects: &mut [Object], fov_map: &FovMap) {

    use Ai::*;
    if let Some(ai) = objects[monster_id].ai.take() {
        let new_ai = match ai {
            Basic => ai_basic(monster_id, objects, fov_map, game),
        };
        objects[monster_id].ai = Some(new_ai);
    }
}

fn ai_basic(
    monster_id: usize,
    objects: &mut [Object],
    fov_map: &FovMap,
    game: &mut Game,
) -> Ai {
    let (monster_x, monster_y) = objects[monster_id].pos();
    if fov_map.is_in_fov(monster_x, monster_y) {
        if objects[monster_id].distance_to(&objects[PLAYER]) >= 2.0 {
            let (player_x, player_y) = objects[PLAYER].pos();
            move_towards(monster_id, player_x, player_y, &game.map, objects);
        } else if objects[PLAYER].fighter.map_or(false, |f| f.hp > 0) {
            let (monster, player) = mut_two(monster_id, PLAYER, objects);
            monster.attack(player, game);
        }
    }
    Ai::Basic
}

fn render_bar(
    panel: &mut Offscreen,
    x: i32,
    y: i32,
    total_width: i32,
    name: &str,
    value: i32,
    maximum: i32,
    bar_color: Color,
    back_color: Color,
){
    let bar_width = (value as f32 / maximum as f32 * total_width as f32) as i32;

    panel.set_default_background(back_color);
    panel.rect(x, y, total_width, 1, false, BackgroundFlag::Screen);

    panel.set_default_background(bar_color);
    if bar_width > 0 {
        panel.rect(x, y, bar_width, 1, false, BackgroundFlag::Screen);
    }

    panel.set_default_foreground(colors::WHITE);
    panel.print_ex(
        x + total_width /2,
        y,
        BackgroundFlag::None,
        TextAlignment::Center,
        &format!("{}: {}/{}", name, value, maximum),
    );
}

fn is_blocked(x: i32, y: i32, map: &Map, objects: &[Object]) -> bool {

    if map[x as usize][y as usize].blocked {
        return true;
    }

    objects.iter().any(|object |{
        object.blocks && object.pos() == (x, y)
    })

}

fn pick_item_up(object_id:usize, objects: &mut Vec<Object>, game: &mut Game){
    if game.inventory.len() >= 26 {
        game.log.add(format!("Your inventory is full, you cannot pick up {}",objects[object_id].name),colors::RED);

    }else{
        let item = objects.swap_remove(object_id);
        game.log.add(format!("you pick up a {}", item.name),colors::GREEN);

        game.inventory.push(item);
    }
}

fn drop_item(inventory_id: usize, objects: &mut Vec<Object>, game: &mut Game){

    let mut item = game.inventory.remove(inventory_id);
    if item.equipment.is_some() {
        item.dequip(&mut game.log);
    }
    item.set_pos(objects[PLAYER].x, objects[PLAYER].y);
    game.log.add(format!("You dropped a {}", item.name), colors::YELLOW);
    objects.push(item);
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect {
            x1: x,
            y1: y,
            x2: x + w,
            y2: y + h,
        }
    }

    pub fn center(&self) -> (i32, i32) {
        let center_x = (self.x1 + self.x2) / 2;
        let center_y = (self.y1 + self.y2) / 2;
        (center_x, center_y)
    }

    pub fn intersect_with(&self, other: &Rect) -> bool {
        (self.x1 <= other.x2)
            && (self.x2 >= other.x1)
            && (self.y1 <= other.y2)
            && (self.y2 >= other.y1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PlayerAction {
    TookTurn,
    DidntTakeTurn,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum DeathCallback {
    Player,
    Monster,
}

impl DeathCallback {
    fn callback(self, object: &mut Object, game: &mut Game) {
        use DeathCallback::*;
        let callback: fn(&mut Object, game: &mut Game) = match self {
            Player => player_death,
            Monster => monster_death,
        };

        callback(object, game);
    }
}

fn player_death(player: &mut Object, game: &mut Game) {

    game.log.add("You died, see you another time!", colors::RED);
    player.char = '%';
    player.color = colors::LIGHTER_RED;
}

fn monster_death(monster: &mut Object, game: &mut Game) {

    game.log.add(format!("PAF! {} is dead! You gain {}", monster.name, monster.fighter.unwrap().xp), colors::ORANGE);
    monster.char = '%';
    monster.color = colors::DARK_RED;
    monster.blocks = false;
    monster.fighter = None;
    monster.ai = None;
    monster.name = format!("remains of {}", monster.name);
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Fighter {
    base_max_hp: i32,
    hp: i32,
    base_defense: i32,
    base_power: i32,
    on_death: DeathCallback,
    xp: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Ai {
    Basic
}

fn create_room(room: Rect, map: &mut Map)
{
    for x in (room.x1 + 1)..room.x2{
        for y in (room.y1 + 1)..room.y2 {
            map[x as usize][y as usize] = Tile::empty();
        }
    }
}

fn create_h_tunnel(x1: i32, x2: i32, y: i32, map: &mut Map) {
    for x in cmp::min(x1, x2)..(cmp::max(x1, x2) + 1) {
        map[x as usize][y as usize] = Tile::empty();
    }
}

fn create_v_tunnel(y1: i32, y2: i32, x: i32, map: &mut Map) {
    for y in cmp::min(y1, y2)..(cmp::max(y1, y2) + 1) {
        map[x as usize][y as usize] = Tile::empty();
    }
}

fn place_object(room: Rect, map: &Map, objects: &mut Vec<Object>, level: u32){

    let max_monsters = from_dungeon_level(
        &[
            Transition { level: 1, value: 2 },
            Transition { level: 4, value: 3 },
            Transition { level: 6, value: 5 },
            Transition { level: 10, value: 7 },
        ],
        level,
    );

    let poulet_chance = from_dungeon_level(
        &[
            Transition {level: 1, value: 60,},
            Transition {level: 2, value: 30,},
            Transition {level: 4, value: 0,},
        ],
        level,
    );

    let orc_chance = from_dungeon_level(
        &[
            Transition {level: 2, value: 30,},
            Transition {level: 5, value: 30,},
        ],
        level,
    );

    let troll_chance = from_dungeon_level(
        &[
            Transition {level: 4, value: 15,},
            Transition {level: 5, value: 30,},
            Transition {level: 7, value: 60,},
        ],
        level,
    );

    let boss_chance = from_dungeon_level(
        &[
            Transition {level: 3, value: 10,},
            Transition {level: 5, value: 15,},
            Transition {level: 7, value: 20,},
        ],
        level,
    );

    let num_monsters = rand::thread_rng().gen_range(0, max_monsters + 1);

    for _ in 0..num_monsters {
        let x = rand::thread_rng().gen_range(room.x1 + 1, room.x2);
        let y = rand::thread_rng().gen_range(room.y1 + 1, room.y2);


        let choices = ["poulet","orc", "troll", "boss"];
        let weights = [poulet_chance, orc_chance,   troll_chance,   boss_chance];
        let monster_choice = WeightedIndex::new(&weights).unwrap();


        if !is_blocked(x, y, map, objects){

            let mut monster = match choices[monster_choice.sample(&mut rand::thread_rng())] {
                "orc" => {
                    let mut orc= Object::new(x, y, 'o', "orc", colors::LIGHT_GREEN, true);
                    let hp_multiplier = ((MONSTER_LEVEL_UP_BASE as i32 + level as i32 ) / MONSTER_LEVEL_UP_FACTOR) as i32;
                    let attack_multiplier = ((MONSTER_ATTACK_LEVEL_UP_BASE as i32 + level as i32 ) / MONSTER_LEVEL_UP_FACTOR) as i32;
                    let defense_multiplier = ((MONSTER_DEFENSE_LEVEL_UP_BASE as i32 + level as i32 ) / MONSTER_LEVEL_UP_FACTOR) as i32;
                    orc.fighter = Some(Fighter {
                        base_max_hp: 10 + hp_multiplier as i32,
                        hp: 10 + hp_multiplier as i32,
                        base_defense: defense_multiplier,
                        base_power: 4 + attack_multiplier as i32,
                        on_death: DeathCallback::Monster,
                        xp:35
                    });
                    orc.ai = Some(Ai::Basic);
                    orc
                }
                "poulet" => {
                    let mut poulet= Object::new(x, y, 'p', "poulet", colors::GREY, true);
                    poulet.fighter = Some(Fighter {
                        base_max_hp: 15,
                        hp: 15,
                        base_defense: 0,
                        base_power: 3,
                        on_death: DeathCallback::Monster,
                        xp:20
                    });
                    poulet.ai = Some(Ai::Basic);
                    poulet
                }
                "troll" => {
                    let hp_multiplier = ((MONSTER_LEVEL_UP_BASE as i32 + level as i32 ) / MONSTER_LEVEL_UP_FACTOR) as i32;
                    let attack_multiplier = ((MONSTER_ATTACK_LEVEL_UP_BASE as i32 + level as i32 ) / MONSTER_LEVEL_UP_FACTOR) as i32;
                    let defense_multiplier = ((MONSTER_DEFENSE_LEVEL_UP_BASE as i32 + level as i32 ) / MONSTER_LEVEL_UP_FACTOR) as i32;
                    let mut troll = Object::new(x, y, 'T', "troll", colors::LIGHT_GREEN, true);
                    troll.fighter = Some(Fighter {
                        base_max_hp: 15 + hp_multiplier as i32,
                        hp: 15 + hp_multiplier as i32,
                        base_defense: 1 + defense_multiplier,
                        base_power: 5 + attack_multiplier,
                        on_death: DeathCallback::Monster,
                        xp:55
                    });
                    troll.ai = Some(Ai::Basic);
                    troll
                }
                "boss" => {
                    let hp_multiplier = ((MONSTER_LEVEL_UP_BASE as i32 + level as i32 ) / MONSTER_LEVEL_UP_FACTOR) as i32;
                    let attack_multiplier = ((MONSTER_ATTACK_LEVEL_UP_BASE as i32 + level as i32 ) / MONSTER_LEVEL_UP_FACTOR) as i32;
                    let defense_multiplier = ((MONSTER_DEFENSE_LEVEL_UP_BASE as i32 + level as i32 ) / MONSTER_LEVEL_UP_FACTOR) as i32;
                    let mut boss = Object::new(x, y, 'W', "BOSS", colors::RED, true);
                    boss.fighter = Some(Fighter{
                        base_max_hp: 60 + hp_multiplier as i32,
                        hp: 60 + hp_multiplier as i32,
                        base_defense: 4 + defense_multiplier,
                        base_power: 8 +attack_multiplier,
                        on_death: DeathCallback::Monster,
                        xp:110
                    });
                    boss.ai = Some(Ai::Basic);
                    boss
                }
                _ => unreachable!(),
            };

            monster.alive= true;
            objects.push(monster);
        }

    }

    let max_items = from_dungeon_level(
        &[
            Transition { level: 1, value: 1 },
            Transition { level: 4, value: 2 },
        ],
        level,
    );

    let num_items = rand::thread_rng().gen_range(0, max_items +1);

    for _ in 0..num_items {
        let x = rand::thread_rng().gen_range(room.x1 +1 , room.x2);
        let y = rand::thread_rng().gen_range(room.y1 +1 , room.y2);

        let item_chances = [Item::Heal, Item::Lightning, Item::Sword, Item::Targe, Item::Chest];
        let weights = [
            35,
            from_dungeon_level(
                &[Transition {level: 4, value: 10,}],
                level,
            ),
            from_dungeon_level(
                &[Transition {level: 3,value: 5,}],
                level,
            ),
            from_dungeon_level(
                &[Transition {level: 6,value: 5,}],
                level,
            ),
            from_dungeon_level(
                &[Transition {level: 8,value: 5,}],
                level,
            ),
        ];
        let item_choice = WeightedIndex::new(&weights).unwrap();


        if !is_blocked(x, y, map, objects){

            let mut item = match item_chances[item_choice.sample(&mut rand::thread_rng())] {
                Item::Heal => {
                    let mut object = Object::new(x, y, '!', "healing potion", colors::VIOLET, false);
                    object.item = Some(Item::Heal);
                    object
                }
                Item::Lightning => {
                    let mut object = Object::new(x, y, '#', "scroll of lightning bolt", colors::LIGHT_YELLOW, false, );
                    object.item = Some(Item::Lightning);
                    object
                }
                Item::AttackBuff => {
                    let mut object = Object::new(x, y, '+', "attack scroll", colors::VIOLET, false);
                    object.item = Some(Item::AttackBuff);
                    object
                }
                Item::Sword => {
                    let mut object = Object::new(x, y, '/', "sword", colors::SKY, false);
                    object.item = Some(Item::Sword);
                    object.equipment = Some(Equipment{equipped: false, slot: Slot::RightHand, power_bonus: 3, defense_bonus: 0, max_hp_bonus: 0});
                    object
                }
                Item::Chest => {
                    let mut object = Object::new(x, y, '░', "chainmail armor", colors::COPPER, false);
                    object.item = Some(Item::Chest);
                    object.equipment = Some(Equipment{equipped: false, slot: Slot::Chest, power_bonus: 0, defense_bonus: 2, max_hp_bonus: 10});
                    object
                }Item::Targe => {
                    let mut object = Object::new(x, y, '◙', "targe", colors::DARK_HAN, false);
                    object.item = Some(Item::Targe);
                    object.equipment = Some(Equipment{equipped: false, slot: Slot::LeftHand, power_bonus: 0, defense_bonus: 1, max_hp_bonus: 0});
                    object
                }
            };
            item.always_visible = true;
            objects.push(item);

        }
    }
}

fn use_item (tcod: &mut Tcod, inventory_id: usize, object: &mut [Object], game: &mut Game) -> PlayerAction{
    use Item::*;
    use PlayerAction::*;

    if let Some(item) = game.inventory[inventory_id].item {
        let on_use = match item {
            Heal => cast_heal,
            AttackBuff => cast_attack_buff,
            Lightning => cast_lightning,
            Sword => toggle_equipment,
            Chest => toggle_equipment,
            Targe => toggle_equipment,
        };

        match on_use(tcod, inventory_id, object, game){
            UseResult::UsedUp => {
                &mut game.inventory.remove(inventory_id);
                DidntTakeTurn
            }
            UseResult::UseAndTakeTurn => {
                &mut game.inventory.remove(inventory_id);
                TookTurn
            },
            UseResult::UseAndKept => {
                DidntTakeTurn
            },
            UseResult::Cancelled => {
                game.log.add("Cancelled", colors::WHITE);
                DidntTakeTurn
            }
        }
    } else {
        game.log.add(format!("The {} cannot be used.", game.inventory[inventory_id].name),colors::RED);
        DidntTakeTurn
    }
}

fn make_map(objects: &mut Vec<Object>, level: u32) -> Map {

    let mut map = vec![vec![Tile::wall(); MAP_HEIGHT as usize]; MAP_WIDTH as usize];
    assert_eq!(&objects[PLAYER] as *const _, &objects[0] as *const _);
    objects.truncate(1);

    let mut rooms = vec![];

    for _ in 0..MAX_ROOMS {
        let w = rand::thread_rng().gen_range(ROOM_MIN_SIZE, ROOM_MAX_SIZE + 1);
        let h = rand::thread_rng().gen_range(ROOM_MIN_SIZE, ROOM_MAX_SIZE + 1);

        let x = rand::thread_rng().gen_range(0, MAP_WIDTH - w);
        let y = rand::thread_rng().gen_range(0, MAP_HEIGHT - h);

        let new_room = Rect::new(x, y, w, h);

        let failed = rooms
            .iter()
            .any(|other_room|new_room.intersect_with(other_room));

        if !failed {
            create_room(new_room, &mut map);
            place_object(new_room, &map, objects, level);
            let (new_x, new_y) = new_room.center();
            if rooms.is_empty() {
                objects[PLAYER].set_pos(new_x, new_y);
            }else{
                let (prev_x, prev_y) = rooms[rooms.len() - 1].center();

                if rand::random() {
                    create_h_tunnel(prev_x, new_x, prev_y, &mut map);
                    create_v_tunnel(prev_y, new_y, new_x, &mut map);
                } else {
                    create_v_tunnel(prev_y, new_y, prev_x, &mut map);
                    create_h_tunnel(prev_x, new_x, new_y, &mut map);
                }
            }


            rooms.push(new_room);
        }
    }

    let (last_room_x, last_room_y) = rooms[rooms.len() - 1].center();
    let mut stairs = Object::new(
        last_room_x,
        last_room_y,
        '<',
        "stairs",
        colors::WHITE,
        false,
    );
    stairs.always_visible = true;
    objects.push(stairs);

    map
}

fn render_all(
    tcod: &mut Tcod,
    objects: &[Object],
    game: &mut Game,
    fov_recompute: bool,
){
    if fov_recompute {
        let player = &objects[PLAYER];
        tcod.fov.compute_fov(player.x, player.y, TORCH_RADIUS, FOV_LIGHT_WALLS, FOV_ALGO);
    }

    for y in 0..MAP_HEIGHT{
        for x in 0..MAP_WIDTH{

            let visible= tcod.fov.is_in_fov(x, y);
            let wall = game.map[x as usize][y as usize].block_sight;
            let color = match (visible, wall){
                (false, true) => COLOR_DARK_WALL,
                (false, false) => COLOR_DARK_GROUND,
                (true, false) => COLOR_LIGHT_GROUND,
                (true, true) => COLOR_LIGHT_WALL
            };

            let explored = &mut game.map[x as usize][y as usize].explored;

            if visible{
                *explored = true;
            }

            if *explored {
                tcod.con.set_char_background(x, y, color, BackgroundFlag::Set);
            }
        }
    }

    let mut to_draw: Vec<_> = objects
        .iter()
        .filter(|o| {
            tcod.fov.is_in_fov(o.x, o.y)
                || (o.always_visible && game.map[o.x as usize][o.y as usize].explored)
        })
        .collect();
    to_draw.sort_by(|o1, o2| o1.blocks.cmp(&o2.blocks));

    for object in &to_draw {
        object.draw(&mut tcod.con);
    }

    if let Some(_fighter) = objects[PLAYER].fighter {
        tcod.panel.set_default_background(colors::BLACK);
        tcod.panel.clear();

        let mut y = MSG_HEIGHT as i32;
        for &(ref msg, color) in game.log.iter().rev() {
            let msg_height = tcod.panel.get_height_rect(MSG_X, y, MSG_WIDTH, 0, msg);
            y -= msg_height;

            if y < 0 {
                break;
            }

            tcod.panel.set_default_foreground(color);
            tcod.panel.print_rect(MSG_X, y, MSG_WIDTH, 0, msg);
        }


        let hp = objects[PLAYER].fighter.map_or(0,|f |f.hp);
        let max_hp = objects[PLAYER].fighter.map_or(0,|f |f.base_max_hp);
        let attack = objects[PLAYER].fighter.map_or(0,|f |f.base_power);
        let defense = objects[PLAYER].fighter.map_or(0,|f |f.base_defense);

        render_bar(&mut tcod.panel, 1, 1, BAR_WIDTH, "HP", hp, max_hp, colors::LIGHT_RED, colors::DARKER_RED);

        tcod.panel.print_ex(
            1,
            5,
            BackgroundFlag::None,
            TextAlignment::Left,
            format!("Dungeon level: {}", game.dungeon_level),
        );

        tcod.panel.set_default_foreground(colors::LIGHT_GREY);
        tcod.panel.print_ex(
            1,
            0,
            BackgroundFlag::None,
            TextAlignment::Left,
            get_names_under_mouse(tcod.mouse, objects, &tcod.fov)
        );

        tcod.panel.set_default_foreground(colors::LIGHT_AZURE);
        tcod.panel.print_ex(
            1,
            3,
            BackgroundFlag::None,
            TextAlignment::Left,
            format!("attack: {}", attack)
        );

        tcod.panel.set_default_foreground(colors::LIGHT_AZURE);
        tcod.panel.print_ex(
            1,
            4,
            BackgroundFlag::None,
            TextAlignment::Left,
            format!("defense: {}", defense)
        );


        blit(
            &mut tcod.panel,
            (0, 0),
            (SCREEN_WIDTH, SCREEN_HEIGHT),
            &mut tcod.root,
            (0, PANEL_Y),
            1.0,
            1.0,
        )
    }


    blit(
        &mut tcod.con,
        (0, 0),
        (MAP_WIDTH, MAP_HEIGHT),
        &mut tcod.root,
        (0, 0),
        1.0,
        1.0,
    );

}

fn get_names_under_mouse(mouse: Mouse, objects: &[Object], fov_map: &FovMap) -> String {
    let (x, y) = (mouse.cx as i32, mouse.cy as i32);

    let names = objects
        .iter()
        .filter(|obj |{obj.pos() == (x,y) && fov_map.is_in_fov(obj.x, obj.y)})
        .map(|obj |obj.name.clone())
        .collect::<Vec<_>>();

    return names.join(", ");
}



fn handle_keys(key: Key, tcod: &mut Tcod, objects: &mut Vec<Object>, game: &mut Game) -> PlayerAction {
    use tcod::input::KeyCode::*;
    use PlayerAction::*;

    let player_alive = objects[PLAYER].alive;

    match (key, player_alive ) {

        (
            Key {
                code: Enter,
                alt: true,
                ..
            },
            _,
        ) => {
            let fullscreen: bool = tcod.root.is_fullscreen();
            tcod.root.set_fullscreen(!fullscreen);
            DidntTakeTurn
        }

        (Key {code: Escape, ..}, _, )=> Exit,

        (Key {code: Up,..}, true) => {
            player_move_or_attack(0, -1, objects, game);
            TookTurn
        },
        (Key {code: Down,..}, true) => {
            player_move_or_attack(0, 1, objects, game);
            TookTurn
        },
        (Key {code: Left,..}, true) => {
            player_move_or_attack(-1, 0, objects, game);
            TookTurn
        },
        (Key {code: Right,..}, true) => {
            player_move_or_attack(1, 0, objects, game);
            TookTurn
        },
        (Key {printable: 'f',..}, true) => {
            let item_id = objects
                .iter()
                .position(|object |object.pos() == objects[PLAYER].pos() && object.item.is_some());

            if let Some(item_id) = item_id {
                pick_item_up(item_id, objects, game);
            }
            DidntTakeTurn
        },
        (Key { printable: 'i', .. }, true) => {
            let inventory_index = inventory_menu(
                &game.inventory,
                "Press the key next to an item to use it, or any other to cancel.\n",
                &mut tcod.root);

            if let Some(inventory_index) = inventory_index {
                return use_item(tcod, inventory_index, objects, game);
            }
            DidntTakeTurn
        },
        (Key { code: Spacebar, .. }, true) => {
            let player_on_stairs = objects
                .iter()
                .any(|object| object.pos() == objects[PLAYER].pos() && object.name == "stairs");
            if player_on_stairs {
                next_level(tcod, objects, game);
            }
            DidntTakeTurn
        },
        (Key { code: Tab, .. }, true) => {

            let player = &objects[PLAYER];
            let level = player.level;
            let level_up_xp = LEVEL_UP_BASE + player.level * LEVEL_UP_FACTOR;
            if let Some(fighter) = player.fighter.as_ref() {
                let msg = format!(
                    "Character information

Level: {}
Experience: {}
Experience to level up: {}

Maximum HP: {}
Attack: {}
Defense: {}",
                    level, fighter.xp, level_up_xp, player.max_hp(game), player.power(game), player.defense(game)
                );
                msgbox(&msg, CHARACTER_SCREEN_WIDTH, &mut tcod.root);
            }

            DidntTakeTurn
        }
        (Key { printable: 'd', .. }, true) => {
            let inventory_index = inventory_menu(
                &game.inventory,
                "Press the key next to an item to drop it, or any other to cancel.\n'",
                &mut tcod.root,
            );
            if let Some(inventory_index) = inventory_index {
                drop_item(inventory_index, objects, game);
            }
            DidntTakeTurn
        }

        _ => DidntTakeTurn
    }
}

fn player_move_or_attack(dx: i32, dy: i32, objects: &mut [Object], game: &mut Game){
    let x = objects[PLAYER].x + dx;
    let y = objects[PLAYER].y + dy;

    let target_id = objects.iter().position(|object |object.fighter.is_some() && object.pos() == (x, y));

    match target_id {
        Some(target_id) => {
            let (player, target) = mut_two(PLAYER, target_id, objects);
            player.attack(target, game);
        }
        None => {
            move_by(PLAYER, dx, dy, &game.map, objects);
        }
    }
}

fn next_level(tcod: &mut Tcod, objects: &mut Vec<Object>, game: &mut Game) {
    game.log.add(
        "You take a moment to rest.",
        colors::VIOLET,
    );
    let heal_hp = objects[PLAYER].max_hp(game) / 2;
    objects[PLAYER].cast(tcod, "heal", heal_hp, game);

    game.log.add(
        "After a rare moment of peace, you going further in the dungeon.. As always",
        colors::RED,
    );
    game.dungeon_level += 1;
    game.map = make_map(objects, game.dungeon_level);
    initialise_fov(&game.map, tcod);
}

fn initialise_fov(map: &Map, tcod: &mut Tcod) {
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            tcod.fov.set(
                x,
                y,
                !map[x as usize][y as usize].block_sight,
                !map[x as usize][y as usize].blocked,
            );
        }
    }
    tcod.con.clear();
}

fn save_game(objects: &[Object], game: &Game) -> Result<(), Box<Error>> {
    let save_data = serde_json::to_string(&(objects, game))?;
    let mut file = File::create("savegame")?;
    file.write_all(save_data.as_bytes())?;
    Ok(())
}

fn load_game() -> Result<(Vec<Object>, Game), Box<dyn Error>> {
    let mut json_save_state = String::new();
    let mut file = File::open("savegame")?;
    file.read_to_string(&mut json_save_state)?;
    let result = serde_json::from_str::<(Vec<Object>, Game)>(&json_save_state)?;
    Ok(result)
}

fn new_game(tcod: &mut Tcod) -> (Vec<Object>, Game) {
    let mut player: Object = Object::new(0,0,'@', "player", colors::WHITE, true);
    player.fighter = Some(Fighter {
        base_max_hp: 100,
        hp: 100,
        base_defense: 1,
        base_power: 4,
        on_death: DeathCallback::Player,
        xp:0
    });
    player.alive= true;

    let mut objects = vec![player];

    let mut game = Game {
        map: make_map(&mut objects, 1),
        log: vec![],
        inventory: vec![],
        dungeon_level: 1
    };

    let mut dagger = Object::new(0, 0, '-', "dagger", colors::SKY, false);
    dagger.item = Some(Item::Sword);
    dagger.equipment = Some(Equipment {
        equipped: true,
        slot: Slot::LeftHand,
        max_hp_bonus: 0,
        defense_bonus: 0,
        power_bonus: 3
    });
    game.inventory.push(dagger);

    initialise_fov(&game.map, tcod);

    game.log.add("Welcome stranger, brace yourself, you're alone now..",colors::RED);

    (objects, game)
}

fn play_game(objects: &mut Vec<Object>, game: &mut Game, tcod: &mut Tcod) {

    let mut previous_player_position = (-1, -1);
    let mut key = Default::default();

    while !tcod.root.window_closed(){
        tcod.con.clear();

        match input::check_for_event(input::MOUSE | input::KEY_PRESS){
            Some ((_, Event::Mouse(m))) => tcod.mouse = m,
            Some ((_, Event::Key(k))) => key = k,
            _ => key = Default::default(),
        }

        let fov_recompute = previous_player_position != (objects[PLAYER].pos());
        render_all(tcod, &objects, game, fov_recompute);

        tcod.root.flush();

        level_up(objects, game, tcod);

        let player: &mut Object = &mut objects[PLAYER];
        previous_player_position = player.pos();

        let player_action = handle_keys(key, tcod, objects, game);

        if player_action == PlayerAction::Exit {
            save_game(objects, game).unwrap();
            break
        }

        if objects[PLAYER].alive && player_action != PlayerAction::DidntTakeTurn {
            for id in 0..objects.len() {
                if objects[id].ai.is_some() {
                    ai_take_turn(id, game, objects, &tcod.fov);
                }
            }
        }

    }

}

fn msgbox(text: &str, width: i32, root: &mut Root) {
    let options: &[&str] = &[];
    menu(text, options, width, root);
}

fn main_menu(tcod: &mut Tcod){
    let img = tcod::image::Image::from_file("menu_background.png")
        .ok()
        .expect("Background image not found");

    while !tcod.root.window_closed() {
        // show the background image, at twice the regular console resolution
        tcod::image::blit_2x(&img, (0, 0), (-1, -1), &mut tcod.root, (0, 0));

        tcod.root.set_default_foreground(colors::LIGHT_YELLOW);
        tcod.root.print_ex(
            SCREEN_WIDTH / 2,
            SCREEN_HEIGHT / 2 - 4,
            BackgroundFlag::None,
            TextAlignment::Center,
            "TOMBS OF THE ANCIENT KINGS",
        );
        tcod.root.print_ex(
            SCREEN_WIDTH / 2,
            SCREEN_HEIGHT - 2,
            BackgroundFlag::None,
            TextAlignment::Center,
            "By Moi",
        );

        let choices = &["Play a new game", "Continue last game", "Quit"];
        let choice = menu("", choices, 24, &mut tcod.root);

        match choice {
            Some(0) => {
                let (mut objects, mut game) = new_game(tcod);
                play_game(&mut objects, &mut game, tcod);
            }
            Some(1) => {
                match load_game() {
                    Ok((mut objects, mut game)) => {
                        initialise_fov(&game.map, tcod);
                        play_game(&mut objects, &mut game, tcod);
                    }
                    Err(_e) => {
                        msgbox("\nNo saved game to load.\n", 24, &mut tcod.root);
                        continue;
                    }
                }
            }
            Some(2) => {
                break;
            }
            _ => {}
        }
    }
}

fn main(){

    let root = Root::initializer()
        .font("./arial10x10.png", FontLayout::Tcod)
        .font_type(FontType::Greyscale)
        .size(SCREEN_WIDTH, SCREEN_HEIGHT)
        .title("Reflex")
        .init();
    tcod::system::set_fps(LIMIT_FPS);

    let mut tcod = Tcod {
        root,
        con: Offscreen::new(MAP_WIDTH, MAP_HEIGHT),
        panel: Offscreen::new(SCREEN_WIDTH, PANEL_HEIGHT),
        fov: FovMap::new(MAP_WIDTH, MAP_HEIGHT),
        mouse: Default::default(),
    };

    main_menu(&mut tcod);

}
