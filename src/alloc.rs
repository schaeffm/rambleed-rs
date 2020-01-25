use std::ffi::c_void;
use std::ptr::{null_mut, read_volatile};

use proc_getter::buddyinfo::buddyinfo;
use vm_info::page_map::read_page_map;
use vm_info::page_size;
use vm_info::ProcessId::SelfPid;

use crate::architecture::PhysAddr;
use crate::config::Config;
use crate::memmap::MemMap;
use std::ptr;
use nix::libc;
use std::collections::{HashSet, HashMap};
use std::time::Instant;
use crate::hammer::hammer;
use nix::sys::socket::bind;

const SIZE_MB: usize = 1 << 20;

const HUGE_PAGE_BITS: usize = 21;
const HUGE_PAGE_SIZE: usize = 1 << HUGE_PAGE_BITS;
const MAP_HUGE_2MB: i32 = 21 << 26; // 21 << 26
const MAP_HUGE_1GB: i32 = 30 << 26; // 30 << 26

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
    let mem: *mut c_void = unsafe {
        libc::mmap(
            null_mut(),
            sz,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANONYMOUS | libc::MAP_PRIVATE | libc::MAP_POPULATE,
            -1,
            0,
        )
    };
    Some((mem as *mut u8, sz))
}

fn rdtsc() -> u64 {
    let mut a = 0;
    let mut d = 0;
    unsafe {
        asm!("xor %rax, %rax" ::: "rax" : "intel", "volatile");
        asm!("cpuid" ::: "rax", "rbx", "rcx", "rdx": "intel", "volatile");
        asm!("rdtsc" : "=rw" (a), "=rw" (d) ::: "intel", "volatile");
    }
    (d << 32) | a
}

fn get_timing(a1 : *const u8, a2 : *const u8, num_reads : usize) -> usize {
    for _ in 0..10 {
        unsafe {libc::sched_yield()};
    }

    let start = rdtsc();
    println!("start{}", start);
    unsafe {
        for _ in 0..num_reads {
            asm!("clflush [$0]\n\t\
                  clflush [$1]"
                  :
                  : "r"(a1), "r"(a2)
                  :
                  : "volatile", "memory", "intel");

            read_volatile(a1);
            read_volatile(a2);
        }
    }

    let end = rdtsc();
    println!("end{}", end);

    ((end - start) / num_reads as u64) as usize
}


pub(crate) fn reverse_mapping(c : &Config, buf : *mut u8) -> usize {
    let arch_map = create_offset_map(c);
    let reads_per_it = 10000000;
    let mut page_off_cand = HashSet::new();
    for i in 0..512 {
        page_off_cand.insert(i);
    }

    println!("dram_to_phys: {:X?}", virt_to_phys_pagemap(buf));
    let t0 = Instant::now();
    hammer(buf, buf.wrapping_add(4096), reads_per_it);
    let blind2 = get_timing(buf, buf.wrapping_add(0), reads_per_it);
    let blind = t0.elapsed().as_nanos();
    println!("blind value: {}", blind);
    println!("blind rdtsc: {}", blind2);

        for i in 1..0 {
            if page_off_cand.len() <= 1 {
                break;
            }

            for j in 1..*arch_map.keys().max().unwrap_or(&0) {
                println!("{:#?}", c.arch.phys_to_dram(4096*(i+j)));
                let t0 = Instant::now();
                hammer(buf.wrapping_add(4096*i), buf.wrapping_add(4096 * (i+j)), reads_per_it);
                let t_diff = t0.elapsed().as_nanos();
                println!("{}", t_diff);
                if t_diff >= (blind * 5) / 3 {
                    page_off_cand.intersection(
                        &arch_map
                            .get(&j)
                            .unwrap_or(&HashSet::new())
                            .iter()
                            .map(|p| p+j)
                            .collect::<HashSet<usize>>());
                    println!("found offset: {} to be slow",  j);
                    println!("new cands: {:#?}", page_off_cand);
                    break;

                }
            }
    }

    *page_off_cand.iter().collect::<Vec<&usize>>()[0]
}

