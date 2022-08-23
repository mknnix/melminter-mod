fn git_commit_hash() -> String {
    #[allow(unused_variables)]

    let static_git_sha = "[[Replaceit]]";
let static_git_sha = "git.9ed9f59e3d4f15bb5a870eafaa8217949867f508"; //CODEADD// by gitsha in code

    if static_git_sha.contains("[[Replaceit]]") {
        return _dyna_git_commit_hash();
    }
    if static_git_sha == "git." {
        return "release-nogit?".to_string();
    }

    assert!( static_git_sha.starts_with("git.") || static_git_sha == "release" );
    static_git_sha.to_string()
}
fn _dyna_git_commit_hash() -> String {
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

