use crate::*;
use std::any::type_name;
use std::marker::PhantomData;

pub struct Trie<A, V> {
    left: Option<Box<Trie<A, V>>>,
    right: Option<Box<Trie<A, V>>>,
    value: Vec<V>,
    phantom: PhantomData<A>,
}

enum TrieIteratorState {
    Local,  // will return the local trie node value next
    Left,   // will proceed to the left branch and get local next
    Right,  // will proceed to the right branch and get local next
}
pub struct TrieIterator<'a, A, V> {
    stack: Vec<&'a Trie<A, V>>,
    step: TrieIteratorState, // left or right
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


    pub fn add(&mut self, ip: &Ipv4Addr, depth: u8, mut value: Vec<V>) {
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
        trie.value.append(&mut value);
    }

    pub fn get(&self, ip: &Ipv4Addr, depth: u8) -> Option<(Ipv4Addr, u8, &Vec<V>)> {
        let address: u32 = (*ip).into();
        let mut trie: &Trie<Ipv4Addr, V> = self;
        let mut best: Option<(Ipv4Addr, u8, &Vec<V>)> = None;
        let mut current: u32 = 0;

        let mut d: u8 = 0;

        loop {
            // If the current position in the trie has an associated value,
            // record it as the current best candidate
            if ! &trie.value.is_empty() {
                best = Some((Ipv4Addr::from(current), d, &trie.value))
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
        best
    }
}

impl<V> Trie<Ipv6Addr, V>
{

    pub fn add(&mut self, ip: &Ipv6Addr, depth: u8, mut value: Vec<V>) {
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
        trie.value.append(&mut value);
    }

    pub fn get(&self, ip: &Ipv6Addr, depth: u8) -> Option<(Ipv6Addr, u8, &Vec<V>)> {
        let address: u128 = (*ip).into();
        let mut trie: &Trie<Ipv6Addr, V> = self;
        let mut best: Option<(Ipv6Addr, u8, &Vec<V>)> = None;
        let mut current: u128 = 0;

        let mut d: u8 = 0;

        loop {
            // If the current position in the trie has an associated value,
            // record it as the current best candidate
            if ! &trie.value.is_empty() {
                best = Some((Ipv6Addr::from(current), d, &trie.value))
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
            value: Vec::new(),
            phantom: PhantomData,
        }
    }
    pub fn iter<'a>(&'a mut self) -> TrieIterator<'a, A, V> {
        TrieIterator {
            stack: vec![self],
            step: TrieIteratorState::Local
        }
    }

    // Return the maximum bit depth of the trie
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

impl<'a, A, V> Iterator for TrieIterator<'a, A, V> {
    type Item = &'a Vec<V>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.step {
            TrieIteratorState::Local => {
            },
            TrieIteratorState::Left => {
            },
            TrieIteratorState::Right => {
            },
        }
        None
    }
}
