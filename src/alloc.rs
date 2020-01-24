use std::ffi::c_void;
use std::ptr::null_mut;

use proc_getter::buddyinfo::buddyinfo;
use vm_info::page_map::read_page_map;
use vm_info::page_size;
use vm_info::ProcessId::SelfPid;

use crate::architecture::PhysAddr;
use crate::config::Config;
use crate::memmap::MemMap;
use std::ptr;
use nix::libc;

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
