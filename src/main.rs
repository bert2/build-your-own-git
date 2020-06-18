use std::env;
use std::fmt;
use std::fs;
use std::io;

fn main() {
    let args: Vec<String> = env::args().collect();
    let exit_code = match run(&args) {
        Ok(msg) => {
            println!("{}", msg);
            0
        },
        Err(err) => {
            println!("error: {}", err);
            1
        }
    };
    std::process::exit(exit_code);
}

fn run(args: &Vec<String>) -> Result<String, String> {
    match args[1].as_str() {
        "init" => init()
            .map(|()| "initialized git directory.".to_string())
            .map_err(stringify),
        s => Err(format!("unknown command '{}'.", s))
    }
}

fn init() -> io::Result<()> {
    fs::create_dir(".git")?;
    fs::create_dir(".git/objects")?;
    fs::create_dir(".git/refs")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n")?;
    Ok(())
}

fn stringify<T: fmt::Display>(x: T) -> String {
    format!("{}", x)
}