fn create_offset_map(c : &Config) -> HashMap<usize, HashSet<usize>> {
    let mut m = HashMap::new();

    for i in 0..512 {
        let off = i * page_size().unwrap_or(4096);
        let da_off= c.arch.phys_to_dram(off);
        for j in 1..512 {
            let cur = off + j * page_size().unwrap_or(4096);
            let da_cur = c.arch.phys_to_dram(cur);
            if da_cur.chan == da_off.chan
                && da_cur.dimm == da_off.dimm
                && da_cur.bank == da_off.bank {
                m.entry(j)
                    .or_insert_with(HashSet::new)
                    .insert(i);
                break;
            }
        }
    }
    m
}

pub(crate) fn alloc_2mb_buddy(c: &Config) -> Option<MemMap> {
    let alloc_sz = sum_frees() - SIZE_MB;

    println!("Bytes in Buddy-allocator {}\n", alloc_sz);
    let mem_buddy_rest = map_eager(alloc_sz)?;

    println!("Bytes in Buddy-allocator {}\n", sum_frees());
    let mem_buddy_rest_2mb = map_eager(2 * SIZE_MB)?;
    println!("Bytes in Buddy-allocator {}\n", sum_frees());
    let mem_attack = map_eager(2 * SIZE_MB)?;

    unsafe {
        libc::munmap(mem_buddy_rest.0 as *mut _, alloc_sz);
        libc::munmap(mem_buddy_rest_2mb.0 as *mut _, 2 * SIZE_MB);
    }
    Some(MemMap::new(mem_attack.0, mem_attack.1, &c))
}

pub(crate) fn virt_to_phys_pagemap(v: *const u8) -> Option<PhysAddr> {
    let v = v as usize;
    let page_size = page_size().unwrap_or(4096);
    let page_num = v / page_size;

    let vpage = read_page_map(SelfPid, page_num).ok()?;
    let frame = vpage.page_frame()?;
    Some(frame as usize * page_size + v % page_size)
}

//setup: as root do: echo 512 > /sys/devices/system/node/node0/hugepages/hugepages-2048kB/free_hugepages
pub(crate) fn alloc_1gb_hugepage(c: &Config) -> Option<MemMap> {
    let mem_attack = unsafe {
        libc::mmap(
            ptr::null_mut(),
            HUGE_PAGE_SIZE << 9,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED | libc::MAP_ANONYMOUS | libc::MAP_POPULATE | MAP_HUGE_1GB,
            -1,
            0,
        )
    };

    Some(MemMap::new(mem_attack as *mut u8, HUGE_PAGE_SIZE << 9, c))
}

pub(crate) fn alloc_2mb_hugepage(c: &Config) -> Option<MemMap> {
    let mem_attack = unsafe {
        libc::mmap(
            ptr::null_mut(),
            HUGE_PAGE_SIZE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED
                | libc::MAP_ANONYMOUS
                | libc::MAP_HUGETLB
                | libc::MAP_POPULATE
                | MAP_HUGE_2MB,
            -1,
            0,
        )
    };

    Some(MemMap::new(mem_attack as *mut u8, HUGE_PAGE_SIZE, c))
}

//fn vpage_to_page(virtual_page_num: usize) -> Option<usize> {
//    let path = String::from("/proc/self/pagemap");
//
//    let mut f = fs::File::open(path).ok()?;
//    // Each entry is 8 bytes wide
//    let offset = virtual_page_num as u64 * 8;
//    f.seek(io::SeekFrom::Start(offset)).ok()?;
//
//    let data = f.read_u64::<byteorder::NativeEndian>().ok()?;
//
//    Some(data as usize)
//}

//Test
pub fn contig_mem_diff(c: &Config) {
    let mem_attack = alloc_2mb_buddy(c).unwrap();
    let start_p = virt_to_phys_pagemap(&mem_attack[0]).unwrap();
    let end_p = virt_to_phys_pagemap(&mem_attack[2 * SIZE_MB - 1]).unwrap();
    assert_eq!(start_p + 2 * SIZE_MB - 1, end_p)
}
