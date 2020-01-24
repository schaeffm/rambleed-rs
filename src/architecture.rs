pub(crate) type PhysAddr = usize;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct DramAddr {
    pub chan: u8,
    pub dimm: u8,
    pub rank: u8,
    pub bank: u8,
    pub row: u16,
    pub col: u16,
    pub byte: u8,
    pub bit: u8,
}

impl DramAddr {
    pub(crate) fn new() -> DramAddr {
        DramAddr {
            chan: 0,
            dimm: 0,
            rank: 0,
            bank: 0,
            row: 0,
            col: 0,
            byte: 0,
            bit: 0,
        }
    }

    pub fn byte_align(&mut self) {
        self.bit = 0;
    }

    pub fn col_align(&mut self) {
        self.byte_align();
        self.byte = 0;
    }

    pub fn row_align(&mut self) {
        self.col_align();
        self.col = 0;
    }

    pub fn row_aligned(&self) -> Self {
        let mut new = self.clone();
        new.row_align();
        new
    }

    pub fn row_below(&self) -> Self {
        let mut new = self.clone();
        new.row += 1;
        new
    }

    pub fn row_above(&self) -> Self {
        let mut new = self.clone();
        new.row -= 1;
        new
    }
}

pub trait Architecture {
    fn phys_to_dram(&self, p: PhysAddr) -> DramAddr;
    fn dram_to_phys(&self, a: &DramAddr) -> PhysAddr;
    fn refresh_period(&self) -> usize;
}
