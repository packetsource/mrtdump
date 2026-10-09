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

    // Insert `node` into `slot` (a left- or right-child slot the caller has
    // already decided on), re-connecting whatever was already in that slot
    // underneath the new node. The caller supplies the slot directly rather
    // than a branch to look up, since it invariably already knows which side
    // it wants — that avoids recomputing next_branch(self, node) here.
    fn splice(slot: &mut Option<Box<RadixTrie<A, V>>>, mut node: RadixTrie<A, V>) {
        if let Some(existing) = slot.take() {
            match node.next_branch(existing.prefix, existing.len) {
                RadixTrieBranch::Left => {
                    node.left = Some(existing);
                },
                RadixTrieBranch::Right => {
                    node.right = Some(existing);
                },
                _ => {}
            }
        }
        *slot = Some(Box::new(node));
    }

    // Walk to (or create) the node for the exact (prefix, len), without attaching a value.
    fn find_or_insert(&mut self, prefix: A, len: u8) -> &mut RadixTrie<A, V> {
        let mut r = self;
        loop {
            let branch = r.next_branch(prefix, len);
            // eprintln!("{}/{}: next branch is {}/{}:{:?}", prefix, len, r.prefix, r.len, branch);
            match branch {
                RadixTrieBranch::ExactMatch => {
                    return r;
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
                            RadixTrie::splice(&mut r.left, p);
                            r = r.left.as_mut().unwrap();
                            continue;
                        }
                    } else {
                        let n = RadixTrie::<A, V>::new(prefix.mask(len), len);
                        RadixTrie::splice(&mut r.left, n);
                        r = r.left.as_mut().unwrap();
                        continue;
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
                            RadixTrie::splice(&mut r.right, p);
                            r = r.right.as_mut().unwrap();
                            continue;
                        }
                    } else {
                        let n = RadixTrie::<A, V>::new(prefix.mask(len), len);
                        RadixTrie::splice(&mut r.right, n);
                        r = r.right.as_mut().unwrap();
                        continue;
                    }

                }
                _ => {}
            }
        }
    }

    pub fn add(&mut self, prefix: A, len: u8, value: V) {
        let node = self.find_or_insert(prefix, len);
        match node.value.as_mut() {
            Some(v) => v.push(value),
            None => node.value = Some(vec![value]),
        }
    }

    pub fn add_vec(&mut self, prefix: A, len: u8, mut value: Vec<V>) {
        let node = self.find_or_insert(prefix, len);
        match node.value.as_mut() {
            Some(v) => v.append(&mut value),
            None => node.value = Some(value),
        }
    }

    // Remove the value stored at the exact (prefix, len) node, if present,
    // and return it. A no-op (returns None) if that exact node either
    // doesn't exist or carries no value.
    //
    // Afterwards, keeps the trie maximally compressed: a child left with
    // neither a value nor children is dropped, and a child left with no
    // value and exactly one child is collapsed — that child is spliced
    // directly into the slot in its place, eliding the now-redundant
    // pass-through node (mirroring what find_or_insert's intermediate nodes
    // look like right after being created, just in reverse). normalize() is
    // applied at every level as the recursion unwinds, so a cascade of
    // empty/single-child nodes collapses all the way up in one pass. The
    // root is exempt, since it isn't Option-wrapped and can't be removed.
    pub fn delete(&mut self, prefix: A, len: u8) -> Option<Vec<V>> {
        let slot = match self.next_branch(prefix, len) {
            RadixTrieBranch::ExactMatch => return self.value.take(),
            RadixTrieBranch::Left => &mut self.left,
            RadixTrieBranch::Right => &mut self.right,
            RadixTrieBranch::NoMatch => return None,
        };
        let removed = slot.as_mut().and_then(|child| child.delete(prefix, len));
        if removed.is_some() {
            Self::normalize(slot);
        }
        removed
    }

    // Drop or collapse the node in `slot` if it's now redundant: gone if it
    // carries neither a value nor children, replaced by its sole child if
    // it carries no value and exactly one child, left alone otherwise.
    fn normalize(slot: &mut Option<Box<RadixTrie<A, V>>>) {
        let Some(mut child) = slot.take() else { return };
        if child.value.is_some() {
            *slot = Some(child);
            return;
        }
        *slot = match (child.left.take(), child.right.take()) {
            (None, None) => None,
            (Some(only), None) | (None, Some(only)) => Some(only),
            (left, right) => {
                child.left = left;
                child.right = right;
                Some(child)
            }
        };
    }

    fn shrink_value(&mut self) {
        if let Some(v) = self.value.as_mut() {
            v.shrink_to_fit();
        }
    }

    // Reclaim any spare Vec capacity left over from incremental insertion
    // (Vec's doubling growth), across every node. Independent of aggregate() —
    // safe to call on its own once insertions are complete.
    pub fn squeeze(&mut self) {
        self.shrink_value();
        if let Some(left) = self.left.as_mut() {
            left.squeeze();
        }
        if let Some(right) = self.right.as_mut() {
            right.squeeze();
        }
    }

    // Aggregate the trie down to its minimal representation: two mechanics,
    // applied bottom-up.
    //
    // Promote: if self has no value of its own, but both children are
    // direct descendents (self.len + 1) and both already carry a value,
    // together they exactly and exhaustively tile self's whole address
    // range — synthesize a combined value at self from them. Adjacency is
    // essential here: it's the only way to be sure there's no unaddressed
    // gap between them that self's new value would incorrectly claim to
    // cover.
    //
    // Absorb: once self has a value — whether it started with one, or just
    // got one from promotion above — every node still beneath it is, by
    // the trie's own structural invariant, a strict subset of self's
    // address range, at any depth, adjacent or not. So it's always safe to
    // fold every remaining descendant value into self and drop the rest of
    // the subtree, which is the actual point of aggregate(): the minimal
    // set of prefixes describing everything known. (This does mean a
    // covering value and a more-specific one beneath it become
    // indistinguishable after aggregation — by design, for this use case.)
    pub fn aggregate(&mut self) {
        // Also squeeze this node's value while we're visiting it, since
        // aggregate() already walks every node once.
        self.shrink_value();

        let max_len = self.prefix.max_len();
        if self.len < max_len {

            // Aggregate depper down the left and right branches first
            self.left.as_mut().map(|x| x.aggregate());
            self.right.as_mut().map(|x| x.aggregate());

            if self.value.is_none() {
                let promotable = matches!(
                    (self.left.as_ref(), self.right.as_ref()),
                    (Some(left), Some(right))
                        if left.len == self.len + 1
                            && right.len == self.len + 1
                            && left.value.is_some()
                            && right.value.is_some()
                );
                if promotable {
                    let mut left_value = self.left.as_mut().unwrap().value.take().unwrap();
                    let mut right_value = self.right.as_mut().unwrap().value.take().unwrap();
                    left_value.append(&mut right_value);
                    self.value = Some(left_value);
                    self.left = None;
                    self.right = None;
                }
            }

            if let Some(value) = self.value.as_mut() {
                if let Some(mut left) = self.left.take() {
                    left.drain_values_into(value);
                }
                if let Some(mut right) = self.right.take() {
                    right.drain_values_into(value);
                }
            }
        }
    }

    // Recursively drain every value in this subtree (this node's own, if
    // any, then left, then right) into `out`, consuming the subtree as it
    // goes. Used by aggregate()'s absorb step.
    fn drain_values_into(&mut self, out: &mut Vec<V>) {
        if let Some(value) = self.value.take() {
            out.extend(value);
        }
        if let Some(mut left) = self.left.take() {
            left.drain_values_into(out);
        }
        if let Some(mut right) = self.right.take() {
            right.drain_values_into(out);
        }
    }

    pub fn get(&self, prefix: A, max_len: u8) -> Option<(A, u8, &Vec<V>)> {
        let max_len = max_len.min((std::mem::size_of::<A>()*8) as u8);
        let mut r: &RadixTrie<A, V> = self;
        let mut best: &RadixTrie<A, V> = self;
        while r.len <= max_len {
            match r.next_branch(prefix, max_len) {
                RadixTrieBranch::Left => {
                    if r.value.is_some() {
                        best = r;
                    }
                    if r.left.is_some() {
                        r = r.left.as_ref().unwrap();
                        continue;
                    } else {
                        break;
                    }
                },
                RadixTrieBranch::Right => {
                    if r.value.is_some() {
                        best = r;
                    }
                    if r.right.is_some() {
                        r = r.right.as_ref().unwrap();
                        continue;
                    } else {
                        break;
                    }
                },
                RadixTrieBranch::ExactMatch => {
                    if r.value.is_some() {
                        best = r;
                    }
                    break;
                },
                _ => { break; }
            }
        }
        if let Some(value) = best.value.as_ref() {
            Some((best.prefix, best.len, value))
        } else {
            None
        }
    }

    pub fn iter(&self) -> RadixTrieIterator<'_, A, V> {
        RadixTrieIterator {
            nodes: vec![(self, RadixTrieIteratorState::R)],
        }
    }

    // Walk every node that carries a value, handing each one's Vec<V> to `f`
    // for in-place mutation (e.g. `Vec::retain`). Unlike an external Iterator
    // over &mut, this needs no unsafe: recursion lets the borrow checker see
    // that self.value, self.left and self.right are disjoint fields.
    // Doesn't restructure the tree — a node whose Vec is emptied by `f` is
    // reset to `value: None` (preserving the invariant that Some(_) implies
    // non-empty) but the node itself is left in place.
    pub fn for_each_value_mut<F: FnMut(A, u8, &mut Vec<V>)>(&mut self, f: &mut F) {
        if let Some(value) = self.value.as_mut() {
            f(self.prefix, self.len, value);
            if value.is_empty() {
                self.value = None;
            }
        }
        if let Some(left) = self.left.as_mut() {
            left.for_each_value_mut(f);
        }
        if let Some(right) = self.right.as_mut() {
            right.for_each_value_mut(f);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    // Depth-first count of nodes actually present in the trie (root included) —
    // used to prove structural collapse happened, since get()/iter() alone
    // can't distinguish a collapsed trie from one with redundant pass-through
    // nodes still hanging around.
    fn node_count<V: PartialEq>(node: &RadixTrie<Ipv4Addr, V>) -> usize {
        1 + node.left.as_ref().map_or(0, |n| node_count(n))
              + node.right.as_ref().map_or(0, |n| node_count(n))
    }

    // Locate the node at an exact (prefix, len), wherever it landed in the
    // tree — lets assertions check a specific node's shape without assuming
    // which side of a parent it was spliced onto.
    fn find<'a, V: PartialEq>(node: &'a RadixTrie<Ipv4Addr, V>, prefix: Ipv4Addr, len: u8) -> Option<&'a RadixTrie<Ipv4Addr, V>> {
        if node.prefix == prefix && node.len == len {
            return Some(node);
        }
        node.left.as_deref().and_then(|n| find(n, prefix, len))
            .or_else(|| node.right.as_deref().and_then(|n| find(n, prefix, len)))
    }

    #[test]
    fn delete_collapses_redundant_intermediate_node() {
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 24, 1);
        t.add(ip("10.0.1.0"), 24, 2);

        // root -> intermediate 10.0.0.0/23 -> {10.0.0.0/24, 10.0.1.0/24}
        assert_eq!(node_count(&t), 4);
        let mid = t.left.as_ref().or(t.right.as_ref()).unwrap();
        assert_eq!(mid.len, 23);

        let removed = t.delete(ip("10.0.1.0"), 24);
        assert_eq!(removed, Some(vec![2]));

        // The now-redundant /23 node should be gone; its surviving sibling
        // (10.0.0.0/24) should be spliced directly into its old slot.
        assert_eq!(node_count(&t), 2);
        let child = t.left.as_ref().or(t.right.as_ref()).unwrap();
        assert_eq!((child.prefix, child.len), (ip("10.0.0.0"), 24));
        assert_eq!(child.value, Some(vec![1]));

        assert_eq!(t.get(ip("10.0.0.5"), 32).unwrap().2, &vec![1]);
        assert!(t.get(ip("10.0.1.5"), 32).is_none());
    }

    #[test]
    fn delete_removes_leaf_entirely() {
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 24, 1);
        assert_eq!(node_count(&t), 2);

        let removed = t.delete(ip("10.0.0.0"), 24);
        assert_eq!(removed, Some(vec![1]));
        assert_eq!(node_count(&t), 1); // just the root
        assert!(t.left.is_none() && t.right.is_none());
    }

    #[test]
    fn delete_missing_prefix_is_a_noop() {
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 24, 1);

        assert_eq!(t.delete(ip("10.0.1.0"), 24), None);
        assert_eq!(node_count(&t), 2);
        assert_eq!(t.get(ip("10.0.0.5"), 32).unwrap().2, &vec![1]);
    }

    #[test]
    fn delete_preserves_value_at_branching_node() {
        // A node can carry its own value AND have children (e.g. an explicit
        // 10.0.0.0/23 plus a more specific 10.0.0.0/24 within it). Deleting
        // the child must leave the parent's own value alone rather than
        // collapsing/removing it.
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 23, 100);
        t.add(ip("10.0.0.0"), 24, 1);

        let removed = t.delete(ip("10.0.0.0"), 24);
        assert_eq!(removed, Some(vec![1]));
        assert_eq!(t.get(ip("10.0.0.5"), 32).unwrap().2, &vec![100]);
    }

    #[test]
    fn aggregate_merges_direct_sibling_leaves() {
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 24, 1);
        t.add(ip("10.0.1.0"), 24, 2);
        assert_eq!(node_count(&t), 4); // root, /23 parent, two /24 leaves

        t.aggregate();

        // Both /24 leaves are direct (len+1) siblings under the /23, and
        // both carry values, so they should merge up and be pruned.
        assert_eq!(node_count(&t), 2);
        let merged = find(&t, ip("10.0.0.0"), 23).expect("merged /23 node");
        assert_eq!(merged.value, Some(vec![1, 2]));
        assert!(merged.left.is_none() && merged.right.is_none());

        assert_eq!(t.get(ip("10.0.0.5"), 32).unwrap().2, &vec![1, 2]);
        assert_eq!(t.get(ip("10.0.1.5"), 32).unwrap().2, &vec![1, 2]);
    }

    #[test]
    fn aggregate_prepends_existing_parent_value() {
        // The /23 itself is an explicit route (its own value), on top of the
        // two /24 children below it. Merge order should be parent-first,
        // then left, then right.
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 23, 99);
        t.add(ip("10.0.0.0"), 24, 1);
        t.add(ip("10.0.1.0"), 24, 2);

        t.aggregate();

        let merged = find(&t, ip("10.0.0.0"), 23).expect("merged /23 node");
        assert_eq!(merged.value, Some(vec![99, 1, 2]));
        assert!(merged.left.is_none() && merged.right.is_none());
    }

    #[test]
    fn aggregate_does_not_merge_across_a_length_gap() {
        // These two /32s only share a /14 common ancestor -- nowhere near
        // "immediate siblings" (self.len + 1) -- so aggregate() must leave
        // them as separate, unmerged leaves rather than over-claiming the
        // whole /14 as covered by their combined values.
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.1.1.1"), 32, 1);
        t.add(ip("10.2.2.2"), 32, 2);
        let before = node_count(&t);

        t.aggregate();

        assert_eq!(node_count(&t), before);
        let parent = find(&t, ip("10.0.0.0"), 14).expect("common /14 ancestor");
        assert!(parent.value.is_none());
        assert!(parent.left.is_some() && parent.right.is_some());

        assert_eq!(t.get(ip("10.1.1.1"), 32).unwrap().2, &vec![1]);
        assert_eq!(t.get(ip("10.2.2.2"), 32).unwrap().2, &vec![2]);
        // If aggregate() had wrongly merged, this unrelated address inside
        // the /14 would incorrectly resolve to a value.
        assert!(t.get(ip("10.1.200.1"), 32).is_none());
    }

    #[test]
    fn aggregate_does_not_merge_asymmetric_siblings() {
        // Three of a full /24's four /26 quadrants are populated. The first
        // pair (adjacent, both /26) should merge into a /25. The lone third
        // quadrant stays a /26. Since /25 != /26, the auto-created /24
        // branch point above them must NOT attempt to merge those two,
        // despite both now carrying values after their own aggregation.
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 26, 1);
        t.add(ip("10.0.0.64"), 26, 2);
        t.add(ip("10.0.0.128"), 26, 3);

        t.aggregate();

        let quad_pair = find(&t, ip("10.0.0.0"), 25).expect("merged /25 node");
        assert_eq!(quad_pair.value, Some(vec![1, 2]));
        assert!(quad_pair.left.is_none() && quad_pair.right.is_none());

        let lone_quad = find(&t, ip("10.0.0.128"), 26).expect("unmerged /26 node");
        assert_eq!(lone_quad.value, Some(vec![3]));

        // The auto-created /24 above them was never given its own value, so
        // it must survive with both children intact rather than being
        // pruned.
        let branch = find(&t, ip("10.0.0.0"), 24).expect("/24 branch point");
        assert!(branch.value.is_none());
        assert!(branch.left.is_some() && branch.right.is_some());
    }

    #[test]
    fn aggregate_cascades_multiple_levels() {
        // All four /26 quadrants of 10.0.0.0/24 are populated: the two
        // adjacent pairs should each merge into their /25s, and those two
        // /25s -- now both carrying values, both len+1 below the /24 -- must
        // then merge again into a single /24 node.
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 26, 1);
        t.add(ip("10.0.0.64"), 26, 2);
        t.add(ip("10.0.0.128"), 26, 3);
        t.add(ip("10.0.0.192"), 26, 4);

        t.aggregate();

        assert_eq!(node_count(&t), 2); // root + the single fully-merged /24
        let merged = find(&t, ip("10.0.0.0"), 24).expect("merged /24 node");
        let mut values = merged.value.clone().expect("merged value");
        values.sort();
        assert_eq!(values, vec![1, 2, 3, 4]);
        assert!(merged.left.is_none() && merged.right.is_none());
    }

    #[test]
    fn aggregate_absorbs_non_adjacent_descendant_into_parent_value() {
        // An explicit route at 10.0.0.0/23 with a far more-specific
        // 10.0.0.0/32 child underneath it (not a direct len+1 descendant)
        // should absorb that child's value into itself, since the /32's
        // range is always a strict subset of the /23's by construction.
        // Every address in the /23 -- whether or not it was the specific
        // /32 -- resolves to the same combined value afterwards; that's the
        // intended effect of aggregate() as a minimal-representation pass,
        // not a bug.
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 23, 99);
        t.add(ip("10.0.0.0"), 32, 1);

        t.aggregate();

        assert_eq!(node_count(&t), 2); // root + the single absorbed /23
        let merged = find(&t, ip("10.0.0.0"), 23).expect("absorbing /23 node");
        assert_eq!(merged.value, Some(vec![99, 1]));
        assert!(merged.left.is_none() && merged.right.is_none());

        assert_eq!(t.get(ip("10.0.0.0"), 32).unwrap().2, &vec![99, 1]);
        assert_eq!(t.get(ip("10.0.0.1"), 32).unwrap().2, &vec![99, 1]);
    }

    #[test]
    fn aggregate_absorbs_lone_direct_child_into_parent_value() {
        // Sharper variant of the same mechanic: here the child IS the
        // immediate next step (self.len + 1), but has no sibling on the
        // other side, so it can't trigger promotion. It should still be
        // absorbed once self already carries its own value -- absorption
        // doesn't need adjacency or full coverage, only that self has a
        // value to fold into.
        let mut t = RadixTrie::<Ipv4Addr, i32>::new(ip("0.0.0.0"), 0);
        t.add(ip("10.0.0.0"), 23, 99);
        t.add(ip("10.0.0.0"), 24, 1);

        t.aggregate();

        assert_eq!(node_count(&t), 2);
        let merged = find(&t, ip("10.0.0.0"), 23).expect("absorbing /23 node");
        assert_eq!(merged.value, Some(vec![99, 1]));
        assert!(merged.left.is_none() && merged.right.is_none());

        assert_eq!(t.get(ip("10.0.0.1"), 32).unwrap().2, &vec![99, 1]);
        assert_eq!(t.get(ip("10.0.1.1"), 32).unwrap().2, &vec![99, 1]);
    }
}
