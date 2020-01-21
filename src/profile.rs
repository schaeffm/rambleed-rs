use crate::memmap::{MemMap, DramRange};
use crate::config::Config;
use crate::architecture::DramAddr;
use crate::profile::Direction::{From1To0, From0To1};
use crate::hammer::hammer;

#[derive(Debug)]
#[derive(Clone)]
#[derive(Copy)]
pub(crate) enum Direction {
    From1To0,
    From0To1,
}

#[derive(Debug)]
#[derive(Clone)]
#[derive(Copy)]
pub(crate) struct Flip {
    pub(crate) dir: Direction,
    pub(crate) offset: usize,
    pub(crate) bit: u8,
}

fn fill_ranges(mem: &MemMap, rs: &Vec<DramRange>, p: u8, c: &Config) {
    for r in rs {
        let v_addr = mem.buf.wrapping_add(c.arch.dram_to_phys(&r.start));
        let v_arr = unsafe { std::slice::from_raw_parts_mut(v_addr as *mut u8, r.bytes) };
        for b in v_arr { *b = p }
    }
}

fn dram_to_virt(base: *const u8, addr: &DramAddr, c: &Config) -> *const u8 {
    base.wrapping_add(c.arch.dram_to_phys(addr))
}

fn find_flips(offset: usize, mut expected: u8, mut actual: u8) -> Vec<Flip> {
    let mut flips = Vec::new();
    for bit in 0..8 {
        if expected & 1 != actual & 1 {
            let dir = if expected & 1 == 1 {
                From1To0
            } else {
                From0To1
            };
            flips.push(Flip { dir, bit, offset })
        }
        expected >>= 1;
        actual >>= 1;
    }
    flips
}

fn flips_in_range(mem: &MemMap, v : &DramRange, expected : u8, c: &Config) -> Vec<Flip> {
    let mut flips = Vec::new();
    for i in 0..v.bytes {
        let offset = i+c.arch.dram_to_phys(&v.start);
        let actual = mem[offset];

        if actual != expected {
            println!("ALERT, found bitflip");
            flips.append(&mut find_flips(offset, expected, actual));
        }
    }
    flips
}

pub(crate) fn profile_ranges(mem: &MemMap,
                             r1: &Vec<DramRange>,
                             r2: &Vec<DramRange>,
                             v: &Vec<DramRange>,
                             p: u8,
                             c: &Config) -> Vec<Flip> {
    fill_ranges(mem, r1, p, c);
    fill_ranges(mem, v, !p, c);
    fill_ranges(mem, r2, p, c);
    let a1 = dram_to_virt(mem.buf, &r1[0].start, c);
    let a2 = dram_to_virt(mem.buf, &r2[0].start, c);
    let start = &v[0].start;
    //println!("profiling ({}, {}, {}, {}): row {}, pattern {}, {}, {}", start.chan, start.dimm, start.rank, start.bank, v[0].start.row, p, !p, p);
    hammer(a1, a2, c.reads_per_hammer);

    let mut flips = Vec::new();
    for v_range in v {
        flips.append(&mut flips_in_range(mem, v_range, !p, c));
    }
    flips
}