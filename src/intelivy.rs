use crate::architecture::{Architecture, DramAddr};
#[derive(Clone)]
pub(crate) struct IntelIvy {
    pub dual_channel : bool,
    pub dual_dimm : bool,
    pub dual_rank : bool,
}

const MW_BITS : usize= 3;
const COL_BITS : usize = 10;

impl IntelIvy {
}

fn remove_bit(x : usize, i : usize) -> usize {
    ls_bits(x, i) + ((x >> (i+1)) << i)
}

impl Architecture for IntelIvy {
    fn refresh_period(&self) -> usize {
        64_000
    }
    fn phys_to_dram(&self, mut p: usize) -> DramAddr {
        //println!("{}", p);
        let mut dram_addr : DramAddr = DramAddr::new();
        if self.dual_channel {
            dram_addr.chan = (
                bit(p,7) ^ bit(p,8) ^ bit(p, 9) ^ bit(p, 12) ^
                    bit(p, 13) ^ bit(p, 18) ^ bit(p, 19)) as u8;
            p = remove_bit(p, 7);
        };

        p >>= MW_BITS;

        dram_addr.col = ls_bits(p, COL_BITS) as u16;
        p >>= COL_BITS;

        if self.dual_dimm {
            dram_addr.dimm = bit(p, 2) as u8;
            p = remove_bit(p, 2);
        }

        if self.dual_rank {
            dram_addr.rank = (bit(p, 2) ^ bit(p, 6)) as u8;
            p = remove_bit(p, 2);
        }
        for i in 0 .. 2 {
            dram_addr.bank |= ((bit(p, 0) ^ bit(p, 3)) as u8) << i;
            p >>= 1;
        }

        if self.dual_rank {
            dram_addr.bank |= ((bit(p, 0) ^ bit(p, 4)) as u8) << 2;
        } else {
            dram_addr.bank |= ((bit(p, 0) ^ bit(p, 3)) as u8) << 2;
        }
        p >>= 1;

        dram_addr.row = ls_bits(p, 16) as u16;
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

        if self.dual_dimm {
            p_addr <<= 1;
            p_addr |= bit(dimm, 0);
        }

        p_addr <<= 1;
        p_addr |= bit(bank, 1) ^ bit(row, 1);
        p_addr <<= 1;
        p_addr |= bit(bank, 0) ^ bit(row, 0);

        if self.dual_channel {
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