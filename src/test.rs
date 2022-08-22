use crate::*;
use crate::db::*;

#[test]
fn nva_test() {
    for i in 0..10000 {
    println!("VA-{}: {}", i+1, new_void_address());
    }
}

#[test]
fn db_test() {
    for _ in 0..2 {
        println!("boringdb path {:?}", db_path());
        let tab = dict_open("testable").unwrap();
        println!("boring dict opened");

        println!("{:?}", tab.remove(b"922"));

        for i in 0..0x10fff {
            let v=format!("testval.{}", i);
            let k=fastrand::u16(..).to_string();
            tab.insert(k.as_bytes().to_vec(), v.as_bytes().to_vec()).unwrap();
        }
        tab.flush().unwrap();
    }
}

#[test]
fn db_without_flush() {
    let mut n=fastrand::u32(..);
    let mut ks = vec![];
    let na = 20349;

    let tn = format!("nonflush_{}", fastrand::u64(..));

    let mut tab = dict_open(&tn).unwrap();
    tab.flush().unwrap(); // init make sure
    loop {
        tab.insert(n.to_string().as_bytes().to_vec(), (n*2).to_string().as_bytes().to_vec() ).unwrap();
        ks.push(n);

        let s=tab.get((n-na-na-na).to_string().as_bytes());
        println!("result {:?}", s);
        if s.is_err() || s.unwrap().is_none() {
            if ks.len() > 30 {
                break;
            }
        }

        n += na;

       if fastrand::u128(..)%30 == 1 {
            println!("random flush {:?}", tab.flush());
        }
        if fastrand::u8(..)%2 == 0{
            println!("rand-rop");
            //std::mem::drop(tab);
            tab = dict_open(&tn).unwrap();
        }

        if ks.len() > 50000 {
            println!("finish total.");
            break;
        }
    }
}

#[test]
fn nnd_test() {
    return;
    for i in 0..1 {
        println!("null[{}] {}", i+1, new_null_dst());
    }
}
