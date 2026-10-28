//! Remembers the last few IDs handled, so a resent request or event isn't acted on twice (protocol.md §5.0a).

/// Whether an ID was among the last ones remembered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Seen {
    New,    // not among them; now remembered
    Repeat, // already handled: a resend
}

/// The last `N` IDs handled, of any type: `RequestId` on the drone, `EventId` and answered `RequestId`s on the client.
#[derive(Clone, Copy, Debug)]
pub struct RecentIds<Id, const N: usize> {
    ids: [Option<Id>; N], // a ring: `next` is the oldest, overwritten by the next new ID
    next: usize,
}

impl<Id: Copy + Eq, const N: usize> RecentIds<Id, N> {
    pub fn new() -> Self {
        const { assert!(N > 0, "RecentIds must remember at least one ID") };
        return Self {
            ids: [None; N],
            next: 0,
        };
    }

    /// Remembers `id` if it is new, forgetting the oldest one once `N` are remembered. A repeat changes nothing.
    pub fn remember(&mut self, id: Id) -> Seen {
        if self.contains(id) {
            return Seen::Repeat;
        }
        self.ids[self.next] = Some(id);
        self.next = (self.next + 1) % N;
        return Seen::New;
    }

    pub fn contains(&self, id: Id) -> bool {
        return self.ids.contains(&Some(id));
    }
}

impl<Id: Copy + Eq, const N: usize> Default for RecentIds<Id, N> {
    fn default() -> Self {
        return Self::new();
    }
}
