use std::fmt::{Debug, Display};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::ipaddrmask;
use crate::ipaddrmask::IpAddrMask;

pub static TRIE_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub struct RadixTrie<A: Copy + IpAddrMask, V: PartialEq> {
    pub prefix: A, // Use u32 or u128
    pub len: u8,
    pub value: Option<Vec<V>>,
    left: Option<Box<RadixTrie<A, V>>>,
    right: Option<Box<RadixTrie<A, V>>>,
    phantom: PhantomData<A>,
}
#[derive(PartialEq, Debug)]
enum RadixTrieBranch { ExactMatch, Left, Right, NoMatch }

//const DEBUG: bool = true;

impl<A: Copy + IpAddrMask + PartialEq + Display, V: PartialEq> RadixTrie<A, V> {
    pub fn new(prefix: A, len: u8) -> Self {
        TRIE_COUNT.fetch_add(1,Ordering::Relaxed);
        RadixTrie {
            prefix: prefix.mask(len),
            len,
            value: None,
            left: None,
            right: None,
            phantom: PhantomData,
        }
    }

    // Is the candidate contained within the prefix?
    fn contains(&self, candidate: A, len: u8) -> bool {
        len >= self.len && self.prefix == candidate.mask(self.len)
    }



    // Analyse the provided candidate and length.
    //  Determine if:
    // - it is left from the current node,
    // - right from the current node,
    // - an exact match,
    // - not contained
    fn next_branch(&self, candidate: A, len: u8) -> RadixTrieBranch {
        if candidate.mask(self.len) == self.prefix {
            if self.len == len {
                return RadixTrieBranch::ExactMatch;
            } else {
                if candidate.next_bit(self.len) == 0u8 {
                    return RadixTrieBranch::Left;
                } else {
                    return RadixTrieBranch::Right;
                }
            }
        } else {
            return RadixTrieBranch::NoMatch;
        }
    }

    // Insert a new node in the radix trie into either the left or
    // right branch, re-connecting any existing left or right branch
    // to the new node correctly
    fn splice(&mut self, mut node: RadixTrie<A, V>) {
        let branch = self.next_branch(node.prefix, node.len);
        match branch{
            RadixTrieBranch::Left => {
                let existing = self.left.take();
                if let Some(existing) = existing {
                    let next_branch = node.next_branch(existing.prefix, existing.len);
                    // eprintln!("Splicing: {}/{}:{:?} -> {}/{}:{:?} -> {}/{}", self.prefix, self.len, branch, node.prefix, node.len, next_branch, existing.prefix, existing.len);
                    match next_branch {
                        RadixTrieBranch::Left => {
                            node.left = Some(existing);
                        },
                        RadixTrieBranch::Right => {
                            node.right = Some(existing)
                        }
                        _ => {}
                    }
                } else {
                    // eprintln!("Splicing: {}/{}:{:?} -> {}/{}", self.prefix, self.len, branch, node.prefix, node.len);
                }
                self.left = Some(Box::new(node));
            },
            RadixTrieBranch::Right => {
                let existing = self.right.take();
                if let Some(existing) = existing {
                    let next_branch = node.next_branch(existing.prefix, existing.len);
                    // eprintln!("Splicing: {}/{}:{:?} -> {}/{}:{:?} -> {}/{}", self.prefix, self.len, branch, node.prefix, node.len, next_branch, existing.prefix, existing.len);
                    match next_branch {
                        RadixTrieBranch::Left => {
                            node.left = Some(existing);
                        },
                        RadixTrieBranch::Right => {
                            node.right = Some(existing)
                        }
                        _ => {}
                    }
                } else {
                    // eprintln!("Splicing: {}/{}:{:?} -> {}/{}", self.prefix, self.len, branch, node.prefix, node.len);
                }
                self.right = Some(Box::new(node));
            },
            _ => {}
        }
    }

    pub fn add(&mut self, prefix: A, len: u8, value: V) {
        self.add_vec(prefix, len, vec![value])
    }


