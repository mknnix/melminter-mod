fn git_commit_hash() -> String {
    #[allow(unused_variables)]

    let static_git_sha = "[[Replaceit]]";
let static_git_sha = "git.124c9e362e2e9fdb36bf3bbd409884ec72d2dcb1"; //CODEADD// by gitsha in code

    if env!("CARGO_PKG_VERSION").to_ascii_lowercase().contains("alpha") == false {
        return "release".to_string();
    }
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
    // CPU arch checks but try build anyway
    if cfg!(any(target_arch="x86_64", target_arch="aarch64")) {
        println!("Good CPU Arch: amd/ia 64-bit or arm 64-bit");
    } else {
        println!("Your CPU Arch is not tested and cannot make sure it works.");

        if cfg!(target_arch="powerpc64") {
            println!("Your CPU is 64-bit but a PowerPC arch, welcome for testing that");
        }
        if cfg!(not(target_pointer_width="64")) {
            //#[cfg(any(target_arch="arm", target_arch="mips", target_arch="powerpc"))]
            println!("Your CPU is not 64-bit! This Is Undefined And May Not Recommended");
        }
    }

    println!("cargo:rustc-env=GIT_COMMIT_HASH={}", git_commit_hash());
}

