use std::time::Instant;

const START_GRAN: usize = 0x100000;
const MAX_OVERSHOOT: f64 = 1.0 / 32.0;

pub(crate) fn reads_per_refresh(a1: *const u8, a2: *const u8, refresh_period_us: usize) -> usize {
    let mut reads_per_hammer = 0;
    let mut gran = START_GRAN;

    while gran > 0 {
        let t0 = Instant::now();
        hammer(a1, a2, reads_per_hammer + gran);
        let t_diff = t0.elapsed().as_micros();

        if t_diff < refresh_period_us as u128 {
            //println!("too fast");
            reads_per_hammer += gran;
        } else if t_diff < ((1.0 + MAX_OVERSHOOT) * (refresh_period_us as f64)) as u128 {
            return reads_per_hammer + gran;
        } else {
            //println!("too slow");
        }

        gran >>= 1
    }

    reads_per_hammer
}

pub(crate) fn hammer(a1: *const u8, a2: *const u8, num_reads: usize) {
    unsafe {
        for _ in 0..num_reads {
            asm!("mov eax, [$0]\n\t\
                  clflush [$0]\n\t\
                  mov eax, [$1]\n\t\
                  clflush [$1]"
                  :
                  : "r"(a1), "r"(a2)
                  : "eax"
                  : "volatile", "intel");
        }
    }
}
