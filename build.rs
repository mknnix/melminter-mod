fn git_commit_hash() -> String {
    use std::process::Command;
    let hash: String = String::from_utf8( Command::new("git").arg("rev-parse").arg("HEAD").output().unwrap().stdout ).unwrap();
    hash
}

fn main() {
    println!("cargo:rustc-env=GIT_COMMIT_HASH={}", git_commit_hash());
}
