use crate::camera::{self, Camera};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::Print,
    terminal::{Clear, ClearType},
};
use std::collections::HashMap;
use std::{
    collections::HashSet,
    io::{self, Stdout, Write},
};

const CHUNK_SIZE: i64 = 32;
const SURVIVAL: usize = 4;
const BIRTH: usize = 2;

#[derive(Eq, Hash, PartialEq, Clone, Copy, Debug)]
pub struct Cord {
    pub x: i64,
    pub y: i64,
}

#[derive(Eq, Hash, PartialEq, Debug)]
struct ChunkCord {
    chunk_x: i64,
    chunk_y: i64,
}
#[derive(Debug, PartialEq)]
enum Cell {
    Alive,
    Dead,
}
#[derive(Debug)]
pub struct World {
    chunks: HashMap<ChunkCord, HashMap<Cord, Cell>>,
}
impl World {
    pub fn new() -> Self {
        World {
            chunks: HashMap::new(),
        }
    }

    fn get_alive_neighbors_count(&self, cords: Cord) -> usize {
        let mut count = 0;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = cords.x + dx;
                let ny = cords.y + dy;
                if self.is_cell_alive(nx, ny) {
                    count += 1;
                }
            }
        }
        count
    }

    fn is_cell_alive(&self, x: i64, y: i64) -> bool {
        let chunk_cord = self.get_chunk_cords(Cord { x, y });
        let chunk = match self.chunks.get(&chunk_cord) {
            Some(chunk) => chunk,
            None => return false,
        };

        match chunk.get(&Cord { x, y }) {
            Some(Cell::Alive) => true,
            _ => false,
        }
    }

    fn get_chunk_cords(&self, cords: Cord) -> ChunkCord {
        ChunkCord {
            chunk_x: cords.x / CHUNK_SIZE,
            chunk_y: cords.y / CHUNK_SIZE,
        }
    }

    fn should_be_alive(&self, x: i64, y: i64) -> bool {
        let neighbors = self.get_alive_neighbors_count(Cord { x, y });
        let is_alive = self.is_cell_alive(x, y);
        (!is_alive && neighbors == BIRTH)
            || (is_alive && (neighbors == SURVIVAL || neighbors == BIRTH))
    }

    pub fn animate_cell(&mut self, x: i64, y: i64) {
        if x < 0 || y < 0 {
            return;
        }
        let chunk_cord = self.get_chunk_cords(Cord { x, y });
        let chunk = self.chunks.entry(chunk_cord).or_insert(HashMap::new());
        chunk.insert(Cord { x, y }, Cell::Alive);
    }

    pub fn kill_cell(&mut self, x: i64, y: i64) {
        if x < 0 || y < 0 {
            return;
        }
        let chunk_cord = self.get_chunk_cords(Cord { x, y });
        if let Some(chunk) = self.chunks.get_mut(&chunk_cord) {
            chunk.remove(&Cord { x, y });
        }
    }

    fn neighbors(&self, cords: Cord) -> Vec<Cord> {
        let mut neighbors: Vec<Cord> = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = cords.x + dx;
                let ny = cords.y + dy;
                if nx < 0 || ny < 0 {
                    continue;
                }
                neighbors.push(Cord { x: nx, y: ny });
            }
        }
        return neighbors;
    }

    pub fn tick(&mut self) {
        let mut to_check: HashSet<Cord> = HashSet::new();
        for (chunk_cord, chunk) in self.chunks.iter() {
            for cell_cord in chunk.keys() {
                let neighbors = self.neighbors(cell_cord.clone());
                for neighbor_cords in neighbors {
                    to_check.insert(neighbor_cords);
                }
            }
        }

        let mut to_alive: HashMap<Cord, Cell> = HashMap::new();
        for cord in to_check {
            if self.should_be_alive(cord.x, cord.y) {
                to_alive.insert(cord, Cell::Alive);
            }
        }

        let mut new_chunks: HashMap<ChunkCord, HashMap<Cord, Cell>> = HashMap::new();
        for cell_cord in to_alive.keys() {
            let chunk_cord = self.get_chunk_cords(cell_cord.clone());
            new_chunks
                .entry(chunk_cord)
                .or_insert(HashMap::new())
                .insert(cell_cord.clone(), Cell::Alive);
        }
        self.chunks.clear();
        self.chunks = new_chunks;
    }

    pub fn draw(&mut self, out: &mut Stdout, mut cam: Camera) {
        queue!(out, Clear(ClearType::All), MoveTo(0, 0));
        let left = cam.pos_x;
        let right = cam.pos_x + cam.term_width as i64;
        let top = cam.pos_y;
        let bottom = cam.pos_y + cam.term_height as i64;

        let chunk_left = left / CHUNK_SIZE;
        let chunk_right = right / CHUNK_SIZE;
        let chunk_top = top / CHUNK_SIZE;
        let chunk_bottom = bottom / CHUNK_SIZE;
        for x in chunk_left..=chunk_right {
            for y in chunk_top..=chunk_bottom {
                match self.chunks.get(&ChunkCord {
                    chunk_x: x,
                    chunk_y: y,
                }) {
                    Some(chunk) => {
                        for (cord, _) in chunk {
                            if (cord.x >= left && cord.x < right)
                                && (cord.y >= top && cord.y < bottom)
                            {
                                let (screen_x, screen_y) = cam.world_to_screen(cord.x, cord.y);
                                queue!(out, MoveTo(screen_x, screen_y), Print('#'));
                                out.flush();
                            }
                        }
                    }
                    _ => (),
                }
            }
        }

        out.flush();
    }

    pub fn lazy_draw(&mut self, out: &mut Stdout, mut cam: Camera, x: i64, y: i64, to_alive: bool) {
        let chunk_cords = self.get_chunk_cords(Cord { x, y });
        let chunk = self.chunks.entry(chunk_cords).or_insert_with(HashMap::new);
        let (nx, ny) = cam.world_to_screen(x, y);
        if !to_alive {
            chunk.remove(&Cord { x, y });
            queue!(out, MoveTo(nx, ny), Print(' ')).unwrap();
            out.flush();
        } else {
            chunk.insert(Cord { x, y }, Cell::Alive);
            queue!(out, MoveTo(nx, ny), Print('#')).unwrap();
            out.flush();
        }
    }
}
