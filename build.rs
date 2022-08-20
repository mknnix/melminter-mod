fn git_commit_hash() -> String {
    use std::process::Command;
    let mut hash: String = String::from_utf8( Command::new("git").arg("rev-parse").arg("origin/master").output().unwrap().stdout ).unwrap();

    if hash.is_empty() {
        hash = "release".to_string();
    } else {
        hash = "git.".to_string() + &hash;
    }

    hash
}

fn main() {
    println!("cargo:rustc-env=GIT_COMMIT_HASH={}", git_commit_hash());
}

