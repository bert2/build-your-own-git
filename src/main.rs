use std::env;
use std::env::Args;
use std::fs;
use std::error::Error;
use std::iter::Skip;
use flate2::read::GzDecoder;
use std::io::prelude::*;

type Res<T> = std::result::Result<T, Box<dyn Error>>;

fn main() {
    fn run(args: &mut Skip<Args>) -> Res<String> {
        let cmd = args.next()
            .ok_or("no command provided. available commands: init, cat-file.")?;
        match cmd.as_str() {
            "init" => init(),
            "cat-file" => cat_file(args),
            _ => Err(format!("unknown command '{}'.", cmd).into())
            // s => Err(["unknown command '", cmd, "'."].concat().into())
        }
    }

    let mut args = env::args().skip(1);
    let exit_code = match run(&mut args) {
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

fn init() -> Res<String> {
    fs::create_dir(".git")?;
    fs::create_dir(".git/objects")?;
    fs::create_dir(".git/refs")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n")?;
    Ok("initialized git directory.".to_string())
}

fn cat_file(args: &mut Skip<Args>) -> Res<String> {
    fn assert_prettyprint(args: &mut Skip<Args>) -> Res<()> {
        let arg = args.next()
            .ok_or("not enough arguments provided for command cat-file. missing flag '-p'.")?;
        match arg.as_str() {
            "-p" => Ok(()),
            _ => Err("cat-file must be used with '-p'.")?
        }
    }

    fn validate_sha(args: &mut Skip<Args>) -> Res<String> {
        let sha = args.next()
            .ok_or("not enough arguments provided for command cat-file. missing SHA.")?;
        if sha.len() < 3 {
            Err("provided SHA is invalid.")?
        } else {
            Ok(sha)
        }
    }

    assert_prettyprint(args)?;
    let sha = validate_sha(args)?;

    let (dir, filename) = sha.split_at(3);
    let path = [dir, "/", filename].concat();
    println!("path: {}", path);
    let file = fs::File::open(path)?;

    let mut decoder = GzDecoder::new(file);
    let mut contents = String::new();
    decoder.read_to_string(&mut contents)?;

    Ok(contents)
}
