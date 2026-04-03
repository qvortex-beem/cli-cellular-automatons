#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub pos_x: i64,
    pub pos_y: i64,
    pub zoom: i64,
    pub term_width: u16,
    pub term_height: u16,
}

impl Camera {
    pub fn new(term_width: u16, term_height: u16) -> Self {
        Self {
            pos_x: 0,
            pos_y: 0,
            zoom: 1,
            term_width,
            term_height,
        }
    }

    pub fn screen_to_world(&mut self, x: u16, y: u16) -> (i64, i64) {
        ((x as i64 + self.pos_x), (y as i64 + self.pos_y))
    }
    pub fn world_to_screen(&mut self, x: i64, y: i64) -> (u16, u16) {
        ((x - self.pos_x) as u16, (y - self.pos_y) as u16)
    }
}
