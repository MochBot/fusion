#[derive(Copy, Clone, Debug)]
pub struct Rules {
    pub enable_180: bool,
    pub enable_tspin: bool,
    pub enable_allspin: bool,
    pub srs_plus: bool,
    pub spawn_row: i32,
}

pub const ACTIVE_RULES: Rules = Rules {
    enable_180: true,
    enable_tspin: true,
    enable_allspin: true,
    srs_plus: true,
    spawn_row: 21,
};
