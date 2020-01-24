use crate::architecture::Architecture;

pub struct Config {
    pub aligned_bits: usize,
    pub reads_per_hammer: usize,
    pub contiguous_dram_addr: usize,
    pub arch: Box<dyn Architecture>,
}