    pub fn add_vec(&mut self, prefix: A, len: u8, mut value: Vec<V>) {
        let mut r = self;
        loop {
            let branch = r.next_branch(prefix, len);
            // eprintln!("{}/{}: next branch is {}/{}:{:?}", prefix, len, r.prefix, r.len, branch);
            match branch {
                RadixTrieBranch::ExactMatch => {
                    if r.value.is_none() {
                        r.value = Some(value);
                    } else {
                        r.value.as_mut().unwrap().append(&mut value);
                    }
                    break;
                }
                RadixTrieBranch::Left => {
                    if r.left.is_some() {
                        if r.left.as_ref().unwrap().contains(prefix, len) {
                            r = r.left.as_mut().unwrap();
                            continue;   // follow left branch
                        } else {
                            // We need to add an intermediate parent, common to the existing left
                            // node and the new node we want to add
                            let common_len = r.left.as_ref().unwrap().prefix.common_len(r.left.as_ref().unwrap().len, &prefix, len);
                            let p = RadixTrie::<A, V>::new(prefix.mask(common_len), common_len);
                            r.splice(p);
                            r = r.left.as_mut().unwrap();
                            continue;
                        }
                    } else {
                        let mut n = RadixTrie::<A, V>::new(prefix.mask(len), len);
                        n.value = Some(value);
                        r.splice(n);
                        break;
                    }
                },
                RadixTrieBranch::Right => {
                    if r.right.is_some() {
                        if r.right.as_ref().unwrap().contains(prefix, len) {
                            r = r.right.as_mut().unwrap();
                            continue;   // follow left branch
                        } else {
                            // We need to add an intermediate parent, common to the existing left
                            // node and the new node we want to add
                            let common_len = r.right.as_ref().unwrap().prefix.common_len(r.right.as_ref().unwrap().len, &prefix, len);
                            let p = RadixTrie::<A, V>::new(prefix.mask(common_len), common_len);
                            r.splice(p);
                            r = r.right.as_mut().unwrap();
                            continue;
                        }
                    } else {
                        let mut n = RadixTrie::<A, V>::new(prefix.mask(len), len);
                        n.value = Some(value);
                        r.splice(n);
                        break;
                    }

                }
                _ => {}
            }
        }
    }

    pub fn aggregate(&mut self) {
        let max_len = self.prefix.max_len();
        if self.len < max_len {

            // Aggregate depper down the left and right branches first
            self.left.as_mut().map(|x| x.aggregate());
            self.right.as_mut().map(|x| x.aggregate());

            // If there are two direct descendents,
            // promote the current node with value
            if self.left.is_some() && self.right.is_some() {
                let left = self.left.as_ref().unwrap();
                let right = self.right.as_ref().unwrap();

                // and they are direct descendents, prime the current node
                // with a value
                if left.len == right.len && left.len == self.len + 1 {
                    if left.value.is_some() && right.value.is_some() {
                        if self.value.is_none() {
                            self.value = Some(Vec::new());
                        }
                    }
                }
            }

            // If the current node has value, then we can disregard
            // any child nodes.
            if self.value.is_some() {
                let _ = self.left.take();
                let _ = self.right.take();
            }
        }
    }

    pub fn get(&self, prefix: A, max_len: u8) -> Option<(A, u8, &Vec<V>)> {
        let max_len = max_len.min((std::mem::size_of::<A>()*8) as u8);
        let mut r: &RadixTrie<A, V> = self;
        let mut best: &RadixTrie<A, V> = self;
        while r.len <= max_len {
            match r.next_branch(prefix, max_len) {
                RadixTrieBranch::Left => {
                    best = r;
                    if r.left.is_some() {
                        r = r.left.as_ref().unwrap();
                        continue;
                    } else {
                        break;
                    }
                },
                RadixTrieBranch::Right => {
                    best = r;
                    if r.right.is_some() {
                        r = r.right.as_ref().unwrap();
                        continue;
                    } else {
                        break;
                    }
                },
                RadixTrieBranch::ExactMatch => {
                    best = r;
                    break;
                },
                _ => { break; }
            }
        }
        if let Some(value) = best.value.as_ref() {
            Some((r.prefix, r.len, value))
        } else {
            None
        }
    }

    pub fn iter(&self) -> RadixTrieIterator<'_, A, V> {
        RadixTrieIterator {
            nodes: vec![(self, RadixTrieIteratorState::R)],
        }
    }
}

#[derive(PartialEq)]
enum RadixTrieIteratorState { R, Left, Right }
pub struct RadixTrieIterator<'a, A: Copy + ipaddrmask::IpAddrMask, V: PartialEq> {
    nodes: Vec<(&'a RadixTrie<A, V>, RadixTrieIteratorState)>,
}

impl <'a, A: IpAddrMask + Copy + Display, V: PartialEq> Iterator for RadixTrieIterator<'a, A, V> {
    type Item = (A, u8, Option<&'a Vec<V>>);
    fn next(&mut self) -> Option<Self::Item> {
        while let Some((node, state)) = self.nodes.pop() {
            match state {
                RadixTrieIteratorState::R => {
                    self.nodes.push((node, RadixTrieIteratorState::Left));
                    if let Some(value) = node.value.as_ref() {
                        return Some((node.prefix, node.len, Some(value)))
                    } else {
                        continue;
                    }
                },
                RadixTrieIteratorState::Left => {
                    self.nodes.push((node, RadixTrieIteratorState::Right));
                    if let Some(left) = node.left.as_ref() {
                        self.nodes.push((left, RadixTrieIteratorState::R));
                    }
                    return self.next()
                },
                RadixTrieIteratorState::Right => {
                    if let Some(right) = node.right.as_ref() {
                        self.nodes.push((right, RadixTrieIteratorState::R));
                    }
                    return self.next()
                }
            }
        }
        None
    }
}