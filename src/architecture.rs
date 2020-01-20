use bitvec::bits::AsBits;
use bitvec::order::Lsb0;
use bitvec::vec::BitVec;

pub(crate) type PhysAddr = usize;

#[derive(Clone)]
pub(crate) struct DramAddr {
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

trait Architecture {
    fn phys_to_dram(&self, p: PhysAddr) -> DramAddr;
    fn dram_to_phys(&self, a: DramAddr) -> PhysAddr;
}

struct IntelIvy {
    dual_channel : bool,
    dual_dimm : bool,
    dual_rank : bool,
}

const MW_BITS : usize= 3;
const COL_BITS : usize = 10;

impl IntelIvy {
}

impl Architecture for IntelIvy {
    fn phys_to_dram(&self, p: usize) -> DramAddr {
        let mut p = BitVec::from(p.bits::<Lsb0>());
        let mut dram_addr : DramAddr = DramAddr::new();
        if self.dual_channel {
            dram_addr.chan = (
                p[7] ^ p[8] ^ p[9] ^ p[12] ^ p[13] ^ p[18] ^ p[19]) as u8;
            p.remove(7);
        };
        p >>= MW_BITS;
        let cols = p.split_off(10);
        dram_addr.col = cols.as_slice()[0] as u16;

        if self.dual_dimm {
            dram_addr.dimm = p[2] as u8;
            p.remove(2);
        }

        if self.dual_rank {
            dram_addr.rank = (p[2] ^ p[6]) as u8;
            p.remove(2);
        }

        for i in 0 .. 2 {
            dram_addr.bank |= ((p[0] ^ p[3]) as u8) << i;
            p >>= 1;
        }

        if self.dual_rank {
            dram_addr.bank |= ((p[0] ^ p[4]) as u8) << 2;
        } else {
            dram_addr.bank |= ((p[0] ^ p[3]) as u8) << 2;
        }
        p >>= 1;

        dram_addr.row = p[0 .. 16].as_slice()[0] as u16;
        return dram_addr;
    }

    fn dram_to_phys(&self, a: DramAddr) -> usize {
        unimplemented!()
    }
}