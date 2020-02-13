# rambleed-rs
Basic Rambleed PoC written in Rust

## Architecture
Only works on Intel Ivy Bridge CPUs at the moment.
Can easily be extended by adding a custom address translation.

## Setup
Install Rust using rustup  
```curl https://sh.rustup.rs -sSf | sh```

Build project with optimizations  
```cargo build --release```

Activate huge pages  
```echo 512 > /sys/devices/system/node/node0/hugepages/hugepages-2048kB/nr_hugepages```

Execute with privileges to see physical addresses  
```sudo ./target/release/rambleed-rs```

Alternatively (no physical addresses displayed)  
```cargo run --release```

To test reliability of a bit flip: use ```test_stats()```  
Address to hammer is hardcoded at the moment.
