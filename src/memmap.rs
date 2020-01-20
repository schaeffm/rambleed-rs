use std::slice;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use crate::architecture::{DramAddr};
use crate::config::Config;
use std::cmp::min;



#[derive(Clone)]
#[derive(Debug)]
pub(crate) struct DramRange {
    pub start: DramAddr,
    pub bytes: usize,
}

#[derive(Clone)]
#[derive(Debug)]
pub(crate) struct MemMap {
    pub buf: *mut u8,
    pub len: usize,
    pub range_map: HashMap<(u8, u8, u8, u8, u16), Vec<DramRange>>,
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
    pub(crate) fn new(buf: *mut u8, len: usize, c: &Config) -> Self {
        MemMap { buf, len, range_map: to_range_map(len, c) }
    }
}

//fn virt_to_phys_aligned(addr: usize, aligned_bits: usize) -> usize {
//    addr % (2usize.pow(aligned_bits as u32))
//}

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

fn to_range_map(len: usize, c: &Config) -> HashMap<(u8, u8, u8, u8, u16), Vec<DramRange>> {
    // build a Map<(channel, rank, bank, row)> -> Vec<ranges>
    let mut range_map = HashMap::<(u8, u8, u8, u8, u16), Vec<DramRange>>::new();
    for r in split_into_ranges(len, c) {
        let addr = &r.start;
        range_map.entry((addr.chan, addr.dimm, addr.rank, addr.bank, addr.row))
            .or_insert_with(Vec::new)
            .push(r);
    }
    range_map
}