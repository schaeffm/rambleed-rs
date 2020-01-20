use bitvec::bits::AsBits;
use bitvec::order::Lsb0;
use bitvec::vec::BitVec;
use std::ops::Shl;

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

pub(crate) trait Architecture {
    fn phys_to_dram(&self, p: PhysAddr) -> DramAddr;
    fn dram_to_phys(&self, a: &DramAddr) -> PhysAddr;
}

pub(crate) struct IntelIvy {
    pub dual_channel : bool,
    pub dual_dimm : bool,
    pub dual_rank : bool,
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

    fn dram_to_phys(&self, addr: &DramAddr) -> usize {
        let bank = addr.bank as usize;
        let row = addr.row as usize;
        let rank = addr.rank as usize;
        let col = addr.col as usize;
        let chan = addr.chan as usize;
        let dimm = addr.dimm as usize;

        let mut p_addr = ls_bits(row, 16);

        if self.dual_rank {
            p_addr <<= 1;
            p_addr |= bit(bank, 2) ^ bit(row, 3);
            p_addr <<= 1;
            p_addr |= bit(rank, 0) ^ bit(row, 2);
        } else {
            p_addr <<= 1;
            p_addr |= bit(bank, 2) ^ bit(row, 2);
        }

        if(self.dual_dimm) {
            p_addr <<= 1;
            p_addr |= bit(dimm, 0);
        }

        p_addr <<= 1;
        p_addr |= bit(bank, 1) ^ bit(row, 1);
        p_addr <<= 1;
        p_addr |= bit(bank, 0) ^ bit(row, 0);

        if(self.dual_channel) {
            p_addr <<= 6;
            p_addr |= ls_bits(col >> 4, 6);
            p_addr <<= 1;
            p_addr |= bit(chan, 0) ^ bit(p_addr, 1) ^ bit(p_addr, 2) ^
                bit (p_addr, 5) ^ bit(p_addr, 6) ^ bit(p_addr, 11) ^ bit(p_addr, 12);
            p_addr <<= 4;
            p_addr |= ls_bits(col, 4);
        } else {
            p_addr <<= COL_BITS;
            p_addr |= ls_bits(col, COL_BITS);
        }

        p_addr << MW_BITS
    }
}

fn bit(x : usize, i : usize) -> usize {
    (x >> i) & 1
}

fn ls_bits(x : usize, i : usize) -> usize {
    x & ((1 << i) - 1)
}