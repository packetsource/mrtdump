use std::any::type_name;
use std::marker::PhantomData;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub struct Trie<A, V> {
    left: Option<Box<Trie<A, V>>>,
    right: Option<Box<Trie<A, V>>>,
    value: Option<V>,
    phantom: PhantomData<A>,
}

impl<V> Trie<Ipv4Addr, V>
{
    // pub fn walk<F: Fn(IpAddr, u8, &V)>(&self, address: u32, depth: u8, handler: &F) {
    //     let trie: &Trie<Ipv4Addr, V> = self;
    //
    //     if let Some(value) = &trie.value {
    //         println!("{}/{}: {}", Ipv4Addr::from(address), depth, value);
    //         handler(IpAddr::V4(Ipv4Addr::from(address)), depth, value);
    //     }
    //     if let Some(left) = &trie.left {
    //         left.walk(address, depth + 1, handler);
    //     }
    //     if let Some(right) = &trie.right {
    //         let address =
    //             address + (2_u32.pow((Trie::<Ipv4Addr, V>::max_depth() - (depth + 1)) as u32));
    //         right.walk(address, depth + 1, handler);
    //     }
    // }
    // Return the maximum bit depth of the trie
    pub fn add(&mut self, ip: &Ipv4Addr, depth: u8, value: V) {
        let mut trie: &mut Trie<Ipv4Addr, V> = self;
        let address: u32 = (*ip).into();

        for d in 0..depth {
            trie = match address & 2_u32.pow((Trie::<Ipv4Addr, V>::max_depth() - (d + 1)) as u32) {
                0 => match trie.left {
                    Some(ref mut t) => t,
                    None => {
                        trie.left = Some(Box::new(Trie::new()));
                        trie.left.as_mut().unwrap()
                    }
                },
                _ => match trie.right {
                    Some(ref mut t) => t,
                    None => {
                        trie.right = Some(Box::new(Trie::new()));
                        trie.right.as_mut().unwrap()
                    }
                },
            };
        }
        trie.value = Some(value);
    }

    pub fn get(&self, ip: &Ipv4Addr, depth: u8) -> Option<(Ipv4Addr, u8, &V)> {
        let address: u32 = (*ip).into();
        let mut trie: &Trie<Ipv4Addr, V> = self;
        let mut best: Option<(Ipv4Addr, u8, &V)> = None;
        // let mut best: Option<&V> = None;
        let mut current: u32 = 0;

        let mut d: u8 = 0;

        loop {
            // If the current position in the trie has an associated value,
            // record it as the current best candidate
            if let Some(v) = &trie.value {
                best = Some((Ipv4Addr::from(current), d, v))
                // best = Some(v)
            }

            if d == depth || d == Trie::<Ipv4Addr, V>::max_depth() {
                break;
            }

            // Then choose the next direction, updating the effective
            // address for that branch
            trie = match address & 2_u32.pow((Trie::<Ipv4Addr, V>::max_depth() - (d + 1)) as u32) {
                0 => match trie.left {
                    Some(ref t) => t,
                    None => break,
                },
                _ => match trie.right {
                    Some(ref t) => {
                        current |= address
                            & 2_u32.pow((Trie::<Ipv4Addr, V>::max_depth() - (d + 1)) as u32);
                        t
                    }
                    None => break,
                },
            };
            d += 1;
        }
        // best.map(|x| (Ipv4Addr::from(current), d, x.clone()))     // we just do the clone() once here
        best
    }
}

impl<V> Trie<Ipv6Addr, V>
// where
//     V: std::fmt::Display, //+ Copy,
{

    // pub fn walk<F: Fn(IpAddr, u8, &V)>(&self, address: u128, depth: u8, handler: &F) {
    //     let trie: &Trie<Ipv6Addr, V> = self;
    //
    //     if let Some(value) = &trie.value {
    //         println!("{}/{}: {}", Ipv6Addr::from(address), depth, value);
    //         handler(IpAddr::V6(Ipv6Addr::from(address)), depth, value);
    //     }
    //     if let Some(left) = &trie.left {
    //         left.walk(address, depth + 1, handler);
    //     }
    //     if let Some(right) = &trie.right {
    //         let address =
    //             address + (2_u128.pow((Trie::<Ipv6Addr, V>::max_depth() - (depth + 1)) as u32));
    //         right.walk(address, depth + 1, handler);
    //     }
    // }
    // Return the maximum bit depth of the trie
    pub fn add(&mut self, ip: &Ipv6Addr, depth: u8, value: V) {
        let mut trie: &mut Trie<Ipv6Addr, V> = self;
        let address: u128 = (*ip).into();

        for d in 0..depth {
            trie = match address & 2_u128.pow((Trie::<Ipv6Addr, V>::max_depth() - (d + 1)) as u32) {
                0 => match trie.left {
                    Some(ref mut t) => t,
                    None => {
                        trie.left = Some(Box::new(Trie::new()));
                        trie.left.as_mut().unwrap()
                    }
                },
                _ => match trie.right {
                    Some(ref mut t) => t,
                    None => {
                        trie.right = Some(Box::new(Trie::new()));
                        trie.right.as_mut().unwrap()
                    }
                },
            };
        }
        trie.value = Some(value);
    }

    pub fn get(&self, ip: &Ipv6Addr, depth: u8) -> Option<(Ipv6Addr, u8, &V)> {
        let address: u128 = (*ip).into();
        let mut trie: &Trie<Ipv6Addr, V> = self;
        let mut best: Option<(Ipv6Addr, u8, &V)> = None;
        let mut current: u128 = 0;

        let mut d: u8 = 0;

        loop {
            // If the current position in the trie has an associated value,
            // record it as the current best candidate
            if let Some(v) = &trie.value {
                best = Some((Ipv6Addr::from(current), d, v))
            }

            if d == depth || d == Trie::<Ipv6Addr, V>::max_depth() {
                break;
            }

            // Then choose the next direction, updating the effective
            // address for that branch
            trie = match address & 2_u128.pow((Trie::<Ipv6Addr, V>::max_depth() - (d + 1)) as u32) {
                0 => match trie.left {
                    Some(ref t) => t,
                    None => break,
                },
                _ => match trie.right {
                    Some(ref t) => {
                        current |= address
                            & 2_u128.pow((Trie::<Ipv6Addr, V>::max_depth() - (d + 1)) as u32);
                        t
                    }
                    None => break,
                },
            };
            d += 1;
        }
        best
    }
}

impl<A, V> Trie<A, V> {
    pub fn new() -> Self {
        Trie {
            left: None,
            right: None,
            value: None,
            phantom: PhantomData,
        }
    }
    pub fn max_depth() -> u8 {
        size_of::<A>() as u8 * 8
    }
}

use std::fmt;
impl<A, V> fmt::Display for Trie<A, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Trie<{}>", type_name::<A>())
    }
}
