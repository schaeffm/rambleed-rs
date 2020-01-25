use crate::alloc::virt_to_phys_pagemap;
use crate::architecture::DramAddr;
use crate::config::Config;
use crate::hammer::hammer;
use crate::memmap::{DramRange, MemMap};
use crate::profile::Direction::{From0To1, From1To0};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Direction {
    From1To0,
    From0To1,
}

#[derive(Debug, Clone)]
pub(crate) struct FlipStats {
    striped_complement: f64,
    above_complement: f64,
    below_complement: f64,
    uniform: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct Flip {
    pub(crate) dir: Direction,
    pub pos : DramAddr,
    pub stats : FlipStats,
}

impl Flip {
    fn new(dir : Direction, pos : DramAddr) -> Self {
        Flip {
            dir,
            pos,
            stats : FlipStats {
                striped_complement: 0.0,
                above_complement: 0.0,
                below_complement: 0.0,
                uniform: 0.0
            }
        }
    }
}

fn compl_fill(d : Direction) -> u8 {
    match d {
        From0To1 => 0xff,
        From1To0 => 0x00,
    }
}

fn id_fill(d : Direction) -> u8 {
    match d {
        From0To1 => 0x00,
        From1To0 => 0xff,
    }
}

fn byte_range(da : &DramAddr) -> Vec<DramRange> {
    vec![DramRange {
        start: da.clone(),
        bytes: 1,
    }]
}

fn hammer_bit(mem: &mut MemMap, da : &DramAddr, pat_above : u8, pat_victim : u8, pat_below : u8, c: &Config ) -> bool {
    let a1 = &da.row_above();
    let a2 = &da.row_below();
    let row_above = byte_range(a1);
    let row_below = byte_range(a2);
    let row = byte_range(da);

    fill_ranges(mem, &row_above, pat_above, c);
    fill_ranges(mem, &row, pat_victim, c);
    fill_ranges(mem, &row_below, pat_below, c);

    hammer(mem.dram_to_virt( &a1, c), mem.dram_to_virt(&a2, c), c.reads_per_hammer);

    let res = *mem.at_dram(da, c) & (1 << da.bit);
    let before = pat_victim & (1 << da.bit);
    if res != before {
        println!("found bit flip");
    }
    return *mem.at_dram(da,c) != pat_victim;
}

pub(crate) fn create_stats(mem: &mut MemMap, flip : &mut Flip, c : &Config) -> () {
    let n = 20;
    let to = compl_fill(flip.dir);
    let from = id_fill(flip.dir);

    let mut striped_flips = 0;
    let mut uniform_flips = 0;
    let mut above_flips = 0;
    let mut below_flips = 0;

    for i in 0..n {
        println!("{}", i);
        below_flips += hammer_bit(mem, &flip.pos, from, from, to, c) as usize;
        striped_flips += hammer_bit(mem, &flip.pos, to, from, to, c) as usize;
        uniform_flips += hammer_bit(mem, &flip.pos, from, from, from, c) as usize;
        above_flips += hammer_bit(mem, &flip.pos, to, from, from, c) as usize;
        }

    flip.stats.above_complement = above_flips as f64 / n as f64;
    flip.stats.below_complement = below_flips as f64 / n as f64;
    flip.stats.striped_complement = striped_flips as f64 / n as f64;
    flip.stats.uniform = uniform_flips as f64 / n as f64;
}

fn fill_ranges(mem: &mut MemMap, rs: &Vec<DramRange>, p: u8, c: &Config) {
    for r in rs {
        let start_off = mem.dram_to_offset(&r.start, c);

        for i in 0..r.bytes {
            mem[start_off + i] = p;
        }
    }
}

fn find_flips(da: DramAddr, mut expected: u8, mut actual: u8) -> Vec<Flip> {
    let mut flips = Vec::new();
    for bit in 0..8 {
        if expected & 1 != actual & 1 {
            let dir = if expected & 1 == 1 {
                From1To0
            } else {
                From0To1
            };
            let mut da_flip = da.clone();
            da_flip.bit = bit;
            flips.push(Flip::new(dir, da_flip))
        }
        expected >>= 1;
        actual >>= 1;
    }
    flips
}

fn flips_in_range(mem: &MemMap, v: &DramRange, expected: u8, c: &Config) -> Vec<Flip> {
    let mut flips = Vec::new();
    let base = mem.dram_to_offset(&v.start, c);

    for i in 0..v.bytes {
        let actual = mem[base + i];

        if actual != expected {
            println!(
                "Bit flip at physical address: {:p}",
                virt_to_phys_pagemap(&mem[base + i]).unwrap_or(std::usize::MAX) as *const usize
            );

            let mut cur_flips = find_flips(mem.offset_to_dram(base + i, &c), expected, actual);
            for f in &cur_flips {
                println!("Bit: {}, Dir: {:?}", f.pos.bit, f.dir);
            }
            flips.append(&mut cur_flips);
        }
    }
    flips
}

pub(crate) fn profile_addr(mem: &mut MemMap, da: &DramAddr, p: u8, c: &Config) -> Vec<Flip> {
    let mut da_above = da.row_above();
    let mut da_below = da.row_below();

    let row_above = byte_range(&da_above);
    let row_below = byte_range(&da_below);
    let row = byte_range(&da);

    fill_ranges(mem, &row_above, p, c);
    fill_ranges(mem, &row, !p, c);
    fill_ranges(mem, &row_below, p, c);

    let a1 = mem.dram_to_virt( &da_above, c);
    let a2 = mem.dram_to_virt(&da_below, c);

    hammer(a1, a2, c.reads_per_hammer);

    let mut flips = Vec::new();
    flips.append(&mut flips_in_range(
        mem,
        &DramRange { start : da.clone(), bytes: 1 },
        !p,
        c,
    ));

    flips
}

pub(crate) fn profile_ranges(
    mem: &mut MemMap,
    r1: &Vec<DramRange>,
    r2: &Vec<DramRange>,
    v: &Vec<DramRange>,
    p: u8,
    c: &Config,
) -> Vec<Flip> {
    //println!("{:#?}", r1);
    //println!("{:#?}", r2);
    //println!("{:#?}", v);

    let mut flips = Vec::new();
    if let (Some (a1), Some(a2)) = (r1.get(0), r2.get(0)){
        fill_ranges(mem, r1, p, c);
        fill_ranges(mem, v, !p, c);
        fill_ranges(mem, r2, p, c);

        let a1 = mem.dram_to_virt(&a1.start, c);
        let a2 = mem.dram_to_virt( &a2.start, c);
        hammer(a1, a2, c.reads_per_hammer);

        for v_range in v {
            flips.append(&mut flips_in_range(mem, v_range, !p, c));
        }

    }
    flips
}
