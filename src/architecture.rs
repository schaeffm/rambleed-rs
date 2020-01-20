pub(crate) type PhysAddr = usize;

#[derive(Clone)]
#[derive(Debug)]
pub struct DramAddr {
    pub chan: u8,
    pub dimm: u8,
    pub rank: u8,
    pub bank: u8,
    pub row: u16,
    pub col: u16,
}

impl DramAddr {
    pub(crate) fn new() -> DramAddr {
        DramAddr {
            chan : 0,
            dimm : 0,
            rank : 0,
            bank : 0,
            row : 0,
            col : 0,
        }
    }

    pub(crate) fn to_row_index(&self) -> (u8, u8, u8, u8, u16) {
        (self.chan, self.dimm, self.rank, self.bank, self.row)
    }
}

pub trait Architecture {
    fn phys_to_dram(&self, p: PhysAddr) -> DramAddr;
    fn dram_to_phys(&self, a: &DramAddr) -> PhysAddr;
    fn refresh_period(&self) -> usize;
}
