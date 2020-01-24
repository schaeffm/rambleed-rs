use crate::architecture::DramAddr;
use crate::config::Config;
use std::cmp::min;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::slice;

#[derive(Clone, Debug)]
pub(crate) struct DramRange {
    pub start: DramAddr,
    pub bytes: usize,
}

pub(crate) struct MemMap {
    buf: *mut u8,
    len: usize,
    range_map: HashMap<DramAddr, Vec<DramRange>>,
}

impl Deref for MemMap {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.buf, self.len) }
    }
}

impl DerefMut for MemMap {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.buf, self.len) }
    }
}

impl MemMap {
    pub(crate) fn new(buf: *mut u8, len: usize, c : &Config) -> Self {
        MemMap {
            buf,
            len,
            range_map: to_range_map(len, c),
        }
    }

    pub fn remove_range(&mut self, _: &DramRange) {
        //TODO
    }

    pub fn same_row_ranges(&self, da: &DramAddr) -> Vec<DramRange> {
        self.range_map
            .get(&da.row_aligned())
            .unwrap_or(&vec![])
            .clone()
    }

    pub fn get_ranges(&self) -> &HashMap<DramAddr, Vec<DramRange>> {
        &self.range_map
    }

    pub fn offset(&self, n: usize) -> *mut u8 {
        self.buf.wrapping_add(n)
    }

    pub fn at_dram(&mut self, da : &DramAddr, c : &Config) -> &mut u8 {
        &mut self[c.arch.dram_to_phys(&da)]
    }

    pub fn dram_to_offset(&self, da : &DramAddr, c : &Config) -> usize {
        c.arch.dram_to_phys(&da)
    }

    pub fn offset_to_dram(&self, offset : usize, c : &Config) -> DramAddr {
        c.arch.phys_to_dram(offset)
    }

    pub fn dram_to_virt(&self, da : &DramAddr, c : &Config) -> *mut u8 {
        self.buf.wrapping_add(c.arch.dram_to_phys(&da))
    }
}

pub(crate) fn offset_to_dram(offset: usize, c: &Config) -> DramAddr {
    //let phys = virt_to_phys_aligned(offset, c.aligned_bits);
    c.arch.phys_to_dram(offset)
}
// assumes mem is aligned to contiguous address range
fn split_into_ranges(len: usize, c: &Config) -> Vec<DramRange> {
    let mut ranges = Vec::new();
    for i in (0..len).step_by(c.contiguous_dram_addr) {
        ranges.push(DramRange {
            start: offset_to_dram(i, c),
            bytes: min(len - i, c.contiguous_dram_addr),
        });
    }

    ranges
}

fn to_range_map(len: usize, c: &Config) -> HashMap<DramAddr, Vec<DramRange>> {
    let mut range_map = HashMap::<DramAddr, Vec<DramRange>>::new();
    for r in split_into_ranges(len, c) {
        range_map
            .entry(r.start.row_aligned())
            .or_insert_with(Vec::new)
            .push(r);
    }
    //println!("{:#?}", range_map);
    range_map
}
