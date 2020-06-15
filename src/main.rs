#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::fs;

fn main() {
    println!("Your code goes here!");

    let args: Vec<String> = env::args().collect();
    if args[1] == "init" {
        fs::create_dir(".git")
            .and_then(|()| fs::create_dir(".git/objects"))
            .and_then(|()| fs::create_dir(".git/refs"))
            .and_then(|()| fs::write(".git/HEAD", "ref: refs/heads/master\n"))
            .unwrap();
        println!("Initialized git directory")
    } else {
        println!("unknown command: {}", args[1])
    }
}
